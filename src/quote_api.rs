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
    ) -> Result<QuoteResponse, AppError> {
        let payload = ApiCreateQuoteRequest {
            frameworks: &request.frameworks,
            notes: request.analysis_notes(),
            organization: ApiOrganization {
                employee_count: request.employee_count,
                industry: &request.industry,
                legal_name: &request.company_name,
            },
        };
        let response = self
            .http
            .post(format!("{}/v1/quotes", self.base_url))
            .headers(self.headers(actor)?)
            .json(&payload)
            .send()
            .await?;
        let record: ApiQuoteRecord = decode(response, StatusCode::ACCEPTED).await?;
        Ok(record.into())
    }

    pub async fn get(
        &self,
        actor: &AuthContext,
        quote_id: Uuid,
    ) -> Result<QuoteResponse, AppError> {
        let response = self
            .http
            .get(format!("{}/v1/quotes/{quote_id}", self.base_url))
            .headers(self.headers(actor)?)
            .send()
            .await?;
        let record: ApiQuoteRecord = decode(response, StatusCode::OK).await?;
        Ok(record.into())
    }

    pub async fn list(&self, actor: &AuthContext) -> Result<Vec<QuoteResponse>, AppError> {
        let response = self
            .http
            .get(format!("{}/v1/quotes", self.base_url))
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

#[derive(Clone, Debug)]
pub struct QuoteRequest {
    pub company_name: String,
    pub industry: String,
    pub employee_count: u32,
    pub annual_revenue_usd: Option<u64>,
    pub frameworks: Vec<String>,
    pub cloud_providers: Vec<String>,
    pub handles_phi: bool,
    pub handles_payment_cards: bool,
    pub security_program_maturity: String,
    pub target_timeline: String,
    pub existing_certifications: Vec<String>,
    pub notes: Option<String>,
}

impl QuoteRequest {
    fn analysis_notes(&self) -> Option<String> {
        let mut lines = vec![
            format!(
                "Security program maturity: {}",
                self.security_program_maturity
            ),
            format!("Requested timeline: {}", self.target_timeline),
            format!("Handles protected health information: {}", self.handles_phi),
            format!("Handles payment-card data: {}", self.handles_payment_cards),
        ];
        if let Some(revenue) = self.annual_revenue_usd {
            lines.push(format!("Annual revenue USD: {revenue}"));
        }
        if !self.cloud_providers.is_empty() {
            lines.push(format!(
                "Cloud providers: {}",
                self.cloud_providers.join(", ")
            ));
        }
        if !self.existing_certifications.is_empty() {
            lines.push(format!(
                "Existing certifications: {}",
                self.existing_certifications.join(", ")
            ));
        }
        if let Some(notes) = self.notes.as_deref() {
            lines.push(format!("Additional customer notes:\n{notes}"));
        }
        Some(lines.join("\n"))
    }
}

#[derive(Serialize)]
struct ApiCreateQuoteRequest<'a> {
    frameworks: &'a [String],
    notes: Option<String>,
    organization: ApiOrganization<'a>,
}

#[derive(Serialize)]
struct ApiOrganization<'a> {
    employee_count: u32,
    industry: &'a str,
    legal_name: &'a str,
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
                title { "Get a quote · canonical.plus" }
                style {
                    "body{font-family:ui-sans-serif,system-ui,sans-serif;max-width:64rem;margin:0 auto;padding:2rem;line-height:1.5}.card{border:1px solid #8886;border-radius:.75rem;padding:1.25rem;margin:1rem 0}label{display:block;margin:.75rem 0}input,textarea,select,button{font:inherit;padding:.65rem}input,textarea,select{box-sizing:border-box;width:100%}.grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(13rem,1fr));gap:.6rem}.grid label{display:flex;gap:.5rem;align-items:center;margin:0}.grid input{width:auto}.muted{opacity:.72}.error{color:#b42318}.quote-total{font-size:1.5rem;font-weight:700}"
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
                    form class="card" method="post" action="/u/quote"
                        hx-post="/u/quote" hx-target="#quote-results" hx-swap="innerHTML" {
                        input type="hidden" name="csrf" value=(csrf);
                        h2 { "Company" }
                        label { "Company name" input name="company_name" required maxlength="200"; }
                        label { "Industry" input name="industry" required maxlength="120"; }
                        label { "Number of employees"
                            input type="number" name="employee_count" min="1" max="1000000" required;
                        }
                        label { "Annual revenue in USD (optional)"
                            input type="number" name="annual_revenue_usd" min="0" max="10000000000000";
                        }

                        h2 { "Frameworks" }
                        div class="grid" {
                            label { input type="checkbox" name="soc2"; "SOC 2" }
                            label { input type="checkbox" name="nist_csf"; "NIST CSF" }
                            label { input type="checkbox" name="nist_800_53"; "NIST SP 800-53" }
                            label { input type="checkbox" name="hipaa"; "HIPAA" }
                            label { input type="checkbox" name="iso_27001"; "ISO 27001" }
                            label { input type="checkbox" name="pci_dss"; "PCI DSS" }
                            label { input type="checkbox" name="fedramp"; "FedRAMP" }
                            label { input type="checkbox" name="gdpr"; "GDPR" }
                        }

                        h2 { "Scope" }
                        label { "Security program maturity"
                            select name="security_program_maturity" required {
                                option value="" { "Choose a stage" }
                                option value="none" { "Starting from scratch" }
                                option value="informal" { "Informal practices" }
                                option value="documented" { "Controls documented" }
                                option value="managed" { "Managed program" }
                                option value="audited" { "Previously audited" }
                            }
                        }
                        label { "Target timeline"
                            select name="target_timeline" required {
                                option value="" { "Choose a timeline" }
                                option value="under_3_months" { "Under 3 months" }
                                option value="3_to_6_months" { "3–6 months" }
                                option value="6_to_12_months" { "6–12 months" }
                                option value="over_12_months" { "More than 12 months" }
                                option value="unsure" { "Still exploring" }
                            }
                        }
                        div class="grid" {
                            label { input type="checkbox" name="handles_phi"; "Handles protected health information" }
                            label { input type="checkbox" name="handles_payment_cards"; "Handles payment-card data" }
                        }
                        label { "Cloud providers (comma-separated)"
                            input name="cloud_providers" maxlength="640" placeholder="AWS, GCP, Azure, Cloudflare";
                        }
                        label { "Existing certifications (comma-separated)"
                            input name="existing_certifications" maxlength="1920" placeholder="ISO 27001, SOC 2 Type II";
                        }
                        label { "Anything else we should know"
                            textarea name="notes" rows="5" maxlength="4000" {}
                        }
                        button type="submit" { "Analyze my quote" }
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
    if quote.status == "completed" {
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

    #[test]
    fn browser_payload_cannot_select_a_database_context() {
        let frameworks = vec!["soc2".to_owned()];
        let payload = ApiCreateQuoteRequest {
            frameworks: &frameworks,
            notes: None,
            organization: ApiOrganization {
                employee_count: 10,
                industry: "Software",
                legal_name: "Example",
            },
        };
        let value = serde_json::to_value(payload).unwrap();
        assert!(value.get("context_record_id").is_none());
        assert!(value.get("markdown_context").is_none());
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
            "frameworks": ["soc2"],
            "gemini_model": "gemini-3.1-pro-preview",
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
