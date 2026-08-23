//! Browser-facing quote client and Maud views for the dedicated Canonical API.

use std::{env, sync::Arc, time::Duration};

use axum::http::{HeaderMap, HeaderValue, StatusCode};
use canonical_interfaces::{
    QuoteDetail, QuoteDetailStatus, QuoteListResponse, QuoteSubmissionResponse,
    QuoteSubmissionResponseStatus, QuoteSummary, QuoteSummaryStatus,
};
use futures_util::StreamExt;
use maud::{html, Markup, DOCTYPE};
use reqwest::{Client, Response, Url};
use serde::de::DeserializeOwned;
use uuid::Uuid;

use crate::{auth::AuthContext, error::AppError};

pub use canonical_interfaces::QuoteRequest;

const MAX_RESPONSE_BYTES: usize = 512 * 1024;

/// P2 boundary to the API-owned quote data plane. Keep the hop authenticated, deadline- and
/// response-bounded, and non-redirecting; mutations reuse a stable idempotency key. Failures never
/// fall back to the web database or silently switch to P1, P3, or P4. See
/// `docs/web-api-data-access.md` for the consistency, retry, tracing, and backpressure contract.
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
        let accepted: QuoteSubmissionResponse = decode(response, StatusCode::ACCEPTED).await?;
        let accepted_id =
            Uuid::parse_str(&accepted.quote_id).map_err(|_| AppError::ServiceUpstream)?;
        let expected_stream = format!("/api/v1/quotes/{accepted_id}/events");
        if accepted_id != idempotency_key
            || accepted.stream_url != expected_stream
            || accepted.created_at.is_empty()
            || !matches!(
                accepted.status,
                QuoteSubmissionResponseStatus::Queued
                    | QuoteSubmissionResponseStatus::Analyzing
                    | QuoteSubmissionResponseStatus::Ready
                    | QuoteSubmissionResponseStatus::Failed
            )
        {
            return Err(AppError::ServiceUpstream);
        }
        self.get(actor, accepted_id).await
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
        let detail: QuoteDetail = decode(response, StatusCode::OK).await?;
        quote_from_detail(detail)
    }

    pub async fn list(&self, actor: &AuthContext) -> Result<Vec<QuoteResponse>, AppError> {
        let response = self
            .http
            .get(format!("{}/api/v1/quotes", self.base_url))
            .headers(self.headers(actor)?)
            .send()
            .await?;
        let page: QuoteListResponse = decode(response, StatusCode::OK).await?;
        page.quotes.into_iter().map(quote_from_summary).collect()
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
        StatusCode::BAD_REQUEST | StatusCode::CONFLICT | StatusCode::UNPROCESSABLE_ENTITY
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

#[derive(Clone, Debug)]
pub struct QuoteEstimate {
    pub low: u64,
    pub high: u64,
    pub currency: String,
}

fn quote_from_detail(detail: QuoteDetail) -> Result<QuoteResponse, AppError> {
    let analysis_summary = detail
        .estimate
        .as_ref()
        .map(|estimate| estimate.summary.clone());
    let error_code = detail.problem.as_ref().map(|problem| problem.code.clone());
    Ok(QuoteResponse {
        id: Uuid::parse_str(&detail.quote_id).map_err(|_| AppError::ServiceUpstream)?,
        status: detail_status(detail.status).into(),
        company_name: detail.request.organization_name,
        frameworks: detail.request.frameworks,
        estimate: detail.estimate.as_ref().map(quote_estimate),
        analysis_summary,
        error_code,
    })
}

fn quote_from_summary(summary: QuoteSummary) -> Result<QuoteResponse, AppError> {
    let analysis_summary = summary
        .estimate
        .as_ref()
        .map(|estimate| estimate.summary.clone());
    Ok(QuoteResponse {
        id: Uuid::parse_str(&summary.quote_id).map_err(|_| AppError::ServiceUpstream)?,
        status: summary_status(summary.status).into(),
        company_name: summary.organization_name,
        frameworks: summary.frameworks,
        estimate: summary.estimate.as_ref().map(quote_estimate),
        analysis_summary,
        error_code: None,
    })
}

fn quote_estimate(estimate: &canonical_interfaces::QuoteEstimate) -> QuoteEstimate {
    QuoteEstimate {
        low: u64::try_from(estimate.lower_bound_cents.max(0)).unwrap_or_default() / 100,
        high: u64::try_from(estimate.upper_bound_cents.max(0)).unwrap_or_default() / 100,
        currency: estimate.currency.clone(),
    }
}

const fn detail_status(status: QuoteDetailStatus) -> &'static str {
    match status {
        QuoteDetailStatus::Queued => "queued",
        QuoteDetailStatus::Analyzing => "analyzing",
        QuoteDetailStatus::Ready => "ready",
        QuoteDetailStatus::Failed => "failed",
    }
}

const fn summary_status(status: QuoteSummaryStatus) -> &'static str {
    match status {
        QuoteSummaryStatus::Queued => "queued",
        QuoteSummaryStatus::Analyzing => "analyzing",
        QuoteSummaryStatus::Ready => "ready",
        QuoteSummaryStatus::Failed => "failed",
    }
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
                title { "Get a quote · canonical.plus" }
                style {
                    "body{font-family:ui-sans-serif,system-ui,sans-serif;max-width:64rem;margin:0 auto;padding:2rem;line-height:1.5}.card{border:1px solid #8886;border-radius:.75rem;padding:1.25rem;margin:1rem 0}label{display:block;margin:.75rem 0}input,textarea,select,button{font:inherit;padding:.65rem}input,textarea,select{box-sizing:border-box;width:100%}.grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(13rem,1fr));gap:.6rem}.grid label{display:flex;gap:.5rem;align-items:center;margin:0}.grid input{width:auto}.muted{opacity:.72}.error{color:#b42318}.quote-total{font-size:1.5rem;font-weight:700}[data-opto-state=\"pending\"]{border-color:#b7791f}[data-opto-state=\"failed\"]{border-color:#b42318}button[disabled]{opacity:.6;cursor:wait}"
                }
                script type="module" src="/app-assets/app.js" {}
            }
            body {
                nav { a href="/" { "← canonical.plus" } }
                main {
                    h1 { "Get a compliance quote in less than 5 minutes" }
                    p class="muted" {
                        "Signed in as " (actor.email) ". Your answers are private to your account."
                    }
                    p class="muted" {
                        "Do not submit credentials, protected health information, cardholder data, or production evidence."
                    }
                    form class="card" method="post" action="/u/quote" data-opto-quote="true"
                        hx-post="/u/quote" hx-target="#quote-results" hx-swap="afterbegin" {
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
                                option value="audit_ready" { "Audit ready" }
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
                        button type="submit" { "Analyze my quote" }
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
                title { "Quote · canonical.plus" }
            }
            body {
                main {
                    p { a href="/u/quote" { "← All quotes" } }
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
                hx-get={ "/u/quote/" (quote.id) }
                hx-trigger="every 2s"
                hx-swap="outerHTML" {
                h2 { (quote.company_name) }
                p { "Canonical's secure analysis is running." }
                p class="muted" { "This status refreshes automatically." }
            }
        };
    }
    if quote.status == "ready" {
        return html! {
            article id={ "quote-" (quote.id) } class="card" {
                h2 { (quote.company_name) }
                @if let Some(estimate) = quote.estimate.as_ref() {
                    p class="quote-total" {
                        "$" (estimate.low) "–$" (estimate.high) " " (&estimate.currency)
                    }
                }
                p {
                    (quote.analysis_summary.as_deref().unwrap_or("Your preliminary quote is ready."))
                }
                p class="muted" {
                    "This is a preliminary estimate, not an audit opinion or certification."
                }
            }
        };
    }
    html! {
        article id={ "quote-" (quote.id) } class="card" {
            h2 { (quote.company_name) }
            p class="error" role="alert" {
                "We could not finish this quote. Please review your answers and try again."
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
        let expected: serde_json::Value =
            serde_json::from_str(include_str!("../fixtures/quote/v1/request.json")).unwrap();
        assert_eq!(serde_json::to_value(&request).unwrap(), expected);
        assert_eq!(request.context_key.as_deref(), Some("quote-analysis"));
        assert_eq!(request.answers_version, 1);
        assert!(expected.get("contextRecordId").is_none());
        assert!(expected.get("markdown_context").is_none());
        assert!(expected.get("userId").is_none());
    }

    #[test]
    fn parses_the_canonical_submission_fixture() {
        let submission: QuoteSubmissionResponse = serde_json::from_str(include_str!(
            "../fixtures/quote/v1/submission-response.json"
        ))
        .unwrap();
        assert_eq!(submission.quote_id, "11111111-1111-4111-8111-111111111111");
        assert_eq!(submission.status, QuoteSubmissionResponseStatus::Queued);
        assert_eq!(
            submission.stream_url,
            "/api/v1/quotes/11111111-1111-4111-8111-111111111111/events"
        );
    }

    #[test]
    fn maps_the_canonical_detail_contract() {
        let request: serde_json::Value =
            serde_json::from_str(include_str!("../fixtures/quote/v1/request.json")).unwrap();
        let estimate: serde_json::Value =
            serde_json::from_str(include_str!("../fixtures/quote/v1/estimate.json")).unwrap();
        let value = serde_json::json!({
            "quoteId": "11111111-1111-4111-8111-111111111111",
            "status": "ready",
            "request": request,
            "eventsUrl": "/api/v1/quotes/11111111-1111-4111-8111-111111111111/events",
            "createdAt": "2026-08-06T05:00:00Z",
            "updatedAt": "2026-08-06T05:02:00Z",
            "estimate": estimate,
            "problem": null
        });
        let detail: QuoteDetail = serde_json::from_value(value).unwrap();
        let quote = quote_from_detail(detail).unwrap();
        assert_eq!(quote.company_name, "Example Company");
        assert_eq!(quote.estimate.as_ref().unwrap().low, 25_000);
        assert_eq!(
            quote.analysis_summary.as_deref(),
            Some("Preliminary planning range for a multi-framework readiness engagement.")
        );
        assert!(quote.error_code.is_none());
    }
}
