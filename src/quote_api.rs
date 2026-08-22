//! Browser-facing quote client and Maud views for the dedicated Canonical API.

use std::{env, sync::Arc, time::Duration};

use axum::http::{HeaderMap, HeaderValue, StatusCode};
use futures_util::StreamExt;
use maud::{html, Markup, DOCTYPE};
use reqwest::{Client, Response, Url};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value as JsonValue;
use uuid::Uuid;

use crate::{auth::AuthContext, error::AppError};

const MAX_RESPONSE_BYTES: usize = 512 * 1024;
const CANONICAL_CONTEXT_KEY: &str = "quote-analysis";
const ANSWERS_VERSION: u8 = 1;

#[derive(Clone)]
pub struct QuoteApiClient {
    base_url: String,
    http: Client,
    internal_auth_token: Arc<str>,
}

impl QuoteApiClient {
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
            .timeout(Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("canonical-web-server/0.1")
            .build()?;

        Ok(Self {
            base_url: parsed.origin().ascii_serialization(),
            http,
            internal_auth_token: Arc::from(internal_auth_token),
        })
    }

    pub async fn create(
        &self,
        actor: &AuthContext,
        request: &QuoteRequest,
        idempotency_key: Uuid,
    ) -> Result<QuoteResponse, AppError> {
        let mut headers = self.headers(actor)?;
        headers.insert(
            "idempotency-key",
            HeaderValue::from_str(&idempotency_key.to_string()).map_err(|_| AppError::Crypto)?,
        );
        let response = self
            .http
            .post(format!("{}/api/v1/quotes", self.base_url))
            .headers(headers)
            .json(request)
            .send()
            .await?;
        let submission: ApiQuoteSubmissionResponse = decode(response, StatusCode::ACCEPTED).await?;
        let expected_stream = format!("/api/v1/quotes/{}/events", submission.quote_id);
        if submission.status != "queued"
            || submission.stream_url != expected_stream
            || submission.created_at.is_empty()
        {
            return Err(AppError::ServiceUpstream);
        }
        Ok(QuoteResponse::from_submission(request, submission))
    }

    pub async fn get(
        &self,
        actor: &AuthContext,
        quote_id: Uuid,
    ) -> Result<QuoteResponse, AppError> {
        let response = self
            .http
            .get(format!("{}/api/v1/quotes/{quote_id}", self.base_url))
            .headers(self.headers(actor)?)
            .send()
            .await?;
        let record: ApiQuoteRecord = decode(response, StatusCode::OK).await?;
        Ok(record.into())
    }

    pub async fn list(&self, actor: &AuthContext) -> Result<Vec<QuoteResponse>, AppError> {
        let response = self
            .http
            .get(format!("{}/api/v1/quotes", self.base_url))
            .headers(self.headers(actor)?)
            .send()
            .await?;
        let records: Vec<ApiQuoteRecord> = decode(response, StatusCode::OK).await?;
        Ok(records.into_iter().map(QuoteResponse::from).collect())
    }

    fn headers(&self, actor: &AuthContext) -> Result<HeaderMap, AppError> {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-canonical-internal-token",
            HeaderValue::from_str(&self.internal_auth_token).map_err(|_| AppError::Crypto)?,
        );
        headers.insert(
            "x-canonical-subject",
            HeaderValue::from_str(&actor.user_id.to_string()).map_err(|_| AppError::Crypto)?,
        );
        Ok(headers)
    }
}

async fn decode<T: DeserializeOwned>(
    response: Response,
    expected: StatusCode,
) -> Result<T, AppError> {
    let status = response.status();
    if status == StatusCode::NOT_FOUND {
        return Err(AppError::NotFound);
    }
    if matches!(
        status,
        StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY
    ) {
        return Err(AppError::BadRequest(
            "review the quote fields and try again".into(),
        ));
    }
    if status != expected || !status.is_success() {
        tracing::warn!(%status, "dedicated quote API rejected the request");
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

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuoteRequest {
    pub organization_name: String,
    pub contact_name: String,
    pub contact_email: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub website: Option<String>,
    pub employee_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annual_revenue_band: Option<String>,
    pub frameworks: Vec<String>,
    pub current_stage: String,
    pub infrastructure: Vec<String>,
    pub data_sensitivity: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_date: Option<String>,
    pub has_security_program: bool,
    pub has_policies: bool,
    pub has_risk_assessment: bool,
    pub has_incident_response_plan: bool,
    pub has_vendor_management: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    pub context_key: String,
    pub answers_version: u8,
}

impl QuoteRequest {
    pub fn fixed_context_key() -> &'static str {
        CANONICAL_CONTEXT_KEY
    }

    pub const fn answers_version() -> u8 {
        ANSWERS_VERSION
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ApiQuoteSubmissionResponse {
    quote_id: Uuid,
    status: String,
    stream_url: String,
    created_at: String,
}

#[derive(Debug, Deserialize)]
struct ApiQuoteRecord {
    analysis: Option<JsonValue>,
    error_code: Option<String>,
    frameworks: Vec<String>,
    organization_name: String,
    quote_id: Uuid,
    status: String,
}

#[derive(Clone, Debug)]
pub struct QuoteResponse {
    pub id: Uuid,
    pub status: String,
    pub company_name: String,
    pub frameworks: Vec<String>,
    pub estimate: Option<QuoteEstimate>,
    pub analysis_summary: Option<String>,
    pub error_code: Option<String>,
}

impl QuoteResponse {
    fn from_submission(request: &QuoteRequest, submission: ApiQuoteSubmissionResponse) -> Self {
        Self {
            id: submission.quote_id,
            status: submission.status,
            company_name: request.organization_name.clone(),
            frameworks: request.frameworks.clone(),
            estimate: None,
            analysis_summary: None,
            error_code: None,
        }
    }
}

impl From<ApiQuoteRecord> for QuoteResponse {
    fn from(record: ApiQuoteRecord) -> Self {
        let estimate = record.analysis.as_ref().and_then(|analysis| {
            Some(QuoteEstimate {
                low: analysis.get("estimated_total_fee_low")?.as_u64()?,
                high: analysis.get("estimated_total_fee_high")?.as_u64()?,
                currency: analysis.get("currency")?.as_str()?.to_owned(),
            })
        });
        let analysis_summary = record
            .analysis
            .as_ref()
            .and_then(|analysis| analysis.get("summary"))
            .and_then(JsonValue::as_str)
            .map(str::to_owned);
        Self {
            id: record.quote_id,
            status: record.status,
            company_name: record.organization_name,
            frameworks: record.frameworks,
            estimate,
            analysis_summary,
            error_code: record.error_code,
        }
    }
}

#[derive(Clone, Debug)]
pub struct QuoteEstimate {
    pub low: u64,
    pub high: u64,
    pub currency: String,
}

pub fn quote_page(actor: &AuthContext, quotes: &[QuoteResponse]) -> Markup {
    let csrf = actor.csrf_token.as_deref().unwrap_or_default();
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                meta name="canonical-quote-account" content=(actor.user_id);
                title { "Readiness assessment · canonical.plus" }
                style {
                    "body{font-family:ui-sans-serif,system-ui,sans-serif;max-width:64rem;margin:0 auto;padding:2rem;line-height:1.5}.card{border:1px solid #8886;border-radius:.75rem;padding:1.25rem;margin:1rem 0}label{display:block;margin:.75rem 0}input,textarea,select,button{font:inherit;padding:.65rem}input,textarea,select{box-sizing:border-box;width:100%}.grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(13rem,1fr));gap:.6rem}.grid label{display:flex;gap:.5rem;align-items:center;margin:0}.grid input{width:auto}.muted{opacity:.72}.error{color:#b42318}.quote-total{font-size:1.5rem;font-weight:700}[data-opto-state=\"pending\"]{border-color:#b7791f}[data-opto-state=\"failed\"]{border-color:#b42318}button[disabled]{opacity:.6;cursor:wait}"
                }
                script type="module" src="/app-assets/app.js" {}
            }
            body {
                nav { a href="/" { "← canonical.plus" } }
                main {
                    h1 { "Scope your readiness plan" }
                    p class="muted" {
                        "Signed in as " (actor.email) ". Your answers are private to your account."
                    }
                    p class="muted" {
                        "Do not submit credentials, protected health information, cardholder data, or production evidence."
                    }
                    form class="card" method="post" action="/u/readiness" data-opto-quote="true"
                        hx-post="/u/readiness" hx-target="#quote-results" hx-swap="afterbegin" {
                        input type="hidden" name="csrf" value=(csrf);
                        input type="hidden" name="client_request_id" value=(Uuid::new_v4());

                        h2 { "Organization and contact" }
                        label { "Organization name" input name="organization_name" required maxlength="200"; }
                        label { "Contact name" input name="contact_name" required maxlength="160"; }
                        label { "Verified contact email"
                            input value=(actor.email) readonly aria-readonly="true";
                        }
                        label { "Public website (optional)"
                            input type="url" name="website" maxlength="2048" placeholder="https://example.com";
                        }
                        label { "Number of employees and long-term contractors"
                            input type="number" name="employee_count" min="1" max="1000000" required;
                        }
                        label { "Annual revenue band (optional)"
                            select name="annual_revenue_band" {
                                option value="" { "Prefer not to say" }
                                option value="pre_revenue" { "Pre-revenue" }
                                option value="under_1m" { "Under $1M" }
                                option value="1m_10m" { "$1M–$10M" }
                                option value="10m_50m" { "$10M–$50M" }
                                option value="50m_250m" { "$50M–$250M" }
                                option value="over_250m" { "Over $250M" }
                                option value="prefer_not_to_say" { "Prefer not to say (recorded)" }
                            }
                        }

                        h2 { "Frameworks" }
                        div class="grid" {
                            label { input type="checkbox" name="soc2_type_1"; "SOC 2 Type I" }
                            label { input type="checkbox" name="soc2_type_2"; "SOC 2 Type II" }
                            label { input type="checkbox" name="nist_csf_2"; "NIST CSF 2.0" }
                            label { input type="checkbox" name="nist_800_53"; "NIST SP 800-53" }
                            label { input type="checkbox" name="hipaa"; "HIPAA" }
                            label { input type="checkbox" name="iso_27001"; "ISO 27001" }
                            label { input type="checkbox" name="pci_dss_4"; "PCI DSS 4" }
                            label { input type="checkbox" name="fedramp"; "FedRAMP" }
                            label { input type="checkbox" name="gdpr"; "GDPR" }
                            label { input type="checkbox" name="custom"; "Custom scope" }
                        }

                        h2 { "Program stage" }
                        label { "Current stage"
                            select name="current_stage" required {
                                option value="" { "Choose a stage" }
                                option value="exploring" { "Exploring" }
                                option value="readiness" { "Readiness" }
                                option value="remediation" { "Remediation" }
                                option value="audit_ready" { "Preparing for independent review" }
                                option value="renewal" { "Renewal" }
                            }
                        }
                        label { "Target date (optional)"
                            input type="date" name="target_date";
                        }

                        h2 { "Infrastructure" }
                        div class="grid" {
                            label { input type="checkbox" name="infra_aws"; "AWS" }
                            label { input type="checkbox" name="infra_azure"; "Azure" }
                            label { input type="checkbox" name="infra_gcp"; "GCP" }
                            label { input type="checkbox" name="infra_supabase"; "Supabase" }
                            label { input type="checkbox" name="infra_on_prem"; "On-premises" }
                            label { input type="checkbox" name="infra_colocation"; "Colocation" }
                            label { input type="checkbox" name="infra_saas_only"; "SaaS-only" }
                            label { input type="checkbox" name="infra_multi_cloud"; "Multi-cloud" }
                            label { input type="checkbox" name="infra_other"; "Other" }
                        }

                        h2 { "Data sensitivity" }
                        div class="grid" {
                            label { input type="checkbox" name="data_public"; "Public" }
                            label { input type="checkbox" name="data_internal"; "Internal" }
                            label { input type="checkbox" name="data_confidential"; "Confidential" }
                            label { input type="checkbox" name="data_pii"; "PII" }
                            label { input type="checkbox" name="data_phi"; "PHI" }
                            label { input type="checkbox" name="data_pci"; "Payment-card data" }
                            label { input type="checkbox" name="data_government_cui"; "Government CUI" }
                            label { input type="checkbox" name="data_customer_secrets"; "Customer secrets" }
                            label { input type="checkbox" name="data_other"; "Other" }
                        }

                        h2 { "Current readiness signals" }
                        div class="grid" {
                            label { input type="checkbox" name="has_security_program"; "Named security owner and operating program" }
                            label { input type="checkbox" name="has_policies"; "Reviewed security and privacy policies" }
                            label { input type="checkbox" name="has_risk_assessment"; "Current documented risk assessment" }
                            label { input type="checkbox" name="has_incident_response_plan"; "Exercised incident-response plan" }
                            label { input type="checkbox" name="has_vendor_management"; "Third-party risk and vendor review process" }
                        }
                        label { "Anything else we should know"
                            textarea name="notes" rows="5" maxlength="5000" {}
                        }
                        button type="submit" { "Build my readiness scope" }
                        p id="quote-sync-status" class="muted" aria-live="polite" {
                            "Writes are saved locally before delivery."
                        }
                    }
                    section id="quote-results" aria-live="polite" {
                        @for quote in quotes {
                            (quote_status_fragment(quote))
                        }
                    }
                }
            }
        }
    }
}

pub fn quote_detail_page(actor: &AuthContext, quote: &QuoteResponse) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "Readiness scope · canonical.plus" }
            }
            body {
                main {
                    p { a href="/u/readiness" { "← All readiness scopes" } }
                    p class="muted" { "Signed in as " (actor.email) }
                    (quote_status_fragment(quote))
                }
            }
        }
    }
}

pub fn quote_status_fragment(quote: &QuoteResponse) -> Markup {
    if matches!(quote.status.as_str(), "queued" | "analyzing") {
        return html! {
            article id={ "quote-" (quote.id) } class="card"
                hx-get={ "/u/readiness/" (quote.id) }
                hx-trigger="every 2s"
                hx-swap="outerHTML" {
                h2 { (quote.company_name) }
                p { "Canonical's bounded readiness analysis is running." }
                p class="muted" { "This status refreshes automatically." }
            }
        };
    }
    if matches!(quote.status.as_str(), "completed" | "ready") {
        return html! {
            article id={ "quote-" (quote.id) } class="card" {
                h2 { (quote.company_name) }
                @if let Some(estimate) = quote.estimate.as_ref() {
                    p class="quote-total" {
                        "$" (estimate.low) "–$" (estimate.high) " " (&estimate.currency)
                    }
                }
                p {
                    (quote.analysis_summary.as_deref().unwrap_or("Your preliminary readiness scope is ready."))
                }
                p class="muted" {
                    "This is readiness planning support, not an audit opinion, attestation, certification, or legal conclusion."
                }
            }
        };
    }
    html! {
        article id={ "quote-" (quote.id) } class="card" {
            h2 { (quote.company_name) }
            p class="error" role="alert" {
                "We could not finish this readiness scope. Please review your answers and try again."
            }
            @if let Some(code) = quote.error_code.as_deref() {
                p class="muted" { "Reference: " (code) }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_request() -> QuoteRequest {
        serde_json::from_str(include_str!("../fixtures/quote/v1/request.json")).unwrap()
    }

    #[test]
    fn canonical_fixture_serializes_without_transport_drift() {
        let request = fixture_request();
        let expected: JsonValue =
            serde_json::from_str(include_str!("../fixtures/quote/v1/request.json")).unwrap();
        assert_eq!(serde_json::to_value(&request).unwrap(), expected);
        assert_eq!(request.context_key, CANONICAL_CONTEXT_KEY);
        assert_eq!(request.answers_version, ANSWERS_VERSION);
        assert!(expected.get("contextRecordId").is_none());
        assert!(expected.get("markdown_context").is_none());
        assert!(expected.get("userId").is_none());
    }

    #[test]
    fn maps_the_canonical_submission_fixture() {
        let submission: ApiQuoteSubmissionResponse = serde_json::from_str(include_str!(
            "../fixtures/quote/v1/submission-response.json"
        ))
        .unwrap();
        let request = fixture_request();
        let quote = QuoteResponse::from_submission(&request, submission);
        assert_eq!(
            quote.id,
            Uuid::parse_str("11111111-1111-4111-8111-111111111111").unwrap()
        );
        assert_eq!(quote.status, "queued");
        assert_eq!(quote.company_name, "Example Company");
        assert!(quote.frameworks.contains(&"nist_800_53".to_owned()));
    }

    #[test]
    fn maps_the_durable_api_record() {
        let value = serde_json::json!({
            "analysis": {
                "summary": "A phased readiness engagement.",
                "currency": "USD",
                "estimated_total_fee_low": 12000,
                "estimated_total_fee_high": 18000
            },
            "context_record_id": Uuid::nil(),
            "error_code": null,
            "frameworks": ["soc2_type_2"],
            "gemini_model": "gemini-3.6-flash",
            "organization_name": "Example",
            "persistence": "postgres",
            "quote_id": Uuid::nil(),
            "status": "completed"
        });
        let record: ApiQuoteRecord = serde_json::from_value(value).unwrap();
        let quote = QuoteResponse::from(record);
        assert_eq!(quote.company_name, "Example");
        assert_eq!(quote.estimate.as_ref().unwrap().low, 12_000);
        assert_eq!(
            quote.analysis_summary.as_deref(),
            Some("A phased readiness engagement.")
        );
    }
}
