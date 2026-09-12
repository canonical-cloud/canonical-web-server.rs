use crate::{error::AppError, AppState};
use axum::{
    extract::FromRequestParts,
    http::{request::Parts, HeaderMap},
};
use sha2::{Digest, Sha256};

const EDGE_SECRET_HEADER: &str = "x-auth-edge-secret";
const SUBJECT_HEADER: &str = "x-auth-user-id";
const EMAIL_HEADER: &str = "x-auth-email";
const PROVIDER_HEADER: &str = "x-auth-provider";
const PROVIDER_TENANT_HEADER: &str = "x-auth-provider-tenant";
const PROJECT_HEADER: &str = "x-auth-project";
const ROLES_HEADER: &str = "x-auth-roles";

#[derive(Clone)]
pub struct EdgeAuthVerifier {
    expected_hash: Option<[u8; 32]>,
}

impl EdgeAuthVerifier {
    /// Loads the secret injected by the Cloudflare Worker. Missing configuration
    /// disables edge identity rather than weakening to unsigned headers.
    pub fn from_env() -> Result<Self, AppError> {
        let Ok(secret) = crate::config::flags::var("EDGE_AUTH_SHARED_SECRET") else {
            return Ok(Self {
                expected_hash: None,
            });
        };
        if !(32..=512).contains(&secret.len()) || secret.chars().any(char::is_control) {
            return Err(AppError::BadRequest(
                "EDGE_AUTH_SHARED_SECRET must contain 32 to 512 non-control characters".into(),
            ));
        }
        Ok(Self {
            expected_hash: Some(Sha256::digest(secret.as_bytes()).into()),
        })
    }

    fn verify(&self, supplied: &str) -> bool {
        let Some(expected) = self.expected_hash else {
            return false;
        };
        if !(32..=512).contains(&supplied.len()) || supplied.chars().any(char::is_control) {
            return false;
        }
        let actual: [u8; 32] = Sha256::digest(supplied.as_bytes()).into();
        constant_time_equal(&expected, &actual)
    }

    #[cfg(test)]
    fn for_test(secret: &str) -> Self {
        Self {
            expected_hash: Some(Sha256::digest(secret.as_bytes()).into()),
        }
    }
}

#[derive(Clone)]
pub struct EdgeIdentity {
    pub subject: String,
    pub email: Option<String>,
    pub provider: String,
    pub provider_tenant: String,
    pub project: Option<String>,
    pub roles: Vec<String>,
}

pub struct EdgeAuthenticated(pub EdgeIdentity);

impl FromRequestParts<AppState> for EdgeAuthenticated {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let supplied_secret = required_header(&parts.headers, EDGE_SECRET_HEADER, 512)?;
        if !state.edge_auth.verify(supplied_secret) {
            return Err(AppError::Unauthorized);
        }

        let subject = validated_header(&parts.headers, SUBJECT_HEADER, 512, true)?
            .ok_or(AppError::Unauthorized)?;
        let provider = validated_header(&parts.headers, PROVIDER_HEADER, 128, true)?
            .ok_or(AppError::Unauthorized)?;
        let provider_tenant = validated_header(&parts.headers, PROVIDER_TENANT_HEADER, 255, true)?
            .ok_or(AppError::Unauthorized)?;
        let email = validated_header(&parts.headers, EMAIL_HEADER, 320, false)?;
        let project = validated_header(&parts.headers, PROJECT_HEADER, 255, false)?;
        let roles = parse_roles(&parts.headers)?;

        Ok(Self(EdgeIdentity {
            subject,
            email,
            provider,
            provider_tenant,
            project,
            roles,
        }))
    }
}

fn required_header<'a>(
    headers: &'a HeaderMap,
    name: &'static str,
    max_len: usize,
) -> Result<&'a str, AppError> {
    let value = headers
        .get(name)
        .ok_or(AppError::Unauthorized)?
        .to_str()
        .map_err(|_| AppError::Unauthorized)?;
    if value.is_empty() || value.len() > max_len || value.chars().any(char::is_control) {
        return Err(AppError::Unauthorized);
    }
    Ok(value)
}

fn validated_header(
    headers: &HeaderMap,
    name: &'static str,
    max_len: usize,
    required: bool,
) -> Result<Option<String>, AppError> {
    let Some(raw) = headers.get(name) else {
        return if required {
            Err(AppError::Unauthorized)
        } else {
            Ok(None)
        };
    };
    let value = raw.to_str().map_err(|_| AppError::Unauthorized)?.trim();
    if value.is_empty() || value.len() > max_len || value.chars().any(char::is_control) {
        return Err(AppError::Unauthorized);
    }
    Ok(Some(value.to_owned()))
}

fn parse_roles(headers: &HeaderMap) -> Result<Vec<String>, AppError> {
    let Some(raw) = headers.get(ROLES_HEADER) else {
        return Ok(Vec::new());
    };
    let raw = raw.to_str().map_err(|_| AppError::Unauthorized)?;
    if raw.len() > 2_048 {
        return Err(AppError::Unauthorized);
    }
    let mut roles = Vec::new();
    for role in raw
        .split(',')
        .map(str::trim)
        .filter(|role| !role.is_empty())
    {
        if role.len() > 64
            || role.bytes().any(|byte| {
                !(byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':' | b'.'))
            })
        {
            return Err(AppError::Unauthorized);
        }
        if !roles.iter().any(|existing| existing == role) {
            roles.push(role.to_owned());
        }
        if roles.len() > 32 {
            return Err(AppError::Unauthorized);
        }
    }
    Ok(roles)
}

fn constant_time_equal(expected: &[u8; 32], actual: &[u8; 32]) -> bool {
    expected
        .iter()
        .zip(actual.iter())
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edge_secret_is_hashed_and_compared_without_prefix_matching() {
        let verifier = EdgeAuthVerifier::for_test("a-very-long-test-secret-that-is-not-production");
        assert!(verifier.verify("a-very-long-test-secret-that-is-not-production"));
        assert!(!verifier.verify("a-very-long-test-secret-that-is-not-productioN"));
        assert!(!verifier.verify("a-very-long-test-secret"));
    }

    #[test]
    fn roles_are_bounded_and_use_a_safe_alphabet() {
        let mut headers = HeaderMap::new();
        headers.insert(
            ROLES_HEADER,
            "customer,quote:read,quote.write".parse().unwrap(),
        );
        assert_eq!(
            parse_roles(&headers).unwrap(),
            vec!["customer", "quote:read", "quote.write"]
        );
        headers.insert(ROLES_HEADER, "customer,admin/spoof".parse().unwrap());
        assert!(parse_roles(&headers).is_err());
    }
}
