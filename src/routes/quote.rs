use crate::{
    auth::{require_csrf, require_origin, QuoteSessionAuthenticated},
    error::AppError,
    quote_api::{self, QuoteRequest},
    AppState,
};
use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Redirect, Response},
    Form,
};
use maud::html;
use serde::Deserialize;
use uuid::Uuid;

const QUOTE_RETURN_PATH: &str = "/u/readiness";
const SHARED_AUTH_BROWSER_SIGN_IN_PATH: &str = "/shared-auth/auth/browser/sign-in";

pub async fn page(
    State(state): State<AppState>,
    headers: HeaderMap,
    auth: Result<QuoteSessionAuthenticated, AppError>,
) -> Response {
    let actor = match auth {
        Ok(QuoteSessionAuthenticated(actor)) => actor,
        Err(AppError::Unauthorized) => return htmx_or_browser_auth_redirect(&headers, &state),
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
        Err(AppError::Unauthorized) => return htmx_or_browser_auth_redirect(&headers, &state),
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
        Ok(record) if is_htmx_request(&headers) => {
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
    let request = match form.into_request(&actor.email) {
        Ok(request) => request,
        Err(AppError::BadRequest(message)) => return form_error(&headers, &message),
        Err(error) => return error.into_response(),
    };
    let Some(client) = state.quote_api.as_ref() else {
        return AppError::ServiceUpstream.into_response();
    };
    match client.create(&actor, &request, idempotency_key).await {
        Ok(record) if is_htmx_request(&headers) => {
            quote_api::quote_status_fragment(&record).into_response()
        }
        Ok(record) => Redirect::to(&format!("/u/readiness/{}", record.id)).into_response(),
        Err(AppError::BadRequest(message)) => form_error(&headers, &message),
        Err(error) => error.into_response(),
    }
}

#[derive(Debug, Deserialize)]
pub struct QuoteForm {
    csrf: String,
    client_request_id: Uuid,
    organization_name: String,
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
    notes: String,

    #[serde(default)]
    soc2_type_1: Option<String>,
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
    pci_dss_4: Option<String>,
    #[serde(default)]
    fedramp: Option<String>,
    #[serde(default)]
    gdpr: Option<String>,
    #[serde(default)]
    custom: Option<String>,

    #[serde(default)]
    infra_aws: Option<String>,
    #[serde(default)]
    infra_azure: Option<String>,
    #[serde(default)]
    infra_gcp: Option<String>,
    #[serde(default)]
    infra_supabase: Option<String>,
    #[serde(default)]
    infra_on_prem: Option<String>,
    #[serde(default)]
    infra_colocation: Option<String>,
    #[serde(default)]
    infra_saas_only: Option<String>,
    #[serde(default)]
    infra_multi_cloud: Option<String>,
    #[serde(default)]
    infra_other: Option<String>,

    #[serde(default)]
    data_public: Option<String>,
    #[serde(default)]
    data_internal: Option<String>,
    #[serde(default)]
    data_confidential: Option<String>,
    #[serde(default)]
    data_pii: Option<String>,
    #[serde(default)]
    data_phi: Option<String>,
    #[serde(default)]
    data_pci: Option<String>,
    #[serde(default)]
    data_government_cui: Option<String>,
    #[serde(default)]
    data_customer_secrets: Option<String>,
    #[serde(default)]
    data_other: Option<String>,

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
    fn into_request(self, verified_email: &str) -> Result<QuoteRequest, AppError> {
        let organization_name = bounded_required(self.organization_name, 200, "organization name")?;
        let contact_name = bounded_required(self.contact_name, 160, "contact name")?;
        let contact_email = verified_email.trim().to_owned();
        if contact_email.is_empty()
            || contact_email.chars().count() > 320
            || !contact_email.contains('@')
        {
            return Err(AppError::BadRequest(
                "the signed-in account does not have a valid contact email".into(),
            ));
        }
        if !(1..=1_000_000).contains(&self.employee_count) {
            return Err(AppError::BadRequest(
                "employee count must be between 1 and 1000000".into(),
            ));
        }

        let website = validated_website(self.website)?;
        let annual_revenue_band = optional_enum(
            self.annual_revenue_band,
            &[
                "pre_revenue",
                "under_1m",
                "1m_10m",
                "10m_50m",
                "50m_250m",
                "over_250m",
                "prefer_not_to_say",
            ],
            "annual revenue band",
        )?;

        let frameworks = selected_values([
            ("soc2_type_1", self.soc2_type_1),
            ("soc2_type_2", self.soc2_type_2),
            ("nist_csf_2", self.nist_csf_2),
            ("nist_800_53", self.nist_800_53),
            ("hipaa", self.hipaa),
            ("iso_27001", self.iso_27001),
            ("pci_dss_4", self.pci_dss_4),
            ("fedramp", self.fedramp),
            ("gdpr", self.gdpr),
            ("custom", self.custom),
        ]);
        if frameworks.is_empty() {
            return Err(AppError::BadRequest(
                "choose at least one supported framework".into(),
            ));
        }

        let current_stage = required_enum(
            self.current_stage,
            &[
                "exploring",
                "readiness",
                "remediation",
                "audit_ready",
                "renewal",
            ],
            "current stage",
        )?;

        let infrastructure = selected_values([
            ("aws", self.infra_aws),
            ("azure", self.infra_azure),
            ("gcp", self.infra_gcp),
            ("supabase", self.infra_supabase),
            ("on_prem", self.infra_on_prem),
            ("colocation", self.infra_colocation),
            ("saas_only", self.infra_saas_only),
            ("multi_cloud", self.infra_multi_cloud),
            ("other", self.infra_other),
        ]);
        if infrastructure.is_empty() {
            return Err(AppError::BadRequest(
                "choose at least one infrastructure category".into(),
            ));
        }

        let data_sensitivity = selected_values([
            ("public", self.data_public),
            ("internal", self.data_internal),
            ("confidential", self.data_confidential),
            ("pii", self.data_pii),
            ("phi", self.data_phi),
            ("pci", self.data_pci),
            ("government_cui", self.data_government_cui),
            ("customer_secrets", self.data_customer_secrets),
            ("other", self.data_other),
        ]);
        if data_sensitivity.is_empty() {
            return Err(AppError::BadRequest(
                "choose at least one data-sensitivity category".into(),
            ));
        }

        let target_date = optional(self.target_date);
        if target_date
            .as_deref()
            .is_some_and(|value| chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").is_err())
        {
            return Err(AppError::BadRequest(
                "target date must be a valid YYYY-MM-DD date".into(),
            ));
        }

        let notes = optional(self.notes);
        if notes
            .as_deref()
            .is_some_and(|value| value.chars().count() > 5_000)
        {
            return Err(AppError::BadRequest(
                "notes must be at most 5000 characters".into(),
            ));
        }

        Ok(QuoteRequest {
            organization_name,
            contact_name,
            contact_email,
            website,
            employee_count: i64::from(self.employee_count),
            annual_revenue_band,
            frameworks,
            current_stage,
            infrastructure,
            data_sensitivity,
            target_date,
            has_security_program: self.has_security_program.is_some(),
            has_policies: self.has_policies.is_some(),
            has_risk_assessment: self.has_risk_assessment.is_some(),
            has_incident_response_plan: self.has_incident_response_plan.is_some(),
            has_vendor_management: self.has_vendor_management.is_some(),
            notes,
            context_key: Some("quote-analysis".into()),
            answers_version: 1,
        })
    }
}

fn bounded_required(value: String, maximum: usize, label: &str) -> Result<String, AppError> {
    let value = value.trim().to_owned();
    if value.is_empty() || value.chars().count() > maximum {
        return Err(AppError::BadRequest(format!(
            "{label} is required and must be at most {maximum} characters"
        )));
    }
    Ok(value)
}

fn selected_values<const N: usize>(values: [(&str, Option<String>); N]) -> Vec<String> {
    values
        .into_iter()
        .filter_map(|(value, selected)| selected.map(|_| value.to_owned()))
        .collect()
}

fn required_enum(value: String, allowed: &[&str], label: &str) -> Result<String, AppError> {
    let value = value.trim().to_owned();
    if allowed.contains(&value.as_str()) {
        Ok(value)
    } else {
        Err(AppError::BadRequest(format!("choose a supported {label}")))
    }
}

fn optional_enum(value: String, allowed: &[&str], label: &str) -> Result<Option<String>, AppError> {
    let Some(value) = optional(value) else {
        return Ok(None);
    };
    if allowed.contains(&value.as_str()) {
        Ok(Some(value))
    } else {
        Err(AppError::BadRequest(format!("choose a supported {label}")))
    }
}

fn validated_website(value: String) -> Result<Option<String>, AppError> {
    let Some(value) = optional(value) else {
        return Ok(None);
    };
    if value.chars().count() > 2_048 {
        return Err(AppError::BadRequest(
            "website must be at most 2048 characters".into(),
        ));
    }
    let url = reqwest::Url::parse(&value)
        .map_err(|_| AppError::BadRequest("website must be an absolute HTTP(S) URL".into()))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(AppError::BadRequest(
            "website must be an absolute HTTP(S) URL without credentials".into(),
        ));
    }
    Ok(Some(value))
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
    destination.set_fragment(None);
    destination
        .query_pairs_mut()
        .append_pair("client_id", "canonical-web")
        .append_pair("return", QUOTE_RETURN_PATH);
    destination
}

// HX-Request selects a response representation only; authentication, Origin,
// CSRF, and account ownership remain separate mandatory checks.
fn is_htmx_request(headers: &HeaderMap) -> bool {
    let values = headers.get_all("hx-request");
    let mut values = values.iter();
    let enabled = values.next().is_some_and(|value| value == "true");
    enabled && values.next().is_none()
}

fn htmx_or_browser_auth_redirect(headers: &HeaderMap, state: &AppState) -> Response {
    auth_redirect_response(headers, &state.config.app_base_url)
}

fn auth_redirect_response(headers: &HeaderMap, app_base_url: &str) -> Response {
    let destination = shared_auth_sign_in_url(app_base_url);
    let mut response = if is_htmx_request(headers) {
        // HTMX cannot process HX-Redirect on an ordinary 3xx response: the
        // browser follows it first. Keep the denial and navigate the full page,
        // rather than inserting a sign-in document into the quote fragment.
        let mut response = StatusCode::UNAUTHORIZED.into_response();
        if let Ok(value) = HeaderValue::from_str(destination.as_str()) {
            response.headers_mut().insert("hx-redirect", value);
        }
        response
            .headers_mut()
            .insert("hx-reswap", HeaderValue::from_static("none"));
        response
    } else {
        // A 303 deliberately changes an expired form submission to GET. A 307
        // would replay the quote's POST body into the sign-in endpoint.
        Redirect::to(destination.as_str()).into_response()
    };
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn form_error(headers: &HeaderMap, message: &str) -> Response {
    let fragment = html! { p class="error" role="alert" { (message) } };
    if is_htmx_request(headers) {
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

    fn fixture_form() -> QuoteForm {
        QuoteForm {
            csrf: "csrf".into(),
            client_request_id: Uuid::new_v4(),
            organization_name: "Example Company".into(),
            contact_name: "Taylor Example".into(),
            website: "https://example.com".into(),
            employee_count: 120,
            annual_revenue_band: "10m_50m".into(),
            current_stage: "readiness".into(),
            target_date: "2026-12-31".into(),
            notes: "Planning fixture only; contains no secrets or regulated records.".into(),
            soc2_type_1: None,
            soc2_type_2: Some("on".into()),
            nist_csf_2: Some("on".into()),
            nist_800_53: Some("on".into()),
            hipaa: Some("on".into()),
            iso_27001: None,
            pci_dss_4: None,
            fedramp: None,
            gdpr: None,
            custom: None,
            infra_aws: Some("on".into()),
            infra_azure: None,
            infra_gcp: None,
            infra_supabase: None,
            infra_on_prem: None,
            infra_colocation: None,
            infra_saas_only: Some("on".into()),
            infra_multi_cloud: None,
            infra_other: None,
            data_public: None,
            data_internal: None,
            data_confidential: Some("on".into()),
            data_pii: Some("on".into()),
            data_phi: Some("on".into()),
            data_pci: None,
            data_government_cui: None,
            data_customer_secrets: None,
            data_other: None,
            has_security_program: Some("on".into()),
            has_policies: Some("on".into()),
            has_risk_assessment: None,
            has_incident_response_plan: Some("on".into()),
            has_vendor_management: None,
        }
    }

    #[test]
    fn form_maps_exactly_to_the_canonical_golden_request() {
        let request = fixture_form().into_request("security@example.com").unwrap();
        let expected: serde_json::Value =
            serde_json::from_str(include_str!("../../fixtures/quote/v1/request.json")).unwrap();
        assert_eq!(serde_json::to_value(request).unwrap(), expected);
    }

    #[test]
    fn browser_form_cannot_select_a_database_context() {
        let request = fixture_form().into_request("security@example.com").unwrap();
        assert_eq!(request.context_key.as_deref(), Some("quote-analysis"));
        let value = serde_json::to_value(request).unwrap();
        assert!(value.get("contextRecordId").is_none());
        assert!(value.get("markdownContext").is_none());
        assert!(value.get("userId").is_none());
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

    #[test]
    fn expired_browser_submission_redirects_with_see_other_not_body_replay() {
        let response = auth_redirect_response(&HeaderMap::new(), "https://app.canonical.plus");
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let location = response.headers()[header::LOCATION].to_str().unwrap();
        assert_eq!(
            location,
            shared_auth_sign_in_url("https://app.canonical.plus").as_str()
        );
        assert!(!response.headers().contains_key("hx-redirect"));
        assert!(!response.headers().contains_key("hx-reswap"));
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    }

    #[test]
    fn expired_htmx_request_denies_and_navigates_without_swapping_login_html() {
        let mut headers = HeaderMap::new();
        headers.insert("hx-request", HeaderValue::from_static("true"));
        let response = auth_redirect_response(&headers, "https://app.canonical.plus");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            response.headers()["hx-redirect"],
            shared_auth_sign_in_url("https://app.canonical.plus").as_str()
        );
        assert_eq!(response.headers()["hx-reswap"], "none");
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert!(!response.headers().contains_key(header::LOCATION));
    }

    #[test]
    fn htmx_representation_requires_one_exact_true_value() {
        for value in ["false", "", "TRUE", " true ", "true,false"] {
            let mut headers = HeaderMap::new();
            headers.insert("hx-request", HeaderValue::from_str(value).unwrap());
            assert!(!is_htmx_request(&headers));
            assert_eq!(
                auth_redirect_response(&headers, "https://app.canonical.plus").status(),
                StatusCode::SEE_OTHER
            );
        }
        let mut headers = HeaderMap::new();
        headers.append("hx-request", HeaderValue::from_static("true"));
        headers.append("hx-request", HeaderValue::from_static("true"));
        assert!(!is_htmx_request(&headers));
    }

    #[test]
    fn caller_headers_cannot_choose_the_sign_in_origin_or_return_path() {
        let mut headers = HeaderMap::new();
        headers.insert("hx-request", HeaderValue::from_static("true"));
        for name in [
            "host",
            "x-forwarded-host",
            "hx-current-url",
            "hx-target",
            "referer",
        ] {
            headers.insert(
                name,
                HeaderValue::from_static("https://evil.invalid/?code=synthetic"),
            );
        }
        let response = auth_redirect_response(&headers, "https://app.canonical.plus");
        let value = response.headers()["hx-redirect"].to_str().unwrap();
        assert_eq!(
            value,
            shared_auth_sign_in_url("https://app.canonical.plus").as_str()
        );
        assert!(!value.contains("evil.invalid"));
        assert!(!value.contains("synthetic"));
    }

    #[test]
    fn sign_in_builder_removes_unrelated_query_and_fragment() {
        let destination =
            shared_auth_sign_in_url("https://app.canonical.plus/old?unrelated=synthetic#synthetic");
        assert_eq!(destination.fragment(), None);
        assert_eq!(destination.query_pairs().count(), 2);
        assert!(!destination.as_str().contains("synthetic"));
        assert_eq!(destination.path(), SHARED_AUTH_BROWSER_SIGN_IN_PATH);
    }

    #[tokio::test]
    async fn expired_session_responses_never_echo_customer_content() {
        for htmx in [false, true] {
            let mut headers = HeaderMap::new();
            headers.insert(
                header::COOKIE,
                HeaderValue::from_static("session=synthetic"),
            );
            if htmx {
                headers.insert("hx-request", HeaderValue::from_static("true"));
            }
            let response = auth_redirect_response(&headers, "https://app.canonical.plus");
            assert!(!response.headers().contains_key(header::SET_COOKIE));
            let body = axum::body::to_bytes(response.into_body(), 1024)
                .await
                .unwrap();
            assert!(body.is_empty());
        }
    }
}
