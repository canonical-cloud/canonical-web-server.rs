//! Protected Shared Auth introspection for quote-facing routes.
//!
//! The independent service credential lives only in the server-side official
//! client. It is never copied into a browser cookie, request URL, user token,
//! or product data-plane envelope.

use std::env;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{TimeZone as _, Utc};
use reqwest::Url;
use sha2::{Digest as _, Sha256};
use shared_auth_client::{ClientError, Introspection, SharedAuthClient};
use uuid::Uuid;

use super::{AuthContext, CredentialSource};
use crate::{config::Config, error::AppError};

const DEFAULT_SECURE_COOKIE: &str = "__Host-canonical-customer-auth";
const DEFAULT_LOOPBACK_COOKIE: &str = "canonical-customer-auth";
const DEFAULT_AUDIENCE: &str = "canonical-plus-web";
const MAX_TOKEN_BYTES: usize = 16 * 1024;
const MAX_EMAIL_BYTES: usize = 320;
const MAX_RESPONSE_BYTES: usize = 64 * 1024;

#[derive(Clone)]
pub struct SharedAuthVerifier {
    client: SharedAuthClient,
    audience: String,
    expected_issuer: Option<String>,
    cookie_name: String,
    csrf_key: [u8; 32],
}

impl SharedAuthVerifier {
    pub fn from_env(config: &Config) -> Result<Self, AppError> {
        let default_cookie = if config.cookie_secure {
            DEFAULT_SECURE_COOKIE
        } else {
            DEFAULT_LOOPBACK_COOKIE
        };
        let cookie_name = env::var("SHARED_AUTH_BROWSER_COOKIE_NAME")
            .unwrap_or_else(|_| default_cookie.to_owned());
        validate_cookie_name(&cookie_name, config.cookie_secure)?;

        let explicit_base = env::var("SHARED_AUTH_BASE").ok();
        let legacy_base = env::var("SHARED_AUTH_VERIFY_URL")
            .ok()
            .map(|value| legacy_verify_url_to_base(&value))
            .transpose()?;
        if explicit_base.is_some() && legacy_base.is_some() {
            return Err(AppError::BadRequest(
                "configure SHARED_AUTH_BASE, not both Shared Auth URL variables".into(),
            ));
        }
        let configured_base = explicit_base.or(legacy_base);
        let service_credential = env::var("SHARED_AUTH_INTROSPECT_SECRET").ok();
        if configured_base.is_some() && service_credential.is_none() {
            return Err(AppError::BadRequest(
                "SHARED_AUTH_BASE requires SHARED_AUTH_INTROSPECT_SECRET".into(),
            ));
        }
        if let Some(credential) = service_credential.as_deref() {
            validate_service_credential(credential)?;
        }

        let base = configured_base.unwrap_or_else(|| {
            format!("{}/shared-auth", config.app_base_url.trim_end_matches('/'))
        });
        let audience =
            env::var("SHARED_AUTH_AUDIENCE").unwrap_or_else(|_| DEFAULT_AUDIENCE.to_owned());
        validate_identifier(&audience, "SHARED_AUTH_AUDIENCE")?;
        let expected_issuer = env::var("SHARED_AUTH_ISSUER")
            .ok()
            .filter(|value| !value.is_empty());
        if let Some(issuer) = expected_issuer.as_deref() {
            validate_claim(issuer, "SHARED_AUTH_ISSUER", 256)?;
        }

        let csrf_key: [u8; 32] = config
            .session_encryption_key
            .as_slice()
            .try_into()
            .map_err(|_| AppError::Crypto)?;
        Self::try_new(
            base,
            service_credential,
            audience,
            expected_issuer,
            cookie_name,
            csrf_key,
        )
        .map_err(|_| {
            AppError::BadRequest(
                "Shared Auth configuration must use a valid HTTPS or internal service base URL"
                    .into(),
            )
        })
    }

    fn try_new(
        base: impl Into<String>,
        service_credential: Option<String>,
        audience: impl Into<String>,
        expected_issuer: Option<String>,
        cookie_name: impl Into<String>,
        csrf_key: [u8; 32],
    ) -> Result<Self, ClientError> {
        let mut client =
            SharedAuthClient::try_new(base.into())?.with_max_response_bytes(MAX_RESPONSE_BYTES);
        if let Some(credential) = service_credential {
            client = client.with_service_credential(credential);
        }
        Ok(Self {
            client,
            audience: audience.into(),
            expected_issuer,
            cookie_name: cookie_name.into(),
            csrf_key,
        })
    }

    pub fn cookie_name(&self) -> &str {
        &self.cookie_name
    }

    pub async fn authenticate_session(
        &self,
        token: &str,
        required_scopes: &[&str],
    ) -> Result<AuthContext, AppError> {
        self.authenticate_as(token, required_scopes, CredentialSource::SessionCookie)
            .await
    }

    pub async fn authenticate_bearer(
        &self,
        token: &str,
        required_scopes: &[&str],
    ) -> Result<AuthContext, AppError> {
        self.authenticate_as(token, required_scopes, CredentialSource::Bearer)
            .await
    }

    async fn raw_introspect(
        &self,
        token: &str,
        required_scopes: &[&str],
    ) -> Result<Introspection, ClientError> {
        self.client
            .introspect_with_requirements(token, &self.audience, required_scopes)
            .await
    }

    async fn authenticate_as(
        &self,
        token: &str,
        required_scopes: &[&str],
        source: CredentialSource,
    ) -> Result<AuthContext, AppError> {
        if token.is_empty() || token.len() > MAX_TOKEN_BYTES {
            return Err(AppError::Unauthorized);
        }
        let identity = self
            .raw_introspect(token, required_scopes)
            .await
            .map_err(map_client_error)?;
        if !identity.active
            || identity.aud.as_deref() != Some(self.audience.as_str())
            || required_scopes
                .iter()
                .any(|required| !identity.has_scope(required))
        {
            return Err(AppError::Unauthorized);
        }

        let issuer = required_claim(identity.iss.as_deref(), 256)?;
        if self
            .expected_issuer
            .as_deref()
            .is_some_and(|expected| issuer != expected)
        {
            return Err(AppError::Unauthorized);
        }
        let user_id = required_claim(identity.sub.as_deref(), 128)?
            .parse::<Uuid>()
            .map_err(|_| AppError::Unauthorized)?;
        let email = required_claim(identity.email.as_deref(), MAX_EMAIL_BYTES)?.to_owned();
        if identity.email_verified != Some(true) {
            return Err(AppError::Unauthorized);
        }
        let _provider = required_claim(identity.provider.as_deref(), 128)?;
        let _provider_tenant = required_claim(identity.provider_tenant.as_deref(), 256)?;
        let expires_at = identity
            .exp
            .and_then(|value| i64::try_from(value).ok())
            .and_then(|value| Utc.timestamp_opt(value, 0).single())
            .filter(|value| *value > Utc::now())
            .ok_or(AppError::Unauthorized)?;

        Ok(AuthContext {
            user_id,
            email,
            source,
            supabase_session_id: None,
            session_hash: Some(token_fingerprint(token)),
            csrf_token: self.csrf_token_for_source(token, source),
            expires_at,
        })
    }

    fn csrf_token_for_source(&self, token: &str, source: CredentialSource) -> Option<String> {
        match source {
            CredentialSource::SessionCookie => Some(self.csrf_token(token)),
            CredentialSource::Bearer => None,
        }
    }

    fn csrf_token(&self, token: &str) -> String {
        let mut digest = Sha256::new();
        digest.update(b"canonical-plus/shared-auth-csrf/v1\0");
        digest.update(self.csrf_key);
        digest.update((token.len() as u64).to_be_bytes());
        digest.update(token.as_bytes());
        URL_SAFE_NO_PAD.encode(digest.finalize())
    }
}

fn map_client_error(error: ClientError) -> AppError {
    match error {
        ClientError::Unauthorized | ClientError::InvalidInput(_) => AppError::Unauthorized,
        ClientError::Status(429) => AppError::AuthBusy,
        _ => AppError::AuthUpstream,
    }
}

fn required_claim(value: Option<&str>, maximum: usize) -> Result<&str, AppError> {
    let value = value.ok_or(AppError::Unauthorized)?;
    if value.is_empty()
        || value.len() > maximum
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(AppError::Unauthorized);
    }
    Ok(value)
}

fn validate_service_credential(value: &str) -> Result<(), AppError> {
    if value.trim() != value
        || value.chars().any(char::is_control)
        || value
            .bytes()
            .filter(|byte| !byte.is_ascii_whitespace())
            .count()
            < 32
        || value.len() > MAX_TOKEN_BYTES
    {
        return Err(AppError::BadRequest(
            "SHARED_AUTH_INTROSPECT_SECRET must contain at least 32 non-whitespace bytes".into(),
        ));
    }
    Ok(())
}

fn validate_identifier(value: &str, name: &'static str) -> Result<(), AppError> {
    if value.is_empty()
        || value.len() > 128
        || !value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'/' | b'-')
        })
    {
        return Err(AppError::BadRequest(format!(
            "{name} must be a bounded portable identifier"
        )));
    }
    Ok(())
}

fn validate_claim(value: &str, name: &'static str, maximum: usize) -> Result<(), AppError> {
    if value.is_empty()
        || value.len() > maximum
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(AppError::BadRequest(format!("{name} is invalid")));
    }
    Ok(())
}

fn legacy_verify_url_to_base(value: &str) -> Result<String, AppError> {
    let mut url = Url::parse(value).map_err(|_| {
        AppError::BadRequest("SHARED_AUTH_VERIFY_URL must be an absolute URL".into())
    })?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(AppError::BadRequest(
            "SHARED_AUTH_VERIFY_URL must not contain credentials, query, or fragment".into(),
        ));
    }
    let path = url.path().trim_end_matches('/');
    let base_path = path
        .strip_suffix("/auth/verify")
        .ok_or_else(|| {
            AppError::BadRequest("SHARED_AUTH_VERIFY_URL must end in /auth/verify".into())
        })?
        .to_owned();
    url.set_path(if base_path.is_empty() {
        "/"
    } else {
        &base_path
    });
    Ok(url.as_str().trim_end_matches('/').to_owned())
}

fn validate_cookie_name(value: &str, secure: bool) -> Result<(), AppError> {
    let valid = !value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'!' | b'#'..=b'+' | b'-' | b'.' | b'^'..=b'`' | b'|' | b'~')
        });
    if !valid || (secure && !value.starts_with("__Host-")) {
        return Err(AppError::BadRequest(
            "SHARED_AUTH_BROWSER_COOKIE_NAME must be a valid host-only cookie name".into(),
        ));
    }
    Ok(())
}

fn token_fingerprint(token: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(b"canonical-plus/shared-auth-token/v1\0");
    digest.update(token.as_bytes());
    URL_SAFE_NO_PAD.encode(digest.finalize())
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::mpsc,
        thread,
        time::Duration,
    };

    use serde_json::{json, Value};

    use super::*;

    const SERVICE_CREDENTIAL: &str = "independent-web-service-credential-0001";
    const USER_ID: &str = "11111111-1111-4111-8111-111111111111";

    fn verifier(base: String, credential: Option<&str>) -> SharedAuthVerifier {
        SharedAuthVerifier::try_new(
            base,
            credential.map(str::to_owned),
            DEFAULT_AUDIENCE,
            Some("https://auth.canonical.plus".to_owned()),
            DEFAULT_SECURE_COOKIE,
            [7; 32],
        )
        .unwrap()
    }

    fn response(audience: &str, scope: &str) -> String {
        json!({
            "active": true,
            "sub": USER_ID,
            "iss": "https://auth.canonical.plus",
            "aud": audience,
            "exp": 4_102_444_800_u64,
            "provider": "supabase",
            "provider_tenant": "canonical-plus",
            "email": "customer@example.com",
            "email_verified": true,
            "scope": scope,
            "futureEnvelopeField": {"safe": true}
        })
        .to_string()
    }

    fn read_request(stream: &mut std::net::TcpStream) -> String {
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let count = stream.read(&mut buffer).unwrap();
            if count == 0 {
                break;
            }
            bytes.extend_from_slice(&buffer[..count]);
            let Some(header_end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") else {
                continue;
            };
            let headers = String::from_utf8_lossy(&bytes[..header_end]);
            let content_length = headers.lines().find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            });
            if content_length.is_none_or(|length| bytes.len() >= header_end + 4 + length) {
                break;
            }
        }
        String::from_utf8(bytes).unwrap()
    }

    fn spawn_provider(body: String) -> (String, mpsc::Receiver<String>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (sender, receiver) = mpsc::channel();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            sender.send(read_request(&mut stream)).unwrap();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });
        (format!("http://{address}"), receiver, handle)
    }

    #[tokio::test]
    async fn sends_strict_scoped_envelope_with_independent_service_auth() {
        let (base, requests, handle) =
            spawn_provider(response(DEFAULT_AUDIENCE, "quotes:read quotes:write"));
        let verifier = verifier(base, Some(SERVICE_CREDENTIAL));

        let identity = verifier
            .authenticate_bearer("signed-user-token", &["quotes:read"])
            .await
            .unwrap();

        assert_eq!(identity.user_id.to_string(), USER_ID);
        let request = requests.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(request.starts_with("POST /auth/introspect HTTP/1.1"));
        assert!(request.lines().any(|line| line
            .eq_ignore_ascii_case(&format!("authorization: Bearer {SERVICE_CREDENTIAL}"))));
        let (_, body) = request.split_once("\r\n\r\n").unwrap();
        let body: Value = serde_json::from_str(body).unwrap();
        assert_eq!(
            body,
            json!({
                "contract": "IntrospectionRequest",
                "payload": {
                    "token": "signed-user-token",
                    "audience": DEFAULT_AUDIENCE,
                    "requiredScopes": ["quotes:read"]
                }
            })
        );
        handle.join().unwrap();
    }

    #[tokio::test]
    async fn omission_is_explicit_and_unknown_response_fields_are_accepted() {
        let (base, requests, handle) = spawn_provider(response(DEFAULT_AUDIENCE, ""));
        let verifier = verifier(base, Some(SERVICE_CREDENTIAL));

        verifier
            .authenticate_bearer("signed-user-token", &[])
            .await
            .unwrap();

        let request = requests.recv_timeout(Duration::from_secs(2)).unwrap();
        let (_, body) = request.split_once("\r\n\r\n").unwrap();
        let body: Value = serde_json::from_str(body).unwrap();
        assert_eq!(body["payload"]["requiredScopes"], json!([]));
        handle.join().unwrap();
    }

    #[tokio::test]
    async fn audience_and_scope_mismatches_fail_closed() {
        let (base, _requests, handle) = spawn_provider(response("another-realm", "quotes:read"));
        assert!(matches!(
            verifier(base, Some(SERVICE_CREDENTIAL))
                .authenticate_bearer("signed-user-token", &["quotes:read"])
                .await,
            Err(AppError::Unauthorized)
        ));
        handle.join().unwrap();

        let (base, _requests, handle) = spawn_provider(response(DEFAULT_AUDIENCE, "quotes:read"));
        assert!(matches!(
            verifier(base, Some(SERVICE_CREDENTIAL))
                .authenticate_bearer("signed-user-token", &["quotes:write"])
                .await,
            Err(AppError::Unauthorized)
        ));
        handle.join().unwrap();
    }

    #[tokio::test]
    async fn service_auth_precedes_token_constraints_and_duplicates_are_rejected() {
        let without_service = verifier("http://127.0.0.1:9".to_owned(), None);
        let error = without_service
            .raw_introspect("invalid token with spaces", &["duplicate", "duplicate"])
            .await
            .unwrap_err();
        assert!(matches!(error, ClientError::MissingServiceCredential));

        let with_service = verifier("http://127.0.0.1:9".to_owned(), Some(SERVICE_CREDENTIAL));
        let error = with_service
            .raw_introspect("signed-user-token", &["quotes:read", "quotes:read"])
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            ClientError::InvalidInput("required scopes")
        ));
    }

    #[test]
    fn csrf_and_cookie_guards_remain_bound_to_browser_credentials() {
        let verifier = verifier("https://auth.canonical.plus".to_owned(), None);
        assert_eq!(
            verifier.csrf_token("token-a"),
            verifier.csrf_token("token-a")
        );
        assert_ne!(
            verifier.csrf_token("token-a"),
            verifier.csrf_token("token-b")
        );
        assert!(verifier
            .csrf_token_for_source("token-a", CredentialSource::SessionCookie)
            .is_some());
        assert!(verifier
            .csrf_token_for_source("token-a", CredentialSource::Bearer)
            .is_none());
        assert!(validate_cookie_name(DEFAULT_SECURE_COOKIE, true).is_ok());
        assert!(validate_cookie_name("canonical-auth", true).is_err());
    }

    #[test]
    fn legacy_verify_url_is_only_a_bounded_migration_path() {
        assert_eq!(
            legacy_verify_url_to_base("https://app.canonical.plus/shared-auth/auth/verify")
                .unwrap(),
            "https://app.canonical.plus/shared-auth"
        );
        assert!(
            legacy_verify_url_to_base("https://app.canonical.plus/auth/verify?token=x").is_err()
        );
        assert!(legacy_verify_url_to_base("https://app.canonical.plus/auth/exchange").is_err());
    }
}
