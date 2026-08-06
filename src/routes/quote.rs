use crate::{
    auth::{require_csrf, require_origin, QuoteSessionAuthenticated},
    error::AppError,
    quote_api::{self, QuoteRequest},
    AppState,
};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Redirect, Response},
    Form,
};
use maud::html;
use serde::Deserialize;
use uuid::Uuid;

const QUOTE_RETURN_PATH: &str = "/u/quote";
const SHARED_AUTH_BROWSER_SIGN_IN_PATH: &str = "/shared-auth/auth/browser/sign-in";

pub async fn page(
    State(state): State<AppState>,
    auth: Result<QuoteSessionAuthenticated, AppError>,
) -> Response {
    let actor = match auth {
        Ok(QuoteSessionAuthenticated(actor)) => actor,
        Err(AppError::Unauthorized) => return shared_auth_redirect(&state).into_response(),
        Err(error) => return error.into_response(),
    };
    let Some(client) = state.quote_api.as_ref() else {
        return AppError::ServiceUpstream.into_response();
    };
    match client.list(&actor).await {
        Ok(records) => quote_api::quote_page(&actor, &records).into_response(),
        Err(error) => error.into_response(),
    }
}

pub async fn detail(
    State(state): State<AppState>,
    headers: HeaderMap,
    auth: Result<QuoteSessionAuthenticated, AppError>,
    Path(raw_id): Path<String>,
) -> Response {
    let actor = match auth {
        Ok(QuoteSessionAuthenticated(actor)) => actor,
        Err(AppError::Unauthorized) => return shared_auth_redirect(&state).into_response(),
        Err(error) => return error.into_response(),
    };
    let quote_id = match Uuid::parse_str(&raw_id) {
        Ok(id) => id,
        Err(_) => return AppError::NotFound.into_response(),
    };
    let Some(client) = state.quote_api.as_ref() else {
        return AppError::ServiceUpstream.into_response();
    };
    match client.get(&actor, quote_id).await {
        Ok(record) if headers.contains_key("hx-request") => {
            quote_api::quote_status_fragment(&record).into_response()
        }
        Ok(record) => quote_api::quote_detail_page(&actor, &record).into_response(),
        Err(error) => error.into_response(),
    }
}

pub async fn submit(
    State(state): State<AppState>,
    headers: HeaderMap,
    auth: Result<QuoteSessionAuthenticated, AppError>,
    Form(form): Form<QuoteForm>,
) -> Response {
    let actor = match auth {
        Ok(QuoteSessionAuthenticated(actor)) => actor,
        Err(AppError::Unauthorized) => return htmx_or_browser_auth_redirect(&headers, &state),
        Err(error) => return error.into_response(),
    };
    if let Err(error) = require_origin(&headers, &state) {
        return error.into_response();
    }
    if let Err(error) = require_csrf(&actor, &headers, Some(&form.csrf)) {
        return error.into_response();
    }
    let idempotency_key = form.client_request_id;
    let request = match form.into_request(&actor) {
        Ok(request) => request,
        Err(AppError::BadRequest(message)) => return form_error(&headers, &message),
        Err(error) => return error.into_response(),
    };
    let Some(client) = state.quote_api.as_ref() else {
        return AppError::ServiceUpstream.into_response();
    };
    match client.create(&actor, &request, idempotency_key).await {
        Ok(record) if headers.contains_key("hx-request") => {
            quote_api::quote_status_fragment(&record).into_response()
        }
        Ok(record) => Redirect::to(&format!("/u/quote/{}", record.id)).into_response(),
        Err(AppError::BadRequest(message)) => form_error(&headers, &message),
        Err(error) => error.into_response(),
    }
}

#[derive(Debug, Deserialize)]
pub struct QuoteForm {
    csrf: String,
    client_request_id: Uuid,
    company_name: String,
    contact_name: String,
    #[serde(default)]
    website: String,
    employee_count: u32,
    #[serde(default)]
    annual_revenue_band: String,
    current_stage: String,
    #[serde(default)]
    target_date: String,
    #[serde(default)]
    infrastructure: String,
    #[serde(default)]
    notes: String,
    #[serde(default)]
    soc2_type_2: Option<String>,
    #[serde(default)]
    nist_csf_2: Option<String>,
    #[serde(default)]
    nist_800_53: Option<String>,
    #[serde(default)]
    hipaa: Option<String>,
    #[serde(default)]
    iso_27001: Option<String>,
    #[serde(default)]
    fedramp: Option<String>,
    #[serde(default)]
    pci_dss_4: Option<String>,
    #[serde(default)]
    data_pii: Option<String>,
    #[serde(default)]
    data_phi: Option<String>,
    #[serde(default)]
    data_payment_cards: Option<String>,
    #[serde(default)]
    data_confidential: Option<String>,
    #[serde(default)]
    has_security_program: Option<String>,
    #[serde(default)]
    has_policies: Option<String>,
    #[serde(default)]
    has_risk_assessment: Option<String>,
    #[serde(default)]
    has_incident_response_plan: Option<String>,
    #[serde(default)]
    has_vendor_management: Option<String>,
}

impl QuoteForm {
    fn into_request(self, actor: &canonical_auth::AuthContext) -> Result<QuoteRequest, AppError> {
        let company_name = self.company_name.trim().to_owned();
        if company_name.is_empty() || company_name.chars().count() > 200 {
            return Err(AppError::BadRequest(
                "company name is required and must be at most 200 characters".into(),
            ));
        }
        let contact_name = self.contact_name.trim().to_owned();
        if contact_name.is_empty() || contact_name.chars().count() > 200 {
            return Err(AppError::BadRequest(
                "contact name is required and must be at most 200 characters".into(),
            ));
        }
        if !(1..=1_000_000).contains(&self.employee_count) {
            return Err(AppError::BadRequest(
                "employee count must be between 1 and 1000000".into(),
            ));
        }
        let annual_revenue_band = optional(self.annual_revenue_band);
        if annual_revenue_band.as_deref().is_some_and(|value| {
            !matches!(
                value,
                "under_1m" | "1m_to_10m" | "10m_to_100m" | "100m_plus"
            )
        }) {
            return Err(AppError::BadRequest(
                "choose a supported annual revenue band".into(),
            ));
        }
        let frameworks = [
            ("soc2_type_2", self.soc2_type_2),
            ("nist_csf_2", self.nist_csf_2),
            ("nist_800_53", self.nist_800_53),
            ("hipaa", self.hipaa),
            ("iso_27001", self.iso_27001),
            ("fedramp", self.fedramp),
            ("pci_dss_4", self.pci_dss_4),
        ]
        .into_iter()
        .filter_map(|(name, selected)| selected.map(|_| name.to_owned()))
        .collect::<Vec<_>>();
        if frameworks.is_empty() {
            return Err(AppError::BadRequest(
                "choose at least one supported framework".into(),
            ));
        }
        if !matches!(
            self.current_stage.as_str(),
            "none" | "informal" | "documented" | "managed" | "audited"
        ) {
            return Err(AppError::BadRequest(
                "choose a supported security program maturity".into(),
            ));
        }
        let target_date = optional(self.target_date);
        if target_date
            .as_deref()
            .is_some_and(|value| !is_iso_date(value))
        {
            return Err(AppError::BadRequest(
                "target date must use YYYY-MM-DD".into(),
            ));
        }
        let infrastructure = split_list(&self.infrastructure, 16, 80);
        if infrastructure.is_empty() {
            return Err(AppError::BadRequest(
                "list at least one infrastructure provider or platform".into(),
            ));
        }
        let data_sensitivity = [
            ("pii", self.data_pii),
            ("phi", self.data_phi),
            ("payment_card", self.data_payment_cards),
            ("confidential", self.data_confidential),
        ]
        .into_iter()
        .filter_map(|(name, selected)| selected.map(|_| name.to_owned()))
        .collect::<Vec<_>>();
        if data_sensitivity.is_empty() {
            return Err(AppError::BadRequest(
                "choose at least one data sensitivity category".into(),
            ));
        }
        let website = optional(self.website);
        if website.as_deref().is_some_and(|value| {
            value.len() > 2_048
                || reqwest::Url::parse(value).map_or(true, |url| {
                    !matches!(url.scheme(), "http" | "https") || url.host_str().is_none()
                })
        }) {
            return Err(AppError::BadRequest(
                "website must be an absolute HTTP or HTTPS URL".into(),
            ));
        }
        let notes = optional(self.notes);
        if notes.as_deref().is_some_and(|value| value.len() > 4_000) {
            return Err(AppError::BadRequest(
                "notes must be at most 4000 characters".into(),
            ));
        }

        Ok(QuoteRequest {
            organization_name: company_name,
            contact_name,
            contact_email: actor.email.clone(),
            website,
            employee_count: i64::from(self.employee_count),
            annual_revenue_band,
            frameworks,
            current_stage: self.current_stage,
            infrastructure,
            data_sensitivity,
            target_date,
            has_security_program: self.has_security_program.is_some(),
            has_policies: self.has_policies.is_some(),
            has_risk_assessment: self.has_risk_assessment.is_some(),
            has_incident_response_plan: self.has_incident_response_plan.is_some(),
            has_vendor_management: self.has_vendor_management.is_some(),
            notes,
            context_key: None,
            answers_version: 1,
        })
    }
}

fn is_iso_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
}

fn split_list(value: &str, maximum_entries: usize, maximum_length: usize) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .take(maximum_entries)
        .map(|value| value.chars().take(maximum_length).collect())
        .collect()
}

fn optional(value: String) -> Option<String> {
    let value = value.trim().to_owned();
    (!value.is_empty()).then_some(value)
}

fn shared_auth_sign_in_url(app_base_url: &str) -> reqwest::Url {
    let mut destination = reqwest::Url::parse(app_base_url)
        .expect("APP_BASE_URL was validated before application state construction");
    destination.set_path(SHARED_AUTH_BROWSER_SIGN_IN_PATH);
    destination.set_query(None);
    destination
        .query_pairs_mut()
        .append_pair("client_id", "canonical-web")
        .append_pair("return", QUOTE_RETURN_PATH);
    destination
}

fn shared_auth_redirect(state: &AppState) -> Redirect {
    let destination = shared_auth_sign_in_url(&state.config.app_base_url);
    Redirect::temporary(destination.as_str())
}

fn htmx_or_browser_auth_redirect(headers: &HeaderMap, state: &AppState) -> Response {
    let destination = shared_auth_sign_in_url(&state.config.app_base_url);
    if headers.contains_key("hx-request") {
        let mut response = StatusCode::UNAUTHORIZED.into_response();
        if let Ok(value) = HeaderValue::from_str(destination.as_str()) {
            response.headers_mut().insert("hx-redirect", value);
        }
        response
    } else {
        Redirect::temporary(destination.as_str()).into_response()
    }
}

fn form_error(headers: &HeaderMap, message: &str) -> Response {
    let fragment = html! { p class="error" role="alert" { (message) } };
    if headers.contains_key("hx-request") {
        let mut response = (StatusCode::UNPROCESSABLE_ENTITY, fragment).into_response();
        response
            .headers_mut()
            .insert("hx-retarget", HeaderValue::from_static("#quote-results"));
        response
            .headers_mut()
            .insert("hx-reswap", HeaderValue::from_static("afterbegin"));
        response
    } else {
        (StatusCode::UNPROCESSABLE_ENTITY, fragment).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn form_maps_selected_frameworks_and_scope() {
        let form = QuoteForm {
            csrf: "csrf".into(),
            client_request_id: Uuid::new_v4(),
            company_name: "Example".into(),
            contact_name: "Casey Example".into(),
            website: "https://example.com".into(),
            employee_count: 15,
            annual_revenue_band: "1m_to_10m".into(),
            current_stage: "documented".into(),
            target_date: "2027-01-15".into(),
            infrastructure: "AWS, Cloudflare".into(),
            notes: String::new(),
            soc2_type_2: Some("on".into()),
            nist_csf_2: None,
            nist_800_53: None,
            hipaa: Some("on".into()),
            iso_27001: None,
            fedramp: None,
            pci_dss_4: None,
            data_pii: None,
            data_phi: Some("on".into()),
            data_payment_cards: None,
            data_confidential: None,
            has_security_program: Some("on".into()),
            has_policies: Some("on".into()),
            has_risk_assessment: None,
            has_incident_response_plan: Some("on".into()),
            has_vendor_management: None,
        };
        let actor = canonical_auth::AuthContext {
            user_id: Uuid::new_v4(),
            email: "casey@example.com".into(),
            source: canonical_auth::CredentialSource::SessionCookie,
            supabase_session_id: None,
            session_hash: None,
            csrf_token: Some("csrf".into()),
            expires_at: chrono::Utc::now(),
        };
        let request = form.into_request(&actor).unwrap();
        assert_eq!(request.frameworks, ["soc2_type_2", "hipaa"]);
        assert_eq!(request.infrastructure, ["AWS", "Cloudflare"]);
        assert_eq!(request.data_sensitivity, ["phi"]);
        assert_eq!(request.contact_email, "casey@example.com");
        assert!(request.has_security_program);
    }

    #[test]
    fn auth_return_target_is_same_origin_and_relative() {
        let destination = shared_auth_sign_in_url("https://app.canonical.plus");
        assert_eq!(destination.host_str(), Some("app.canonical.plus"));
        assert_eq!(destination.path(), SHARED_AUTH_BROWSER_SIGN_IN_PATH);
        assert_eq!(
            destination
                .query_pairs()
                .find(|(name, _)| name == "return")
                .map(|(_, value)| value.into_owned()),
            Some(QUOTE_RETURN_PATH.into())
        );
        assert_eq!(
            destination
                .query_pairs()
                .find(|(name, _)| name == "client_id")
                .map(|(_, value)| value.into_owned()),
            Some("canonical-web".into())
        );
    }
}
