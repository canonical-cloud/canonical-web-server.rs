use axum::{
    extract::{DefaultBodyLimit, Form, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Redirect, Response},
    routing::get,
    Router,
};
use maud::{html, Markup, DOCTYPE};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    auth::{require_csrf, require_origin, OptionalAuthenticated, SessionAuthenticated},
    error::AppError,
    integrations::{CreateQuoteRequest, QuoteApiError, QuoteResponse},
    AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(page).post(submit))
        .route("/{quote_id}", get(status))
        .layer(DefaultBodyLimit::max(32 * 1024))
}

pub async fn page(
    State(state): State<AppState>,
    OptionalAuthenticated(actor): OptionalAuthenticated,
) -> Result<Response, AppError> {
    let Some(actor) = actor else {
        return Ok(Redirect::to("/auth/shared/start?return_to=%2Fu%2Fquote").into_response());
    };
    let quotes = state
        .quote_api
        .list_quotes(actor.user_id, &actor.email)
        .await
        .map_err(map_quote_error)?;
    Ok(quote_page(&actor.email, actor.csrf_token.as_deref(), &quotes).into_response())
}

#[derive(Deserialize)]
pub struct QuoteForm {
    csrf: String,
    company_name: String,
    employee_count: String,
    annual_revenue_usd: Option<String>,
    soc2: Option<String>,
    nist_csf: Option<String>,
    hipaa: Option<String>,
    iso_27001: Option<String>,
    pci_dss: Option<String>,
    gdpr: Option<String>,
    fedramp: Option<String>,
    cis_controls: Option<String>,
    cloud_providers: String,
    handles_phi: Option<String>,
    handles_payment_cards: Option<String>,
    security_program_maturity: String,
    target_timeline: String,
    existing_certifications: String,
    notes: Option<String>,
}

pub async fn submit(
    State(state): State<AppState>,
    headers: HeaderMap,
    SessionAuthenticated(actor): SessionAuthenticated,
    Form(form): Form<QuoteForm>,
) -> Result<Response, AppError> {
    require_origin(&headers, &state)?;
    require_csrf(&actor, &headers, Some(&form.csrf))?;
    let input = form.into_request()?;
    let quote = state
        .quote_api
        .create_quote(actor.user_id, &actor.email, &input)
        .await
        .map_err(map_quote_error)?;
    if headers.contains_key("hx-request") {
        Ok((StatusCode::ACCEPTED, quote_fragment(&quote)).into_response())
    } else {
        Ok(Redirect::to("/u/quote").into_response())
    }
}

pub async fn status(
    State(state): State<AppState>,
    SessionAuthenticated(actor): SessionAuthenticated,
    Path(quote_id): Path<Uuid>,
) -> Result<Markup, AppError> {
    let quote = state
        .quote_api
        .get_quote(actor.user_id, &actor.email, quote_id)
        .await
        .map_err(map_quote_error)?;
    Ok(quote_fragment(&quote))
}

impl QuoteForm {
    fn into_request(self) -> Result<CreateQuoteRequest, AppError> {
        let employee_count = parse_u32("employeeCount", &self.employee_count)?;
        let annual_revenue_usd = self
            .annual_revenue_usd
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| parse_u64("annualRevenueUsd", value))
            .transpose()?;
        let mut frameworks = Vec::new();
        for (selected, framework) in [
            (self.soc2, "soc2"),
            (self.nist_csf, "nist_csf"),
            (self.hipaa, "hipaa"),
            (self.iso_27001, "iso_27001"),
            (self.pci_dss, "pci_dss"),
            (self.gdpr, "gdpr"),
            (self.fedramp, "fedramp"),
            (self.cis_controls, "cis_controls"),
        ] {
            if selected.is_some() {
                frameworks.push(framework.to_owned());
            }
        }
        if frameworks.is_empty() {
            return Err(AppError::BadRequest(
                "select at least one compliance framework".into(),
            ));
        }
        Ok(CreateQuoteRequest {
            company_name: self.company_name,
            employee_count,
            annual_revenue_usd,
            frameworks,
            cloud_providers: split_list(&self.cloud_providers),
            handles_phi: self.handles_phi.is_some(),
            handles_payment_cards: self.handles_payment_cards.is_some(),
            security_program_maturity: self.security_program_maturity,
            target_timeline: self.target_timeline,
            existing_certifications: split_list(&self.existing_certifications),
            notes: self.notes,
        })
    }
}

fn parse_u32(name: &str, value: &str) -> Result<u32, AppError> {
    value
        .trim()
        .replace(',', "")
        .parse()
        .map_err(|_| AppError::BadRequest(format!("{name} must be a positive integer")))
}

fn parse_u64(name: &str, value: &str) -> Result<u64, AppError> {
    value
        .trim()
        .trim_start_matches('$')
        .replace(',', "")
        .parse()
        .map_err(|_| AppError::BadRequest(format!("{name} must be a positive integer")))
}

fn split_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect()
}

fn map_quote_error(error: QuoteApiError) -> AppError {
    match error {
        QuoteApiError::BadRequest => {
            AppError::BadRequest("the quote fields were not accepted".into())
        }
        QuoteApiError::NotFound => AppError::NotFound,
        QuoteApiError::Unavailable | QuoteApiError::InvalidResponse => {
            tracing::warn!(%error, "quote API request failed");
            AppError::QuoteUpstream
        }
    }
}

fn quote_page(email: &str, csrf: Option<&str>, quotes: &[QuoteResponse]) -> Markup {
    let csrf = csrf.unwrap_or_default();
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                meta name="color-scheme" content="dark";
                title { "Compliance quote · Canonical" }
                style { (QUOTE_STYLE) }
                script src="/app-assets/app.js" defer {}
            }
            body {
                header class="topbar" {
                    a class="brand" href="/" { "canonical.plus" }
                    div class="account" {
                        span { (email) }
                        form method="post" action="/auth/logout" {
                            input type="hidden" name="csrf" value=(csrf);
                            button class="link" type="submit" { "Sign out" }
                        }
                    }
                }
                main {
                    section class="hero" {
                        p class="eyebrow" { "PRELIMINARY COMPLIANCE SCOPING" }
                        h1 { "Get a quote in less than five minutes." }
                        p {
                            "Tell us about your company and target frameworks. Canonical combines your answers with versioned delivery context, then produces a non-binding scope and estimate for human review."
                        }
                    }
                    div class="layout" {
                        form class="card form" method="post" action="/u/quote/"
                            hx-post="/u/quote/" hx-target="#quote-result" hx-swap="outerHTML" {
                            input type="hidden" name="csrf" value=(csrf);
                            h2 { "1. Company" }
                            label { "Company name" input name="company_name" required maxlength="200"; }
                            div class="two" {
                                label { "Employees" input type="number" name="employee_count" min="1" max="1000000" required; }
                                label { "Annual revenue (USD, optional)" input inputmode="numeric" name="annual_revenue_usd"; }
                            }
                            h2 { "2. Frameworks" }
                            fieldset class="checks" {
                                legend { "Select every relevant framework" }
                                (framework_checkbox("soc2", "SOC 2"))
                                (framework_checkbox("nist_csf", "NIST CSF"))
                                (framework_checkbox("hipaa", "HIPAA"))
                                (framework_checkbox("iso_27001", "ISO 27001"))
                                (framework_checkbox("pci_dss", "PCI DSS"))
                                (framework_checkbox("gdpr", "GDPR"))
                                (framework_checkbox("fedramp", "FedRAMP"))
                                (framework_checkbox("cis_controls", "CIS Controls"))
                            }
                            h2 { "3. Environment" }
                            label { "Cloud providers (comma separated)" input name="cloud_providers" placeholder="AWS, Cloudflare, Supabase"; }
                            div class="checks compact" {
                                label { input type="checkbox" name="handles_phi" value="yes"; "Handles protected health information" }
                                label { input type="checkbox" name="handles_payment_cards" value="yes"; "Handles payment-card data" }
                            }
                            div class="two" {
                                label { "Security-program maturity"
                                    select name="security_program_maturity" required {
                                        option value="none" { "No formal program" }
                                        option value="informal" { "Informal controls" }
                                        option value="documented" { "Documented" }
                                        option value="managed" { "Managed and measured" }
                                        option value="audited" { "Previously audited" }
                                    }
                                }
                                label { "Target timeline"
                                    select name="target_timeline" required {
                                        option value="under_3_months" { "Under 3 months" }
                                        option value="3_to_6_months" { "3–6 months" }
                                        option value="6_to_12_months" { "6–12 months" }
                                        option value="over_12_months" { "Over 12 months" }
                                        option value="unsure" { "Not sure" }
                                    }
                                }
                            }
                            label { "Existing certifications (comma separated)" input name="existing_certifications"; }
                            label { "Anything else we should know?" textarea name="notes" maxlength="4000" rows="5" {} }
                            button class="primary" type="submit" { "Analyze and prepare quote" }
                            p class="fine" { "This automated result is preliminary and is not legal advice, an audit opinion, certification, or a binding offer." }
                        }
                        aside {
                            div id="quote-result" class="card result" {
                                h2 { "Your result" }
                                p class="muted" { "Submit the form to start a durable analysis. You may leave and return; the request is saved before model processing begins." }
                            }
                            @if !quotes.is_empty() {
                                div class="card history" {
                                    h2 { "Recent quotes" }
                                    @for quote in quotes {
                                        (quote_fragment(quote))
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn framework_checkbox(name: &str, label: &str) -> Markup {
    html! { label { input type="checkbox" name=(name) value="yes"; (label) } }
}

fn quote_fragment(quote: &QuoteResponse) -> Markup {
    let polling = matches!(quote.status.as_str(), "queued" | "analyzing");
    let label = framework_labels(&quote.frameworks).join(", ");
    html! {
        section id="quote-result" class="quote"
            hx-get=[polling.then(|| format!("/u/quote/{}", quote.id))]
            hx-trigger=[polling.then_some("load delay:2s")]
            hx-swap=[polling.then_some("outerHTML")] {
            p class="eyebrow" { (quote.status.to_uppercase()) }
            h3 { (quote.company_name) }
            p class="muted" { (label) }
            @match quote.status.as_str() {
                "queued" => p { "Your request is safely queued." },
                "analyzing" => p { "Canonical is combining your answers with the current delivery context." },
                "ready" => {
                    @if let Some(estimate) = &quote.estimate {
                        p class="estimate" {
                            "$" (format_usd(estimate.low)) "–$" (format_usd(estimate.high)) " " (estimate.currency)
                        }
                    }
                    pre { (quote.analysis_markdown.as_deref().unwrap_or("Analysis is ready.")) }
                },
                "failed" => p class="error" {
                    (quote.failure_message.as_deref().unwrap_or("The analysis needs human review."))
                },
                _ => p { "Quote status is being refreshed." },
            }
        }
    }
}

fn framework_labels(frameworks: &[String]) -> Vec<&'static str> {
    frameworks
        .iter()
        .map(|value| match value.as_str() {
            "soc2" => "SOC 2",
            "nist_csf" => "NIST CSF",
            "hipaa" => "HIPAA",
            "iso_27001" => "ISO 27001",
            "pci_dss" => "PCI DSS",
            "gdpr" => "GDPR",
            "fedramp" => "FedRAMP",
            "cis_controls" => "CIS Controls",
            _ => "Other",
        })
        .collect()
}

fn format_usd(value: u64) -> String {
    let digits = value.to_string();
    let mut output = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, character) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            output.push(',');
        }
        output.push(character);
    }
    output
}

const QUOTE_STYLE: &str = r#"
:root{font-family:Inter,ui-sans-serif,system-ui,sans-serif;background:#071018;color:#eff7f4;line-height:1.5}*{box-sizing:border-box}body{margin:0;background:radial-gradient(circle at 75% 0,#153b37 0,transparent 34%),#071018}.topbar{display:flex;justify-content:space-between;align-items:center;padding:1rem clamp(1rem,4vw,4rem);border-bottom:1px solid #24403d;background:#081410cc;backdrop-filter:blur(14px);position:sticky;top:0;z-index:3}.brand{color:#b8ffce;text-decoration:none;font-weight:800;letter-spacing:.04em}.account{display:flex;gap:1rem;align-items:center;color:#a9bbb7;font-size:.9rem}.account form{margin:0}.link{border:0;background:none;color:#b8ffce;cursor:pointer}main{max-width:1200px;margin:auto;padding:clamp(2rem,6vw,5rem) 1rem}.hero{max-width:800px;margin-bottom:2rem}.hero h1{font-size:clamp(2.3rem,6vw,5rem);line-height:1;margin:.4rem 0 1rem;letter-spacing:-.05em}.hero p{color:#b8c9c5;font-size:1.1rem}.eyebrow{color:#6ff598;font-size:.75rem;letter-spacing:.16em;font-weight:800}.layout{display:grid;grid-template-columns:minmax(0,1.25fr) minmax(300px,.75fr);gap:1.25rem;align-items:start}.card{background:#0c1b1a;border:1px solid #24403d;border-radius:18px;padding:clamp(1rem,3vw,2rem);box-shadow:0 20px 80px #0005}.form h2{margin:2rem 0 .8rem}.form h2:first-of-type{margin-top:0}.form>label,.two label{display:grid;gap:.4rem;margin:.8rem 0;color:#cfdbd8}input,select,textarea{width:100%;background:#081311;color:#eff7f4;border:1px solid #36534f;border-radius:9px;padding:.75rem;font:inherit}textarea{resize:vertical}.two{display:grid;grid-template-columns:1fr 1fr;gap:1rem}.checks{display:grid;grid-template-columns:repeat(2,1fr);gap:.7rem;border:0;padding:0}.checks label{display:flex;gap:.55rem;align-items:center;background:#102421;border:1px solid #284743;border-radius:9px;padding:.75rem}.checks input{width:auto}.checks.compact{grid-template-columns:1fr;margin:1rem 0}.primary{margin-top:1rem;width:100%;border:0;border-radius:10px;padding:.9rem 1rem;background:#6ff598;color:#05200d;font-weight:800;cursor:pointer}.fine,.muted{color:#91a5a0;font-size:.9rem}.result{min-height:220px}.history{margin-top:1rem}.quote{border-top:1px solid #284743;padding:1rem 0}.quote:first-child{border-top:0}.quote h3{margin:.2rem 0}.estimate{font-size:1.55rem;color:#b8ffce;font-weight:800}.error{color:#ffb4ad}pre{white-space:pre-wrap;word-break:break-word;background:#071210;border:1px solid #284743;border-radius:10px;padding:1rem;color:#d8e6e2;max-height:600px;overflow:auto}@media(max-width:850px){.layout{grid-template-columns:1fr}.two,.checks{grid-template-columns:1fr}.account span{display:none}}
"#;

#[cfg(test)]
mod tests {
    use super::{format_usd, split_list};

    #[test]
    fn comma_lists_are_trimmed() {
        assert_eq!(split_list(" AWS, Cloudflare ,, Supabase "), vec!["AWS", "Cloudflare", "Supabase"]);
    }

    #[test]
    fn quote_currency_is_readable() {
        assert_eq!(format_usd(125_000), "125,000");
    }
}
