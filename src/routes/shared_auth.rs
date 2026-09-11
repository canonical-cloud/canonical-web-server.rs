use axum::{
    extract::{Query, State},
    response::{IntoResponse, Redirect, Response},
};
use axum_extra::extract::cookie::{time, Cookie, CookieJar, SameSite};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{
    auth::{AuthProviderError, AuthTokens, OptionalAuthenticated},
    error::AppError,
    integrations::SharedAuthRequestError,
    AppState,
};

const SECURE_STATE_COOKIE: &str = "__Host-canonical_auth_state";
const SECURE_PKCE_COOKIE: &str = "__Host-canonical_auth_pkce";
const SECURE_RETURN_COOKIE: &str = "__Host-canonical_auth_return";
const LOCAL_STATE_COOKIE: &str = "canonical_auth_state";
const LOCAL_PKCE_COOKIE: &str = "canonical_auth_pkce";
const LOCAL_RETURN_COOKIE: &str = "canonical_auth_return";
const HANDOFF_MAX_AGE_MINUTES: i64 = 10;
const QUOTE_RETURN_PATH: &str = "/u/quote";

#[derive(Deserialize)]
pub struct StartQuery {
    return_to: Option<String>,
}

pub async fn start(
    State(state): State<AppState>,
    jar: CookieJar,
    OptionalAuthenticated(existing): OptionalAuthenticated,
    Query(query): Query<StartQuery>,
) -> Result<Response, AppError> {
    let return_to = query.return_to.as_deref().unwrap_or(QUOTE_RETURN_PATH);
    validate_return_path(return_to)?;
    if existing.is_some() {
        return Ok(Redirect::to(return_to).into_response());
    }

    let state_token = random_token();
    let verifier = random_token();
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let location = state
        .shared_auth
        .authorization_url(return_to, &state_token, &challenge)
        .map_err(map_shared_auth_error)?;
    let (state_cookie, pkce_cookie, return_cookie) = cookie_names(state.config.cookie_secure);
    let jar = jar
        .add(handoff_cookie(
            state_cookie,
            state_token,
            state.config.cookie_secure,
        ))
        .add(handoff_cookie(
            pkce_cookie,
            verifier,
            state.config.cookie_secure,
        ))
        .add(handoff_cookie(
            return_cookie,
            return_to.to_owned(),
            state.config.cookie_secure,
        ));
    Ok((jar, Redirect::to(&location)).into_response())
}

#[derive(Deserialize)]
pub struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

pub async fn callback(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(query): Query<CallbackQuery>,
) -> Result<Response, AppError> {
    if query.error.is_some() {
        return Err(AppError::Unauthorized);
    }
    let code = query.code.as_deref().ok_or(AppError::Unauthorized)?;
    let returned_state = query.state.as_deref().ok_or(AppError::Unauthorized)?;
    if !code.starts_with("sac_")
        || code.len() > 128
        || !(16..=512).contains(&returned_state.len())
        || !is_base64url(returned_state)
    {
        return Err(AppError::Unauthorized);
    }

    let (state_cookie, pkce_cookie, return_cookie) = cookie_names(state.config.cookie_secure);
    let expected_state = jar
        .get(state_cookie)
        .map(Cookie::value)
        .ok_or(AppError::Unauthorized)?;
    let verifier = jar
        .get(pkce_cookie)
        .map(Cookie::value)
        .filter(|value| value.len() == 43 && is_base64url(value))
        .ok_or(AppError::Unauthorized)?;
    let return_to = jar
        .get(return_cookie)
        .map(Cookie::value)
        .ok_or(AppError::Unauthorized)?;
    validate_return_path(return_to)?;
    if !opaque_eq(expected_state, returned_state) {
        return Err(AppError::Unauthorized);
    }

    let handoff = state
        .shared_auth
        .redeem(code, verifier)
        .await
        .map_err(map_shared_auth_error)?;
    if handoff.return_to != return_to {
        return Err(AppError::Unauthorized);
    }

    // shared-auth already verifies against the client-assigned Supabase project,
    // but Canonical remains the authority for its own browser session. Verify
    // the redeemed access token again with Canonical's configured provider and
    // compare the exact subject before inserting an encrypted local session.
    let verified_user = match state.auth.user_for_token(&handoff.access_token).await {
        Ok(user) => user,
        Err(AuthProviderError::InvalidCredentials) => return Err(AppError::Unauthorized),
        Err(error) => {
            tracing::warn!(%error, "Canonical verification of shared-auth token failed");
            return Err(AppError::AuthUpstream);
        }
    };
    if verified_user.id != handoff.user.id
        || verified_user.email.as_deref().is_none_or(|email| email.len() > 320)
        || handoff
            .user
            .email
            .as_deref()
            .zip(verified_user.email.as_deref())
            .is_some_and(|(left, right)| !left.eq_ignore_ascii_case(right))
    {
        return Err(AppError::Unauthorized);
    }

    let created = state
        .sessions
        .create(AuthTokens {
            access_token: handoff.access_token,
            refresh_token: handoff.refresh_token,
            expires_at: handoff.expires_at,
            user: verified_user,
        })
        .await?;
    tracing::info!(user_id = %created.context.user_id, "shared-auth handoff succeeded");
    let max_age = time::Duration::seconds(
        state
            .config
            .session_ttl
            .as_secs()
            .try_into()
            .unwrap_or(i64::MAX),
    );
    let jar = jar
        .remove(removal_cookie(state_cookie, state.config.cookie_secure))
        .remove(removal_cookie(pkce_cookie, state.config.cookie_secure))
        .remove(removal_cookie(return_cookie, state.config.cookie_secure))
        .add(
            Cookie::build((state.config.session_cookie.clone(), created.raw_id))
                .path("/")
                .secure(state.config.cookie_secure)
                .http_only(true)
                .same_site(SameSite::Lax)
                .max_age(max_age)
                .build(),
        );
    Ok((jar, Redirect::to(return_to)).into_response())
}

fn validate_return_path(value: &str) -> Result<(), AppError> {
    if value == QUOTE_RETURN_PATH {
        Ok(())
    } else {
        Err(AppError::BadRequest(
            "return_to is not registered for this application".into(),
        ))
    }
}

fn map_shared_auth_error(error: SharedAuthRequestError) -> AppError {
    match error {
        SharedAuthRequestError::Unauthorized => AppError::Unauthorized,
        SharedAuthRequestError::Unavailable | SharedAuthRequestError::InvalidResponse => {
            tracing::warn!(%error, "shared-auth handoff failed");
            AppError::AuthUpstream
        }
    }
}

fn cookie_names(secure: bool) -> (&'static str, &'static str, &'static str) {
    if secure {
        (
            SECURE_STATE_COOKIE,
            SECURE_PKCE_COOKIE,
            SECURE_RETURN_COOKIE,
        )
    } else {
        (LOCAL_STATE_COOKIE, LOCAL_PKCE_COOKIE, LOCAL_RETURN_COOKIE)
    }
}

fn handoff_cookie(name: &'static str, value: String, secure: bool) -> Cookie<'static> {
    Cookie::build((name, value))
        .path("/")
        .secure(secure)
        .http_only(true)
        .same_site(SameSite::Lax)
        .max_age(time::Duration::minutes(HANDOFF_MAX_AGE_MINUTES))
        .build()
}

fn removal_cookie(name: &'static str, secure: bool) -> Cookie<'static> {
    Cookie::build((name, String::new()))
        .path("/")
        .secure(secure)
        .http_only(true)
        .same_site(SameSite::Lax)
        .max_age(time::Duration::ZERO)
        .build()
}

fn random_token() -> String {
    URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>())
}

fn is_base64url(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn opaque_eq(expected: &str, provided: &str) -> bool {
    Sha256::digest(expected.as_bytes()) == Sha256::digest(provided.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::{opaque_eq, validate_return_path};

    #[test]
    fn return_path_is_an_exact_allowlist() {
        assert!(validate_return_path("/u/quote").is_ok());
        assert!(validate_return_path("//evil.example").is_err());
        assert!(validate_return_path("/u/quote?next=https://evil.example").is_err());
    }

    #[test]
    fn opaque_state_comparison_is_exact() {
        assert!(opaque_eq("state-value", "state-value"));
        assert!(!opaque_eq("state-value", "state-valuE"));
    }
}
