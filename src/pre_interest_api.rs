//! Bounded browser-BFF client for the write-only pre-interest registration API.
//!
//! This module deliberately owns no database connection and never logs request
//! payloads. The browser route derives host and consent metadata, while the
//! dedicated API remains responsible for normalization, idempotency binding,
//! rate limiting, encryption, and persistence.

use std::{env, sync::Arc, time::Duration};

use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use chrono::DateTime;
use futures_util::StreamExt;
use reqwest::{Client, Response, Url};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::AppError;

const MAX_RESPONSE_BYTES: usize = 64 * 1024;
const CREATE_PATH: &str = "/v1/pre-interest-registrations";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PartyType {
    Individual,
    Organization,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum InterestArea {
    #[serde(rename = "readiness_assessment")]
    ReadinessAssessment,
    #[serde(rename = "soc2")]
    Soc2,
    #[serde(rename = "iso_27001")]
    Iso27001,
    #[serde(rename = "hipaa")]
    Hipaa,
    #[serde(rename = "pci_dss_4")]
    PciDss4,
    #[serde(rename = "fedramp")]
    Fedramp,
    #[serde(rename = "nist")]
    Nist,
    #[serde(rename = "gdpr")]
    Gdpr,
    #[serde(rename = "cmmc")]
    Cmmc,
}

impl InterestArea {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "readiness_assessment" => Some(Self::ReadinessAssessment),
            "soc2" => Some(Self::Soc2),
            "iso_27001" => Some(Self::Iso27001),
            "hipaa" => Some(Self::Hipaa),
            "pci_dss_4" => Some(Self::PciDss4),
            "fedramp" => Some(Self::Fedramp),
            "nist" => Some(Self::Nist),
            "gdpr" => Some(Self::Gdpr),
            "cmmc" => Some(Self::Cmmc),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegistrationHost {
    User,
    Organization,
}

impl RegistrationHost {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "user.canonical.plus" => Some(Self::User),
            "org.canonical.plus" => Some(Self::Organization),
            _ => None,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user.canonical.plus",
            Self::Organization => "org.canonical.plus",
        }
    }

    pub const fn party_type(self) -> PartyType {
        match self {
            Self::User => PartyType::Individual,
            Self::Organization => PartyType::Organization,
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PreInterestRegistrationRequest {
    pub request_id: Uuid,
    pub email: String,
    pub party_type: PartyType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub organization_name: Option<String>,
    pub interest_areas: Vec<InterestArea>,
    pub consent_revision: String,
    pub consented_at: String,
    pub source_host: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub referral_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub website_url: Option<String>,
    pub registration_consent: bool,
    pub marketing_consent: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub marketing_consent_revision: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PreInterestRegistrationReceipt {
    receipt_id: Uuid,
    status: String,
    accepted_at: String,
    next_step_url: String,
}

impl PreInterestRegistrationReceipt {
    fn validate(self) -> Result<Self, AppError> {
        if self.status != "accepted"
            || DateTime::parse_from_rfc3339(&self.accepted_at).is_err()
            || !is_reviewed_next_step(&self.next_step_url)
        {
            return Err(AppError::ServiceUpstream);
        }
        let _ = self.receipt_id;
        Ok(self)
    }
}

#[derive(Clone)]
pub struct PreInterestApiClient {
    base_url: String,
    http: Client,
    internal_auth_token: Arc<str>,
}

impl PreInterestApiClient {
    pub fn from_env() -> Result<Self, AppError> {
        let raw_url = env::var("CANONICAL_API_URL")
            .map_err(|_| AppError::BadRequest("CANONICAL_API_URL is required".into()))?;
        let parsed = Url::parse(&raw_url)
            .map_err(|_| AppError::BadRequest("CANONICAL_API_URL must be absolute".into()))?;
        let internal_origin = matches!(parsed.host_str(), Some("localhost" | "127.0.0.1" | "::1"))
            || parsed.host_str().is_some_and(|host| host.ends_with(".svc"))
            || parsed.host_str().is_some_and(|host| host.contains(".svc."));
        if parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
            || parsed.path() != "/"
            || (parsed.scheme() != "https" && !(parsed.scheme() == "http" && internal_origin))
        {
            return Err(AppError::BadRequest(
                "CANONICAL_API_URL must be an HTTPS origin, except for loopback or Kubernetes service DNS"
                    .into(),
            ));
        }

        let internal_auth_token = env::var("CANONICAL_INTERNAL_AUTH_TOKEN").map_err(|_| {
            AppError::BadRequest("CANONICAL_INTERNAL_AUTH_TOKEN is required".into())
        })?;
        if internal_auth_token.trim() != internal_auth_token || internal_auth_token.len() < 32 {
            return Err(AppError::BadRequest(
                "CANONICAL_INTERNAL_AUTH_TOKEN must contain at least 32 bytes".into(),
            ));
        }

        let http = Client::builder()
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(12))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("canonical-web-server/0.1 public-intake")
            .build()?;

        Ok(Self {
            base_url: parsed.origin().ascii_serialization(),
            http,
            internal_auth_token: Arc::from(internal_auth_token),
        })
    }

    pub async fn create(
        &self,
        request: &PreInterestRegistrationRequest,
    ) -> Result<PreInterestRegistrationReceipt, AppError> {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-canonical-internal-token",
            HeaderValue::from_str(&self.internal_auth_token).map_err(|_| AppError::Crypto)?,
        );
        headers.insert(
            "idempotency-key",
            HeaderValue::from_str(&request.request_id.to_string()).map_err(|_| AppError::Crypto)?,
        );

        let response = self
            .http
            .post(format!("{}{}", self.base_url, CREATE_PATH))
            .headers(headers)
            .json(request)
            .send()
            .await?;
        decode(response).await?.validate()
    }
}

async fn decode(response: Response) -> Result<PreInterestRegistrationReceipt, AppError> {
    let status = response.status();
    if status == StatusCode::TOO_MANY_REQUESTS {
        let retry_after_seconds = response
            .headers()
            .get(header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(60)
            .clamp(1, 3_600);
        return Err(AppError::RateLimited {
            retry_after_seconds,
        });
    }
    if matches!(
        status,
        StatusCode::BAD_REQUEST | StatusCode::CONFLICT | StatusCode::UNPROCESSABLE_ENTITY
    ) {
        return Err(AppError::BadRequest(
            "review the registration fields and try again".into(),
        ));
    }
    if status != StatusCode::ACCEPTED {
        tracing::warn!(%status, "dedicated pre-interest API rejected the request");
        return Err(AppError::ServiceUpstream);
    }

    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
            return Err(AppError::ServiceUpstream);
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(AppError::from)
}

fn is_reviewed_next_step(value: &str) -> bool {
    matches!(
        value,
        "https://user.canonical.plus/u/quote"
            | "https://user.canonical.plus/pre-interest"
            | "https://org.canonical.plus/submit-application"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_path_matches_the_authoritative_interface_contract() {
        assert_eq!(CREATE_PATH, "/v1/pre-interest-registrations");
    }

    #[test]
    fn next_steps_are_closed_and_same_site() {
        assert!(is_reviewed_next_step("https://user.canonical.plus/u/quote"));
        assert!(is_reviewed_next_step(
            "https://org.canonical.plus/submit-application"
        ));
        assert!(!is_reviewed_next_step("https://example.com/collect"));
        assert!(!is_reviewed_next_step(
            "https://user.canonical.plus.evil.example/u/quote"
        ));
    }

    #[test]
    fn registration_hosts_are_closed() {
        assert_eq!(
            RegistrationHost::parse("user.canonical.plus").map(RegistrationHost::as_str),
            Some("user.canonical.plus")
        );
        assert_eq!(
            RegistrationHost::parse("org.canonical.plus").map(RegistrationHost::party_type),
            Some(PartyType::Organization)
        );
        assert!(RegistrationHost::parse("api.canonical.plus").is_none());
    }
}
