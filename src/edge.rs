use axum::{
    extract::FromRequestParts,
    http::{request::Parts, HeaderMap, Method, Uri},
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use uuid::Uuid;

use crate::{error::AppError, AppState};

type HmacSha256 = Hmac<Sha256>;

const USER_HEADER: &str = "x-canonical-edge-user-id";
const EMAIL_HEADER: &str = "x-canonical-edge-email";
const ISSUED_AT_HEADER: &str = "x-canonical-edge-issued-at";
const ASSERTION_HEADER: &str = "x-canonical-edge-assertion";

#[derive(Clone, Debug)]
pub struct EdgeIdentity {
    pub user_id: Uuid,
    pub email: String,
}

pub struct EdgeAuthenticated(pub EdgeIdentity);

impl FromRequestParts<AppState> for EdgeAuthenticated {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        verify_assertion(
            &parts.method,
            &parts.uri,
            &parts.headers,
            &state.config.origin_assertion_secret,
            state.config.origin_assertion_max_age_seconds,
            chrono::Utc::now().timestamp(),
        )
        .map(Self)
    }
}

pub(crate) fn signed_headers(
    method: &Method,
    path_and_query: &str,
    identity: &EdgeIdentity,
    secret: &[u8],
) -> Result<HeaderMap, AppError> {
    let issued_at = chrono::Utc::now().timestamp();
    let payload = assertion_payload(
        method.as_str(),
        path_and_query,
        issued_at,
        &identity.user_id.to_string(),
        &identity.email,
    );
    let mut mac =
        <HmacSha256 as Mac>::new_from_slice(secret).map_err(|_| AppError::Crypto)?;
    mac.update(payload.as_bytes());
    let signature = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
    let mut headers = HeaderMap::new();
    headers.insert(
        USER_HEADER,
        identity
            .user_id
            .to_string()
            .parse()
            .map_err(|_| AppError::Crypto)?,
    );
    if !identity.email.is_empty() {
        headers.insert(
            EMAIL_HEADER,
            identity.email.parse().map_err(|_| AppError::Crypto)?,
        );
    }
    headers.insert(
        ISSUED_AT_HEADER,
        issued_at
            .to_string()
            .parse()
            .map_err(|_| AppError::Crypto)?,
    );
    headers.insert(
        ASSERTION_HEADER,
        signature.parse().map_err(|_| AppError::Crypto)?,
    );
    Ok(headers)
}

fn verify_assertion(
    method: &Method,
    uri: &Uri,
    headers: &HeaderMap,
    secret: &[u8],
    maximum_age_seconds: i64,
    now: i64,
) -> Result<EdgeIdentity, AppError> {
    let user = header_string(headers, USER_HEADER, 64)?;
    let user_id = Uuid::parse_str(&user).map_err(|_| AppError::Unauthorized)?;
    let email = optional_header_string(headers, EMAIL_HEADER, 320)?;
    let issued_at = header_string(headers, ISSUED_AT_HEADER, 20)?
        .parse::<i64>()
        .map_err(|_| AppError::Unauthorized)?;
    let age = now.checked_sub(issued_at).ok_or(AppError::Unauthorized)?;
    if age < -5 || age > maximum_age_seconds {
        return Err(AppError::Unauthorized);
    }
    let signature = URL_SAFE_NO_PAD
        .decode(header_string(headers, ASSERTION_HEADER, 128)?)
        .map_err(|_| AppError::Unauthorized)?;
    let path_and_query = uri
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or(uri.path());
    let payload = assertion_payload(
        method.as_str(),
        path_and_query,
        issued_at,
        &user,
        &email,
    );
    let mut mac =
        <HmacSha256 as Mac>::new_from_slice(secret).map_err(|_| AppError::Crypto)?;
    mac.update(payload.as_bytes());
    mac.verify_slice(&signature)
        .map_err(|_| AppError::Unauthorized)?;
    Ok(EdgeIdentity { user_id, email })
}

fn header_string(
    headers: &HeaderMap,
    name: &'static str,
    maximum: usize,
) -> Result<String, AppError> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .filter(|value| {
            !value.is_empty()
                && value.len() <= maximum
                && !value.contains('\r')
                && !value.contains('\n')
        })
        .map(str::to_owned)
        .ok_or(AppError::Unauthorized)
}

fn optional_header_string(
    headers: &HeaderMap,
    name: &'static str,
    maximum: usize,
) -> Result<String, AppError> {
    match headers.get(name) {
        Some(value) => {
            let value = value.to_str().map_err(|_| AppError::Unauthorized)?;
            if value.len() > maximum || value.contains('\r') || value.contains('\n') {
                return Err(AppError::Unauthorized);
            }
            Ok(value.to_owned())
        }
        None => Ok(String::new()),
    }
}

fn assertion_payload(
    method: &str,
    path: &str,
    issued_at: i64,
    user: &str,
    email: &str,
) -> String {
    format!(
        "{}\n{}\n{}\n{}\n{}",
        method.to_uppercase(),
        path,
        issued_at,
        user,
        email
    )
}

#[cfg(test)]
mod tests {
    use super::assertion_payload;

    #[test]
    fn payload_matches_the_worker_and_api_contract() {
        assert_eq!(
            assertion_payload(
                "post",
                "/v1/quotes",
                1_700_000_000,
                "2af35aef-d4a4-4fd4-9504-8f588e51e7ed",
                "person@example.com",
            ),
            "POST\n/v1/quotes\n1700000000\n2af35aef-d4a4-4fd4-9504-8f588e51e7ed\nperson@example.com"
        );
    }
}
