use axum::{
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Form, Router,
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::{
    auth::require_origin,
    edge::{EdgeAuthenticated, EdgeIdentity},
    error::AppError,
    views, AppState,
};

const API_PATH: &str = "/v1/quotes";
const MATURITY_VALUES: &[&str] = &["none", "informal", "documented", "managed", "audited"];
const TIMELINE_VALUES: &[&str] = &[
    "under_3_months",
    "3_to_6_months",
    "6_to_12_months",
    "over_12_months",
    "unsure",
];

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/quote", get(quote_page).post(submit_quote))
        .route("/quote/{quote_id}", get(quote_status))
}

async fn quote_page(EdgeAuthenticated(identity): EdgeAuthenticated) -> Response {
    views::quote_page(&identity.email).into_response()
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct QuoteForm {
    company_name: String,
    industry: String,
    employee_count: u32,
    #[serde(default)]
    annual_revenue_usd: String,
    security_program_maturity: String,
    target_timeline: String,
    #[serde(default)]
    cloud_providers: String,
    #[serde(default)]
    existing_certifications: String,
    #[serde(default)]
    notes: String,
    #[serde(default)]
    handles_phi: Option<String>,
    #[serde(default)]
    handles_payment_cards: Option<String>,
    #[serde(default)]
    soc2: Option<String>,
    #[serde(default)]
    nist_csf: Option<String>,
    #[serde(default)]
    nist_800_53: Option<String>,
    #[serde(default)]
    hipaa: Option<String>,
    #[serde(default)]
    iso_27001: Option<String>,
    #[serde(default)]
    pci_dss: Option<String>,
    #[serde(default)]
    fedramp: Option<String>,
}

impl QuoteForm {
    fn frameworks(&self) -> Vec<String> {
        [
            (&self.soc2, "soc2"),
            (&self.nist_csf, "nist_csf"),
            (&self.nist_800_53, "nist_800_53"),
            (&self.hipaa, "hipaa"),
            (&self.iso_27001, "iso_27001"),
            (&self.pci_dss, "pci_dss"),
            (&self.fedramp, "fedramp"),
        ]
        .into_iter()
        .filter(|(selected, _)| selected.as_deref() == Some("on"))
        .map(|(_, framework)| framework.to_owned())
        .collect()
    }

    fn validate(&self) -> Result<Option<u64>, &'static str> {
        if self.company_name.trim().is_empty() || self.company_name.chars().count() > 200 {
            return Err("Company name is required and must be at most 200 characters.");
        }
        if self.industry.trim().is_empty() || self.industry.chars().count() > 120 {
            return Err("Industry is required and must be at most 120 characters.");
        }
        if !(1..=1_000_000).contains(&self.employee_count) {
            return Err("Employee count must be between 1 and 1,000,000.");
        }
        if self.frameworks().is_empty() {
            return Err("Choose at least one compliance framework.");
        }
        if !MATURITY_VALUES.contains(&self.security_program_maturity.as_str()) {
            return Err("Choose a supported security program maturity.");
        }
        if !TIMELINE_VALUES.contains(&self.target_timeline.as_str()) {
            return Err("Choose a supported target timeline.");
        }
        if self.notes.chars().count() > 4_000 {
            return Err("Notes must be at most 4000 characters.");
        }
        let revenue = self.annual_revenue_usd.trim();
        if revenue.is_empty() {
            return Ok(None);
        }
        revenue
            .parse::<u64>()
            .ok()
            .filter(|value| *value <= 10_000_000_000_000)
            .map(Some)
            .ok_or("Annual revenue must be a valid USD amount.")
    }
}

async fn submit_quote(
    State(state): State<AppState>,
    headers: HeaderMap,
    EdgeAuthenticated(identity): EdgeAuthenticated,
    Form(form): Form<QuoteForm>,
) -> Result<Response, AppError> {
    require_origin(&headers, &state)?;
    let annual_revenue_usd = match form.validate() {
        Ok(value) => value,
        Err(message) => return Ok(quote_error(&headers, message)),
    };
    let quote = state
        .quote
        .as_ref()
        .ok_or(AppError::Configuration("quote client is not initialized"))?;
    let payload = json!({
        "companyName": form.company_name.trim(),
        "industry": form.industry.trim(),
        "employeeCount": form.employee_count,
        "annualRevenueUsd": annual_revenue_usd,
        "frameworks": form.frameworks(),
        "cloudProviders": split_list(&form.cloud_providers, 8, 80),
        "handlesPhi": form.handles_phi.as_deref() == Some("on"),
        "handlesPaymentCards": form.handles_payment_cards.as_deref() == Some("on"),
        "securityProgramMaturity": form.security_program_maturity,
        "targetTimeline": form.target_timeline,
        "existingCertifications": split_list(&form.existing_certifications, 16, 120),
        "notes": nonempty(form.notes.trim()),
    });
    let response = quote
        .http
        .post(format!("{}{}", quote.base_url, API_PATH))
        .headers(service_headers(&identity, &quote.web_service_token)?)
        .json(&payload)
        .send()
        .await?;
    if response.status() != StatusCode::ACCEPTED {
        return Ok(api_error(&headers, response.status()));
    }
    let quote = response.json::<serde_json::Value>().await?;
    Ok(views::quote_result(&quote).into_response())
}

async fn quote_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    EdgeAuthenticated(identity): EdgeAuthenticated,
    Path(quote_id): Path<Uuid>,
) -> Result<Response, AppError> {
    let quote = state
        .quote
        .as_ref()
        .ok_or(AppError::Configuration("quote client is not initialized"))?;
    let response = quote
        .http
        .get(format!("{}{}/{}", quote.base_url, API_PATH, quote_id))
        .headers(service_headers(&identity, &quote.web_service_token)?)
        .send()
        .await?;
    if !response.status().is_success() {
        return Ok(api_error(&headers, response.status()));
    }
    let quote = response.json::<serde_json::Value>().await?;
    Ok(views::quote_result(&quote).into_response())
}

fn service_headers(identity: &EdgeIdentity, token: &str) -> Result<HeaderMap, AppError> {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-canonical-service-token",
        HeaderValue::from_str(token).map_err(|_| AppError::Crypto)?,
    );
    headers.insert(
        "x-canonical-user-id",
        HeaderValue::from_str(&identity.user_id.to_string()).map_err(|_| AppError::Crypto)?,
    );
    if !identity.email.is_empty() {
        headers.insert(
            "x-canonical-user-email",
            HeaderValue::from_str(&identity.email).map_err(|_| AppError::Crypto)?,
        );
    }
    Ok(headers)
}

fn split_list(value: &str, maximum_entries: usize, maximum_length: usize) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .take(maximum_entries)
        .map(|item| item.chars().take(maximum_length).collect())
        .collect()
}

fn nonempty(value: &str) -> Option<&str> {
    (!value.is_empty()).then_some(value)
}

fn api_error(headers: &HeaderMap, status: StatusCode) -> Response {
    tracing::warn!(%status, "quote API rejected the request");
    let message = if matches!(status, StatusCode::UNPROCESSABLE_ENTITY | StatusCode::BAD_REQUEST) {
        "Review the form fields and try again."
    } else {
        "Quote analysis is temporarily unavailable. Please try again."
    };
    quote_error(headers, message)
}

fn quote_error(headers: &HeaderMap, message: &str) -> Response {
    if headers.contains_key("hx-request") {
        views::quote_error(message).into_response()
    } else {
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            views::quote_error(message),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::{split_list, QuoteForm};

    #[test]
    fn free_text_lists_are_bounded() {
        let values = split_list("AWS, GCP, , Azure", 8, 80);
        assert_eq!(values, ["AWS", "GCP", "Azure"]);
    }

    #[test]
    fn a_framework_is_required() {
        let form = QuoteForm {
            company_name: "Canonical Example".into(),
            industry: "Software".into(),
            employee_count: 51,
            annual_revenue_usd: String::new(),
            security_program_maturity: "documented".into(),
            target_timeline: "3_to_6_months".into(),
            cloud_providers: String::new(),
            existing_certifications: String::new(),
            notes: String::new(),
            handles_phi: None,
            handles_payment_cards: None,
            soc2: None,
            nist_csf: None,
            nist_800_53: None,
            hipaa: None,
            iso_27001: None,
            pci_dss: None,
            fedramp: None,
        };
        assert_eq!(
            form.validate(),
            Err("Choose at least one compliance framework.")
        );
    }
}
