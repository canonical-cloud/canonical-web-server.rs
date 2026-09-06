use std::{fmt, time::Duration};

use chrono::{DateTime, Utc};
use reqwest::{StatusCode, Url};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum IntegrationConfigError {
    #[error("required integration environment variable {0} is missing")]
    Missing(&'static str),
    #[error("integration environment variable {name} is invalid: {message}")]
    Invalid { name: &'static str, message: String },
    #[error("integration HTTP client configuration failed")]
    Http(#[from] reqwest::Error),
}

#[derive(Clone)]
pub struct SharedAuthClient {
    http: reqwest::Client,
    base_url: String,
    client_id: String,
    client_secret: String,
    expected_supabase_project: String,
    callback_url: String,
}

impl fmt::Debug for SharedAuthClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SharedAuthClient")
            .field("base_url", &self.base_url)
            .field("client_id", &self.client_id)
            .field("client_secret", &"[REDACTED]")
            .field("expected_supabase_project", &self.expected_supabase_project)
            .field("callback_url", &self.callback_url)
            .finish()
    }
}

impl SharedAuthClient {
    pub fn from_env(app_base_url: &str) -> Result<Self, IntegrationConfigError> {
        let base_url =
            validated_origin("SHARED_AUTH_BASE_URL", &required("SHARED_AUTH_BASE_URL")?)?;
        let client_id =
            validated_identifier("SHARED_AUTH_CLIENT_ID", required("SHARED_AUTH_CLIENT_ID")?)?;
        let client_secret = required("SHARED_AUTH_CLIENT_SECRET")?;
        if client_secret.len() < 32 || client_secret.trim() != client_secret {
            return Err(IntegrationConfigError::Invalid {
                name: "SHARED_AUTH_CLIENT_SECRET",
                message: "must contain at least 32 non-whitespace-trimmed bytes".into(),
            });
        }
        let expected_supabase_project = validated_identifier(
            "SHARED_AUTH_SUPABASE_PROJECT",
            required("SHARED_AUTH_SUPABASE_PROJECT")?,
        )?;
        let callback_url = format!(
            "{}/auth/shared/callback",
            app_base_url.trim_end_matches('/')
        );
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(10))
            .user_agent("canonical-web-server/shared-auth-handoff")
            .build()?;
        Ok(Self {
            http,
            base_url,
            client_id,
            client_secret,
            expected_supabase_project,
            callback_url,
        })
    }

    pub fn authorization_url(
        &self,
        return_to: &str,
        state: &str,
        code_challenge: &str,
    ) -> Result<String, SharedAuthRequestError> {
        let mut url = Url::parse(&format!("{}/authorize", self.base_url))
            .map_err(|_| SharedAuthRequestError::InvalidResponse)?;
        url.query_pairs_mut()
            .append_pair("client_id", &self.client_id)
            .append_pair("redirect_uri", &self.callback_url)
            .append_pair("return_to", return_to)
            .append_pair("state", state)
            .append_pair("code_challenge", code_challenge)
            .append_pair("code_challenge_method", "S256");
        Ok(url.to_string())
    }

    pub async fn redeem(
        &self,
        code: &str,
        code_verifier: &str,
    ) -> Result<SharedHandoffTokens, SharedAuthRequestError> {
        let response = self
            .http
            .post(format!("{}/auth/handoff/redeem", self.base_url))
            .bearer_auth(&self.client_secret)
            .json(&SharedHandoffRedeemRequest {
                client_id: &self.client_id,
                code,
                redirect_uri: &self.callback_url,
                code_verifier,
            })
            .send()
            .await
            .map_err(|_| SharedAuthRequestError::Unavailable)?;
        if matches!(
            response.status(),
            StatusCode::BAD_REQUEST | StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
        ) {
            return Err(SharedAuthRequestError::Unauthorized);
        }
        if !response.status().is_success() {
            return Err(SharedAuthRequestError::Unavailable);
        }
        let tokens: SharedHandoffTokens = response
            .json()
            .await
            .map_err(|_| SharedAuthRequestError::InvalidResponse)?;
        if tokens.access_token.is_empty()
            || tokens.access_token.len() > 16_384
            || tokens.refresh_token.is_empty()
            || tokens.refresh_token.len() > 16_384
            || tokens.user.id.is_nil()
            || tokens.expires_at <= Utc::now()
            || tokens.supabase_project != self.expected_supabase_project
        {
            return Err(SharedAuthRequestError::InvalidResponse);
        }
        Ok(tokens)
    }
}

#[derive(Debug, Error)]
pub enum SharedAuthRequestError {
    #[error("authorization code was rejected")]
    Unauthorized,
    #[error("shared-auth is unavailable")]
    Unavailable,
    #[error("shared-auth returned an invalid response")]
    InvalidResponse,
}

#[derive(Serialize)]
struct SharedHandoffRedeemRequest<'a> {
    client_id: &'a str,
    code: &'a str,
    redirect_uri: &'a str,
    code_verifier: &'a str,
}

#[derive(Deserialize)]
pub struct SharedHandoffTokens {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: DateTime<Utc>,
    pub user: SharedHandoffUser,
    pub return_to: String,
    pub supabase_project: String,
}

#[derive(Deserialize)]
pub struct SharedHandoffUser {
    pub id: Uuid,
    pub email: Option<String>,
}

fn required(name: &'static str) -> Result<String, IntegrationConfigError> {
    crate::config::flags::var(name).map_err(|_| IntegrationConfigError::Missing(name))
}

fn validated_identifier(
    name: &'static str,
    value: String,
) -> Result<String, IntegrationConfigError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(IntegrationConfigError::Invalid {
            name,
            message: "must be a 1-128 character URL-safe identifier".into(),
        });
    }
    Ok(value)
}

fn validated_origin(name: &'static str, value: &str) -> Result<String, IntegrationConfigError> {
    let parsed = Url::parse(value).map_err(|error| IntegrationConfigError::Invalid {
        name,
        message: format!("expected an absolute HTTP(S) origin: {error}"),
    })?;
    let host = parsed
        .host_str()
        .ok_or_else(|| IntegrationConfigError::Invalid {
            name,
            message: "a host is required".into(),
        })?;
    let loopback = matches!(host, "localhost" | "127.0.0.1" | "::1");
    if !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || parsed.path() != "/"
        || !matches!(parsed.scheme(), "https" | "http")
        || (parsed.scheme() == "http" && !loopback)
    {
        return Err(IntegrationConfigError::Invalid {
            name,
            message: "must be an HTTPS origin; HTTP is allowed only for loopback or Kubernetes service DNS"
                .into(),
        });
    }
    Ok(parsed.origin().ascii_serialization())
}
