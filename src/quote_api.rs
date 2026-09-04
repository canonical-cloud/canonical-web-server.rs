//! Browser-facing quote BFF client and Maud views for the dedicated API.

use std::{env, sync::Arc, time::Duration};

use axum::http::{HeaderMap, HeaderValue, StatusCode};
use maud::{html, Markup, DOCTYPE};
use reqwest::{Client, Response, Url};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value as JsonValue;
use uuid::Uuid;

use crate::{
    auth::{AuthContext, VerifiedContacts},
    error::AppError,
};

const MAX_RESPONSE_BYTES: usize = 512 * 1024;

#[derive(Clone)]
pub struct QuoteApiClient {
    base_url: String,
    http: Client,
    service_token: Arc<str>,
}

impl QuoteApiClient {
    pub fn from_env() -> Result<Self, AppError> {
        let raw_url = env::var("CANONICAL_API_URL")
            .map_err(|_| AppError::BadRequest("CANONICAL_API_URL is required".into()))?;
        let parsed = Url::parse(&raw_url)
            .map_err(|_| AppError::BadRequest("CANONICAL_API_URL must be absolute".into()))?;
        let loopback = matches!(parsed.host_str(), Some("localhost" | "127.0.0.1" | "::1"))
            || parsed.host_str().is_some_and(|host| host.ends_with(".svc"))
            || parsed.host_str().is_some_and(|host| host.contains(".svc."));
        if parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
            || parsed.path() != "/"
            || (parsed.scheme() != "https" && !(parsed.scheme() == "http" && loopback))
        {
            return Err(AppError::BadRequest(
                "CANONICAL_API_URL must be an HTTPS origin, except for loopback or Kubernetes service DNS"
                    .into(),
            ));
        }
        let service_token = env::var("CANONICAL_WEB_SERVICE_TOKEN")
            .map_err(|_| AppError::BadRequest("CANONICAL_WEB_SERVICE_TOKEN is required".into()))?;
        if service_token.len() < 32 || service_token.trim() != service_token {
            return Err(AppError::BadRequest(
                "CANONICAL_WEB_SERVICE_TOKEN must contain at least 32 bytes".into(),
            ));
        }
        let http = Client::builder()
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("canonical-web-server/0.1")
            .build()?;
        Ok(Self {
            base_url: parsed.origin().ascii_serialization(),
            http,
            service_token: Arc::from(service_token),
        })
    }

    pub async fn create_contact_selection(
        &self,
        actor: &AuthContext,
        contacts: &VerifiedContacts,
    ) -> Result<QuoteContactSelection, AppError> {
        let email = verified_value(contacts.email.as_ref())?;
        let phone = verified_value(contacts.phone.as_ref())?;
        let mut headers = self.headers_for_subject(actor.user_id)?;
        headers.insert(
            "x-canonical-verified-email",
            HeaderValue::from_str(email).map_err(|_| AppError::Crypto)?,
        );
        headers.insert(
            "x-canonical-verified-phone",
            HeaderValue::from_str(phone).map_err(|_| AppError::Crypto)?,
        );
        let response = self
            .http
            .post(format!("{}/v1/quote-contact-selections", self.base_url))
            .headers(headers)
            .json(&QuoteContactSelectionRequest {
                email_confirmed: true,
                phone_confirmed: true,
            })
            .send()
            .await?;
        decode(response, Some(StatusCode::CREATED)).await
    }

    pub async fn create(
        &self,
        actor: &AuthContext,
        request: &QuoteRequest,
    ) -> Result<QuoteSubmissionResponse, AppError> {
        let response = self
            .http
            .post(format!("{}/v1/quotes", self.base_url))
            .headers(self.headers_for_subject(actor.user_id)?)
            .json(request)
            .send()
            .await?;
        decode(response, Some(StatusCode::ACCEPTED)).await
    }

    pub async fn resubmit(
        &self,
        subject: Uuid,
        quote_id: Uuid,
        request: &QuoteRequest,
    ) -> Result<QuoteResubmissionResponse, AppError> {
        let response = self
            .http
            .post(format!(
                "{}/v1/quotes/{quote_id}/submissions",
                self.base_url
            ))
            .headers(self.headers_for_subject(subject)?)
            .json(request)
            .send()
            .await?;
        decode(response, Some(StatusCode::ACCEPTED)).await
    }

    pub async fn get(
        &self,
        actor: &AuthContext,
        quote_id: Uuid,
    ) -> Result<QuoteResponse, AppError> {
        self.get_for_subject(actor.user_id, quote_id).await
    }

    pub async fn get_for_subject(
        &self,
        subject: Uuid,
        quote_id: Uuid,
    ) -> Result<QuoteResponse, AppError> {
        let response = self
            .http
            .get(format!("{}/v1/quotes/{quote_id}", self.base_url))
            .headers(self.headers_for_subject(subject)?)
            .send()
            .await?;
        decode(response, Some(StatusCode::OK)).await
    }

    pub async fn list(&self, actor: &AuthContext) -> Result<Vec<QuoteResponse>, AppError> {
        let response = self
            .http
            .get(format!("{}/v1/quotes", self.base_url))
            .headers(self.headers_for_subject(actor.user_id)?)
            .send()
            .await?;
        decode(response, Some(StatusCode::OK)).await
    }

    pub async fn redeem(&self, capability: &str) -> Result<RedeemedQuoteLink, AppError> {
        let response = self
            .http
            .post(format!("{}/v1/quote-links/redeem", self.base_url))
            .headers(self.service_headers()?)
            .json(&RedeemQuoteLinkRequest { capability })
            .send()
            .await?;
        decode(response, Some(StatusCode::OK)).await
    }

    fn headers_for_subject(&self, subject: Uuid) -> Result<HeaderMap, AppError> {
        let mut headers = self.service_headers()?;
        headers.insert(
            "x-canonical-subject",
            HeaderValue::from_str(&subject.to_string()).map_err(|_| AppError::Crypto)?,
        );
        Ok(headers)
    }

    fn service_headers(&self) -> Result<HeaderMap, AppError> {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-canonical-internal-token",
            HeaderValue::from_str(&self.service_token).map_err(|_| AppError::Crypto)?,
        );
        Ok(headers)
    }
}

fn verified_value(contact: Option<&crate::auth::VerifiedContact>) -> Result<&str, AppError> {
    contact
        .filter(|contact| contact.verified && !contact.value.trim().is_empty())
        .map(|contact| contact.value.as_str())
        .ok_or_else(|| {
            AppError::BadRequest("verify and confirm both contact methods before submitting".into())
        })
}

async fn decode<T: DeserializeOwned>(
    response: Response,
    expected: Option<StatusCode>,
) -> Result<T, AppError> {
    let status = response.status();
    if status == StatusCode::NOT_FOUND {
        return Err(AppError::NotFound);
    }
    if matches!(
        status,
        StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY | StatusCode::CONFLICT
    ) {
        return Err(AppError::BadRequest(
            "review the quote and verified contact choices, then try again".into(),
        ));
    }
    if expected.is_some_and(|expected| status != expected) || !status.is_success() {
        tracing::warn!(%status, "dedicated quote API rejected the request");
        return Err(AppError::ServiceUpstream);
    }
    let bytes = response.bytes().await?;
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err(AppError::ServiceUpstream);
    }
    serde_json::from_slice(&bytes).map_err(AppError::from)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct QuoteContactSelectionRequest {
    email_confirmed: bool,
    phone_confirmed: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuoteContactSelection {
    pub contact_selection_id: Uuid,
    pub email_masked: String,
    pub expires_at: String,
    pub phone_masked: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuoteRequest {
    pub annual_revenue_band: Option<String>,
    pub answers_version: u32,
    pub contact_name: String,
    pub contact_selection_id: Uuid,
    pub context_key: String,
    pub current_stage: String,
    pub data_sensitivity: Vec<String>,
    pub employee_count: u32,
    pub frameworks: Vec<String>,
    pub has_incident_response_plan: bool,
    pub has_policies: bool,
    pub has_risk_assessment: bool,
    pub has_security_program: bool,
    pub has_vendor_management: bool,
    pub infrastructure: Vec<String>,
    pub notes: Option<String>,
    pub organization_name: String,
    pub target_date: Option<String>,
    pub website: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuoteSubmissionResponse {
    pub access_link_expires_at: String,
    pub created_at: String,
    pub quote_id: Uuid,
    pub revision: i32,
    pub status: String,
    pub stream_url: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuoteResubmissionResponse {
    pub access_link_expires_at: String,
    pub quote_id: Uuid,
    pub revision: i32,
    pub status: String,
    pub stream_url: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuoteResponse {
    pub access_link_expires_at: String,
    pub analysis: Option<JsonValue>,
    pub created_at: String,
    pub error_code: Option<String>,
    pub frameworks: Vec<String>,
    pub organization_name: String,
    pub persistence: String,
    pub quote_id: Uuid,
    pub request: QuoteRequest,
    pub revision: i32,
    pub status: String,
    pub updated_at: String,
}

#[derive(Serialize)]
struct RedeemQuoteLinkRequest<'a> {
    capability: &'a str,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RedeemedQuoteLink {
    pub expires_at: String,
    pub owner_subject: Uuid,
    pub quote_id: Uuid,
}

pub fn quote_page(
    actor: &AuthContext,
    contacts: &VerifiedContacts,
    quotes: &[QuoteResponse],
    form_action: &str,
) -> Markup {
    let csrf = actor.csrf_token.as_deref().unwrap_or_default();
    let email = contacts
        .email
        .as_ref()
        .filter(|contact| contact.verified)
        .map(|contact| contact.value.as_str());
    let phone = contacts
        .phone
        .as_ref()
        .filter(|contact| contact.verified)
        .map(|contact| contact.value.as_str());
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "Get a quote · canonical.plus" }
                style {
                    "body{font-family:ui-sans-serif,system-ui,sans-serif;max-width:64rem;margin:0 auto;padding:2rem;line-height:1.5}.card{border:1px solid #8886;border-radius:.75rem;padding:1.25rem;margin:1rem 0}label{display:block;margin:.75rem 0}input,textarea,select,button{font:inherit;padding:.65rem}input,textarea,select{box-sizing:border-box;width:100%}.grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(13rem,1fr));gap:.6rem}.grid label{display:flex;gap:.5rem;align-items:center;margin:0}.grid input{width:auto}.muted{opacity:.72}.error{color:#b42318}.success{color:#087a42}.confirm{display:flex;gap:.65rem;align-items:center;border:1px solid #8888;border-radius:.6rem;padding:.8rem}.confirm input{width:auto}.quote-total{font-size:1.5rem;font-weight:700}"
                }
                script type="module" src="/app-assets/app.js" {}
            }
            body {
                nav { a href="/" { "← canonical.plus" } }
                main {
                    h1 { "Get a compliance quote in less than 5 minutes" }
                    p class="muted" {
                        "Verified account: " (actor.email) ". Once accepted, processing continues even if you close this page."
                    }

                    @if phone.is_none() {
                        section class="card" {
                            h2 { "Verify your phone" }
                            p { "We verify the phone once with a code, then use Twilio Messaging to send your private quote link." }
                            form method="post" action={ (form_action) "/phone/request" }
                                hx-post={ (form_action) "/phone/request" }
                                hx-target="#phone-verification" hx-swap="innerHTML" {
                                input type="hidden" name="csrf" value=(csrf);
                                label { "Mobile phone in E.164 format"
                                    input name="phone" type="tel" required maxlength="16" placeholder="+14155550100";
                                }
                                button type="submit" { "Text me a verification code" }
                            }
                            div id="phone-verification" aria-live="polite" {}
                        }
                    }

                    form class="card" method="post" action=(form_action)
                        hx-post=(form_action) hx-target="#quote-results" hx-swap="innerHTML" {
                        input type="hidden" name="csrf" value=(csrf);
                        h2 { "Contact methods" }
                        p class="muted" { "Verified details are prefilled, but neither is selected until you click its confirmation control." }
                        @if let Some(email) = email {
                            label class="confirm" {
                                input type="checkbox" name="confirm_email" required;
                                span { "Use verified email " strong { (email) } }
                            }
                        } @else {
                            p class="error" role="alert" { "Verify your email through Shared Auth before submitting." }
                        }
                        @if let Some(phone) = phone {
                            label class="confirm" {
                                input type="checkbox" name="confirm_phone" required;
                                span { "Use verified phone ending in " strong { (phone_suffix(phone)) } }
                            }
                        } @else {
                            p class="error" role="alert" { "Complete phone verification above before submitting." }
                        }

                        h2 { "Organization" }
                        label { "Organization name" input name="organization_name" required maxlength="200"; }
                        label { "Your name" input name="contact_name" required maxlength="160"; }
                        label { "Website (optional)" input name="website" type="url" maxlength="2048"; }
                        label { "Number of employees"
                            input type="number" name="employee_count" min="1" max="1000000" required;
                        }

                        h2 { "Frameworks" }
                        div class="grid" {
                            (checkbox("soc2_type_1", "SOC 2 Type I"))
                            (checkbox("soc2_type_2", "SOC 2 Type II"))
                            (checkbox("nist_csf_2", "NIST CSF 2.0"))
                            (checkbox("nist_800_53", "NIST SP 800-53"))
                            (checkbox("hipaa", "HIPAA"))
                            (checkbox("iso_27001", "ISO 27001"))
                            (checkbox("pci_dss_4", "PCI DSS 4"))
                            (checkbox("fedramp", "FedRAMP"))
                            (checkbox("gdpr", "GDPR"))
                        }

                        h2 { "Scope" }
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
                        div class="grid" {
                            (checkbox("infra_aws", "AWS"))
                            (checkbox("infra_azure", "Azure"))
                            (checkbox("infra_gcp", "GCP"))
                            (checkbox("infra_supabase", "Supabase"))
                            (checkbox("infra_on_prem", "On premises"))
                            (checkbox("infra_saas_only", "SaaS only"))
                            (checkbox("data_confidential", "Confidential data"))
                            (checkbox("data_pii", "PII"))
                            (checkbox("data_phi", "PHI"))
                            (checkbox("data_pci", "Payment-card data"))
                        }
                        h3 { "Current program" }
                        div class="grid" {
                            (checkbox("has_security_program", "Security program"))
                            (checkbox("has_policies", "Policies"))
                            (checkbox("has_risk_assessment", "Risk assessment"))
                            (checkbox("has_incident_response_plan", "Incident response plan"))
                            (checkbox("has_vendor_management", "Vendor management"))
                        }
                        label { "Anything else we should know"
                            textarea name="notes" rows="5" maxlength="5000" {}
                        }
                        button type="submit" disabled[phone.is_none() || email.is_none()] {
                            "Confirm contacts and submit quote"
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

fn checkbox(name: &str, label: &str) -> Markup {
    html! { label { input type="checkbox" name=(name); (label) } }
}

pub fn phone_challenge_fragment(
    csrf: &str,
    form_action: &str,
    challenge_id: Uuid,
    phone_hint: &str,
) -> Markup {
    html! {
        p class="success" { "Code sent to " (phone_hint) "." }
        form method="post" action={ (form_action) "/phone/verify" }
            hx-post={ (form_action) "/phone/verify" }
            hx-target="#phone-verification" hx-swap="innerHTML" {
            input type="hidden" name="csrf" value=(csrf);
            input type="hidden" name="challenge_id" value=(challenge_id);
            label { "Verification code"
                input name="code" inputmode="numeric" autocomplete="one-time-code"
                    minlength="4" maxlength="10" required;
            }
            button type="submit" { "Verify phone" }
        }
    }
}

pub fn phone_verified_fragment(return_path: &str) -> Markup {
    html! {
        p class="success" role="status" { "Phone verified." }
        p { a href=(return_path) { "Continue to contact confirmation" } }
    }
}

pub fn quote_detail_page(
    identity_label: &str,
    csrf: &str,
    quote: &QuoteResponse,
    edit_action: &str,
) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "Quote · canonical.plus" }
                style { "body{font-family:ui-sans-serif,system-ui,sans-serif;max-width:64rem;margin:0 auto;padding:2rem;line-height:1.5}.card{border:1px solid #8886;border-radius:.75rem;padding:1.25rem;margin:1rem 0}label{display:block;margin:.75rem 0}input,textarea,select,button{font:inherit;padding:.65rem}input,textarea,select{box-sizing:border-box;width:100%}.grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(13rem,1fr));gap:.6rem}.grid label{display:flex;gap:.5rem;align-items:center}.grid input{width:auto}.muted{opacity:.72}.error{color:#b42318}.quote-total{font-size:1.5rem;font-weight:700}" }
            }
            body {
                main {
                    p class="muted" { (identity_label) }
                    (quote_status_fragment(quote))
                    @if matches!(quote.status.as_str(), "ready" | "failed") {
                        (quote_edit_form(csrf, quote, edit_action))
                    }
                }
            }
        }
    }
}

fn quote_edit_form(csrf: &str, quote: &QuoteResponse, edit_action: &str) -> Markup {
    let request = &quote.request;
    html! {
        form class="card" method="post" action=(edit_action) {
            input type="hidden" name="csrf" value=(csrf);
            input type="hidden" name="contact_selection_id" value=(request.contact_selection_id);
            h2 { "Edit and resubmit" }
            p class="muted" { "This creates revision " (quote.revision + 1) "; revision " (quote.revision) " remains unchanged." }
            label { "Organization name" input name="organization_name" value=(&request.organization_name) required maxlength="200"; }
            label { "Your name" input name="contact_name" value=(&request.contact_name) required maxlength="160"; }
            label { "Number of employees" input type="number" name="employee_count" value=(request.employee_count) min="1" max="1000000" required; }
            input type="hidden" name="frameworks" value=(request.frameworks.join(","));
            input type="hidden" name="current_stage" value=(&request.current_stage);
            input type="hidden" name="infrastructure" value=(request.infrastructure.join(","));
            input type="hidden" name="data_sensitivity" value=(request.data_sensitivity.join(","));
            input type="hidden" name="has_security_program" value=(request.has_security_program);
            input type="hidden" name="has_policies" value=(request.has_policies);
            input type="hidden" name="has_risk_assessment" value=(request.has_risk_assessment);
            input type="hidden" name="has_incident_response_plan" value=(request.has_incident_response_plan);
            input type="hidden" name="has_vendor_management" value=(request.has_vendor_management);
            label { "Additional scoping details"
                textarea name="notes" rows="5" maxlength="5000" { (request.notes.as_deref().unwrap_or_default()) }
            }
            button type="submit" { "Submit as a new revision" }
        }
    }
}

pub fn quote_status_fragment(quote: &QuoteResponse) -> Markup {
    quote_status_fragment_at(quote, &format!("/u/quote/{}", quote.quote_id))
}

pub fn quote_status_fragment_at(quote: &QuoteResponse, detail_path: &str) -> Markup {
    if matches!(quote.status.as_str(), "queued" | "analyzing") {
        return html! {
            article id={ "quote-" (quote.quote_id) } class="card"
                hx-get=(detail_path)
                hx-trigger="every 2s" hx-swap="outerHTML" {
                h2 { (&quote.organization_name) " · revision " (quote.revision) }
                p { "Canonical's secure analysis is running." }
                p class="muted" { "You may close this page. Durable processing and the private-link message continue in the background." }
            }
        };
    }
    if quote.status == "ready" {
        let low = analysis_integer(quote, "estimated_total_fee_low");
        let high = analysis_integer(quote, "estimated_total_fee_high");
        return html! {
            article id={ "quote-" (quote.quote_id) } class="card" {
                h2 { (&quote.organization_name) " · revision " (quote.revision) }
                @if let (Some(low), Some(high)) = (low, high) {
                    p class="quote-total" { "$" (low) "–$" (high) " USD" }
                }
                p { (analysis_text(quote, "summary").unwrap_or("Your preliminary quote is ready.")) }
                p class="muted" { "This is a preliminary estimate, not an audit opinion or certification." }
            }
        };
    }
    html! {
        article id={ "quote-" (quote.quote_id) } class="card" {
            h2 { (&quote.organization_name) " · revision " (quote.revision) }
            p class="error" role="alert" {
                "We could not finish this revision. You can edit the scope and submit a new revision."
            }
        }
    }
}

fn analysis_integer(quote: &QuoteResponse, field: &str) -> Option<i64> {
    quote.analysis.as_ref()?.get(field)?.as_i64()
}

fn analysis_text<'a>(quote: &'a QuoteResponse, field: &str) -> Option<&'a str> {
    quote.analysis.as_ref()?.get(field)?.as_str()
}

fn phone_suffix(value: &str) -> String {
    value
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_contract_uses_current_camel_case_fields() {
        let value = serde_json::json!({
            "quoteId": Uuid::nil(),
            "revision": 1,
            "status": "queued",
            "organizationName": "Example",
            "frameworks": ["soc2_type_2"],
            "persistence": "postgres",
            "accessLinkExpiresAt": "2026-08-31T00:00:00Z",
            "createdAt": "2026-08-06T00:00:00Z",
            "updatedAt": "2026-08-06T00:00:00Z",
            "analysis": null,
            "errorCode": null,
            "request": {
                "annualRevenueBand": null,
                "answersVersion": 1,
                "contactName": "Casey",
                "contactSelectionId": Uuid::nil(),
                "contextKey": "quote-analysis",
                "currentStage": "readiness",
                "dataSensitivity": ["confidential"],
                "employeeCount": 10,
                "frameworks": ["soc2_type_2"],
                "hasIncidentResponsePlan": true,
                "hasPolicies": true,
                "hasRiskAssessment": false,
                "hasSecurityProgram": true,
                "hasVendorManagement": false,
                "infrastructure": ["aws"],
                "notes": null,
                "organizationName": "Example",
                "targetDate": null,
                "website": null
            }
        });
        let quote: QuoteResponse = serde_json::from_value(value).unwrap();
        assert_eq!(quote.organization_name, "Example");
        assert_eq!(quote.frameworks, ["soc2_type_2"]);
    }

    #[test]
    fn phone_display_discloses_only_suffix() {
        assert_eq!(phone_suffix("+14155550100"), "0100");
    }
}
