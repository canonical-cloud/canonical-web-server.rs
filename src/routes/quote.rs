use crate::{
    auth::{
        require_csrf, require_origin, AuthContext, QuoteSessionAuthenticated, VerifiedContacts,
    },
    error::AppError,
    quote_api::{self, QuoteRequest},
    AppState,
};
use axum::{
    extract::{OriginalUri, Path, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Redirect, Response},
    Form,
};
use maud::html;
use serde::Deserialize;
use uuid::Uuid;

const PUBLIC_QUOTE_PATH: &str = "/quote";
const SIGNED_IN_QUOTE_PATH: &str = "/u/quote";
const SHARED_AUTH_BROWSER_SIGN_IN_PATH: &str = "/shared-auth/auth/browser/sign-in";

pub async fn page(
    State(state): State<AppState>,
    uri: OriginalUri,
    headers: HeaderMap,
    auth: Result<QuoteSessionAuthenticated, AppError>,
) -> Response {
    let form_action = quote_root(uri.0.path());
    let actor = match auth {
        Ok(QuoteSessionAuthenticated(actor)) => actor,
        Err(AppError::Unauthorized) => {
            return shared_auth_redirect(&state, form_action).into_response()
        }
        Err(error) => return error.into_response(),
    };
    let contacts = match verified_contacts_for_actor(&state, &headers, &actor).await {
        Ok(contacts) => contacts,
        Err(AppError::Unauthorized) => {
            return shared_auth_redirect(&state, form_action).into_response()
        }
        Err(error) => return error.into_response(),
    };
    let Some(client) = state.quote_api.as_ref() else {
        return AppError::ServiceUpstream.into_response();
    };
    match client.list(&actor).await {
        Ok(records) => {
            quote_api::quote_page(&actor, &contacts, &records, form_action).into_response()
        }
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
        Err(AppError::Unauthorized) => {
            return shared_auth_redirect(&state, SIGNED_IN_QUOTE_PATH).into_response()
        }
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
        Ok(record) => quote_api::quote_detail_page(
            &format!("Verified account: {}", actor.email),
            actor.csrf_token.as_deref().unwrap_or_default(),
            &record,
            &format!("/u/quote/{quote_id}/submissions"),
        )
        .into_response(),
        Err(error) => error.into_response(),
    }
}

pub async fn submit(
    State(state): State<AppState>,
    uri: OriginalUri,
    headers: HeaderMap,
    auth: Result<QuoteSessionAuthenticated, AppError>,
    Form(form): Form<QuoteForm>,
) -> Response {
    let form_action = quote_root(uri.0.path());
    let actor = match auth {
        Ok(QuoteSessionAuthenticated(actor)) => actor,
        Err(AppError::Unauthorized) => {
            return htmx_or_browser_auth_redirect(&headers, &state, form_action)
        }
        Err(error) => return error.into_response(),
    };
    if let Err(error) = require_origin(&headers, &state) {
        return error.into_response();
    }
    if let Err(error) = require_csrf(&actor, &headers, Some(&form.csrf)) {
        return error.into_response();
    }
    if form.confirm_email.is_none() || form.confirm_phone.is_none() {
        return form_error(
            &headers,
            "click both contact confirmation controls before submitting",
        );
    }
    let contacts = match verified_contacts_for_actor(&state, &headers, &actor).await {
        Ok(contacts) if contacts.email.is_some() && contacts.phone.is_some() => contacts,
        Ok(_) => {
            return form_error(
                &headers,
                "verify both your email and phone before submitting",
            )
        }
        Err(error) => return error.into_response(),
    };
    let Some(client) = state.quote_api.as_ref() else {
        return AppError::ServiceUpstream.into_response();
    };
    let selection = match client.create_contact_selection(&actor, &contacts).await {
        Ok(selection) => selection,
        Err(AppError::BadRequest(message)) => return form_error(&headers, &message),
        Err(error) => return error.into_response(),
    };
    let request = match form.into_request(selection.contact_selection_id) {
        Ok(request) => request,
        Err(AppError::BadRequest(message)) => return form_error(&headers, &message),
        Err(error) => return error.into_response(),
    };
    match client.create(&actor, &request).await {
        Ok(submission) if headers.contains_key("hx-request") => {
            match client.get(&actor, submission.quote_id).await {
                Ok(record) => quote_api::quote_status_fragment(&record).into_response(),
                Err(error) => error.into_response(),
            }
        }
        Ok(submission) => Redirect::to(&format!(
            "{}/{quote_id}",
            form_action,
            quote_id = submission.quote_id
        ))
        .into_response(),
        Err(AppError::BadRequest(message)) => form_error(&headers, &message),
        Err(error) => error.into_response(),
    }
}

pub async fn request_phone(
    State(state): State<AppState>,
    uri: OriginalUri,
    headers: HeaderMap,
    auth: Result<QuoteSessionAuthenticated, AppError>,
    Form(form): Form<PhoneRequestForm>,
) -> Response {
    let return_path = quote_root(uri.0.path());
    let actor = match auth {
        Ok(QuoteSessionAuthenticated(actor)) => actor,
        Err(error) => return error.into_response(),
    };
    if let Err(error) = require_origin(&headers, &state) {
        return error.into_response();
    }
    if let Err(error) = require_csrf(&actor, &headers, Some(&form.csrf)) {
        return error.into_response();
    }
    let token = match shared_auth_token_for_actor(&state, &headers, &actor).await {
        Ok(token) => token,
        Err(error) => return error.into_response(),
    };
    let phone = form.phone.trim();
    if !valid_e164(phone) {
        return form_error(
            &headers,
            "phone must use E.164 format, such as +14155550100",
        );
    }
    match state
        .shared_auth
        .request_phone_verification(&token, phone)
        .await
    {
        Ok(challenge) => quote_api::phone_challenge_fragment(
            actor.csrf_token.as_deref().unwrap_or_default(),
            return_path,
            challenge.challenge_id,
            &challenge.phone_hint,
        )
        .into_response(),
        Err(AppError::AuthBusy) => AppError::RateLimited {
            retry_after_seconds: 60,
        }
        .into_response(),
        Err(error) => error.into_response(),
    }
}

pub async fn verify_phone(
    State(state): State<AppState>,
    uri: OriginalUri,
    headers: HeaderMap,
    auth: Result<QuoteSessionAuthenticated, AppError>,
    Form(form): Form<PhoneVerifyForm>,
) -> Response {
    let return_path = quote_root(uri.0.path());
    let actor = match auth {
        Ok(QuoteSessionAuthenticated(actor)) => actor,
        Err(error) => return error.into_response(),
    };
    if let Err(error) = require_origin(&headers, &state) {
        return error.into_response();
    }
    if let Err(error) = require_csrf(&actor, &headers, Some(&form.csrf)) {
        return error.into_response();
    }
    let code = form.code.trim();
    if !(4..=10).contains(&code.len()) || !code.bytes().all(|byte| byte.is_ascii_digit()) {
        return form_error(
            &headers,
            "enter the numeric verification code from the text message",
        );
    }
    let token = match shared_auth_token_for_actor(&state, &headers, &actor).await {
        Ok(token) => token,
        Err(error) => return error.into_response(),
    };
    match state
        .shared_auth
        .verify_phone(&token, form.challenge_id, code)
        .await
    {
        Ok(()) if headers.contains_key("hx-request") => {
            let mut response = quote_api::phone_verified_fragment(return_path).into_response();
            if let Ok(value) = HeaderValue::from_str(return_path) {
                response.headers_mut().insert("hx-redirect", value);
            }
            response
        }
        Ok(()) => Redirect::to(return_path).into_response(),
        Err(AppError::Unauthorized) => {
            form_error(&headers, "the verification code was invalid or expired")
        }
        Err(error) => error.into_response(),
    }
}

pub async fn resubmit(
    State(state): State<AppState>,
    headers: HeaderMap,
    auth: Result<QuoteSessionAuthenticated, AppError>,
    Path(quote_id): Path<Uuid>,
    Form(form): Form<EditQuoteForm>,
) -> Response {
    let actor = match auth {
        Ok(QuoteSessionAuthenticated(actor)) => actor,
        Err(error) => return error.into_response(),
    };
    if let Err(error) = require_origin(&headers, &state) {
        return error.into_response();
    }
    if let Err(error) = require_csrf(&actor, &headers, Some(&form.csrf)) {
        return error.into_response();
    }
    let request = match form.into_request() {
        Ok(request) => request,
        Err(error) => return error.into_response(),
    };
    let Some(client) = state.quote_api.as_ref() else {
        return AppError::ServiceUpstream.into_response();
    };
    match client.resubmit(actor.user_id, quote_id, &request).await {
        Ok(_) => Redirect::to(&format!("/u/quote/{quote_id}")).into_response(),
        Err(error) => error.into_response(),
    }
}

pub async fn redeem_link(
    State(state): State<AppState>,
    Path(capability): Path<String>,
) -> Response {
    if capability.len() != 43
        || !capability
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return AppError::NotFound.into_response();
    }
    let Some(client) = state.quote_api.as_ref() else {
        return AppError::ServiceUpstream.into_response();
    };
    let redeemed = match client.redeem(&capability).await {
        Ok(redeemed) => redeemed,
        Err(error) => return error.into_response(),
    };
    let (cookie, _) = match state.quote_grants.issue(
        redeemed.owner_subject,
        redeemed.quote_id,
        &redeemed.expires_at,
    ) {
        Ok(result) => result,
        Err(error) => return error.into_response(),
    };
    let mut response = Redirect::to(&format!("/quote/{}", redeemed.quote_id)).into_response();
    response.headers_mut().insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    match HeaderValue::from_str(&cookie.to_string()) {
        Ok(value) => {
            response.headers_mut().append(header::SET_COOKIE, value);
            response
        }
        Err(_) => AppError::Crypto.into_response(),
    }
}

pub async fn link_detail(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(quote_id): Path<Uuid>,
) -> Response {
    let grant = match state.quote_grants.authenticate(&headers, quote_id) {
        Ok(grant) => grant,
        Err(error) => return error.into_response(),
    };
    let Some(client) = state.quote_api.as_ref() else {
        return AppError::ServiceUpstream.into_response();
    };
    match client.get_for_subject(grant.owner_subject, quote_id).await {
        Ok(record) if headers.contains_key("hx-request") => {
            quote_api::quote_status_fragment_at(&record, &format!("/quote/{quote_id}"))
                .into_response()
        }
        Ok(record) => quote_api::quote_detail_page(
            "Private quote link · access is limited to this quote",
            &grant.csrf_token,
            &record,
            &format!("/quote/{quote_id}/submissions"),
        )
        .into_response(),
        Err(error) => error.into_response(),
    }
}

pub async fn link_resubmit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(quote_id): Path<Uuid>,
    Form(form): Form<EditQuoteForm>,
) -> Response {
    let grant = match state.quote_grants.authenticate(&headers, quote_id) {
        Ok(grant) => grant,
        Err(error) => return error.into_response(),
    };
    if let Err(error) = require_origin(&headers, &state) {
        return error.into_response();
    }
    if form.csrf != grant.csrf_token {
        return AppError::Forbidden.into_response();
    }
    let request = match form.into_request() {
        Ok(request) => request,
        Err(error) => return error.into_response(),
    };
    let Some(client) = state.quote_api.as_ref() else {
        return AppError::ServiceUpstream.into_response();
    };
    match client
        .resubmit(grant.owner_subject, quote_id, &request)
        .await
    {
        Ok(_) => Redirect::to(&format!("/quote/{quote_id}")).into_response(),
        Err(error) => error.into_response(),
    }
}

async fn verified_contacts_for_actor(
    state: &AppState,
    headers: &HeaderMap,
    actor: &AuthContext,
) -> Result<VerifiedContacts, AppError> {
    let token = shared_auth_token_for_actor(state, headers, actor).await?;
    let contacts = state.shared_auth.verified_contacts(&token).await?;
    if contacts
        .email
        .as_ref()
        .is_some_and(|contact| !contact.verified)
        || contacts
            .phone
            .as_ref()
            .is_some_and(|contact| !contact.verified)
    {
        return Err(AppError::Unauthorized);
    }
    Ok(contacts)
}

async fn shared_auth_token_for_actor(
    state: &AppState,
    headers: &HeaderMap,
    actor: &AuthContext,
) -> Result<String, AppError> {
    let token = state
        .shared_auth
        .session_token(headers)
        .ok_or(AppError::Unauthorized)?;
    let shared_actor = state.shared_auth.authenticate_session(&token).await?;
    if shared_actor.user_id != actor.user_id {
        return Err(AppError::Unauthorized);
    }
    Ok(token)
}

#[derive(Debug, Deserialize)]
pub struct PhoneRequestForm {
    csrf: String,
    phone: String,
}

#[derive(Debug, Deserialize)]
pub struct PhoneVerifyForm {
    challenge_id: Uuid,
    code: String,
    csrf: String,
}

#[derive(Debug, Deserialize)]
pub struct QuoteForm {
    csrf: String,
    organization_name: String,
    contact_name: String,
    employee_count: u32,
    #[serde(default)]
    website: String,
    current_stage: String,
    #[serde(default)]
    notes: String,
    #[serde(default)]
    confirm_email: Option<String>,
    #[serde(default)]
    confirm_phone: Option<String>,
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
    infra_saas_only: Option<String>,
    #[serde(default)]
    data_confidential: Option<String>,
    #[serde(default)]
    data_pii: Option<String>,
    #[serde(default)]
    data_phi: Option<String>,
    #[serde(default)]
    data_pci: Option<String>,
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
    fn into_request(self, contact_selection_id: Uuid) -> Result<QuoteRequest, AppError> {
        let organization_name = bounded_required(self.organization_name, 200, "organization name")?;
        let contact_name = bounded_required(self.contact_name, 160, "contact name")?;
        if !(1..=1_000_000).contains(&self.employee_count) {
            return Err(AppError::BadRequest(
                "employee count must be between 1 and 1000000".into(),
            ));
        }
        if !matches!(
            self.current_stage.as_str(),
            "exploring" | "readiness" | "remediation" | "audit_ready" | "renewal"
        ) {
            return Err(AppError::BadRequest(
                "choose a supported current stage".into(),
            ));
        }
        let frameworks = selected(&[
            ("soc2_type_1", &self.soc2_type_1),
            ("soc2_type_2", &self.soc2_type_2),
            ("nist_csf_2", &self.nist_csf_2),
            ("nist_800_53", &self.nist_800_53),
            ("hipaa", &self.hipaa),
            ("iso_27001", &self.iso_27001),
            ("pci_dss_4", &self.pci_dss_4),
            ("fedramp", &self.fedramp),
            ("gdpr", &self.gdpr),
        ]);
        if frameworks.is_empty() {
            return Err(AppError::BadRequest(
                "choose at least one supported framework".into(),
            ));
        }
        let infrastructure = selected(&[
            ("aws", &self.infra_aws),
            ("azure", &self.infra_azure),
            ("gcp", &self.infra_gcp),
            ("supabase", &self.infra_supabase),
            ("on_prem", &self.infra_on_prem),
            ("saas_only", &self.infra_saas_only),
        ]);
        if infrastructure.is_empty() {
            return Err(AppError::BadRequest(
                "choose at least one infrastructure option".into(),
            ));
        }
        let mut data_sensitivity = selected(&[
            ("confidential", &self.data_confidential),
            ("pii", &self.data_pii),
            ("phi", &self.data_phi),
            ("pci", &self.data_pci),
        ]);
        if data_sensitivity.is_empty() {
            data_sensitivity.push("internal".into());
        }
        let website = optional(self.website);
        if website.as_deref().is_some_and(|value| {
            value.len() > 2_048 || !(value.starts_with("https://") || value.starts_with("http://"))
        }) {
            return Err(AppError::BadRequest(
                "website must be a valid HTTP or HTTPS URL".into(),
            ));
        }
        let notes = optional(self.notes);
        if notes.as_deref().is_some_and(|value| value.len() > 5_000) {
            return Err(AppError::BadRequest(
                "notes must be at most 5000 characters".into(),
            ));
        }
        Ok(QuoteRequest {
            annual_revenue_band: None,
            answers_version: 1,
            contact_name,
            contact_selection_id,
            context_key: "quote-analysis".into(),
            current_stage: self.current_stage,
            data_sensitivity,
            employee_count: self.employee_count,
            frameworks,
            has_incident_response_plan: self.has_incident_response_plan.is_some(),
            has_policies: self.has_policies.is_some(),
            has_risk_assessment: self.has_risk_assessment.is_some(),
            has_security_program: self.has_security_program.is_some(),
            has_vendor_management: self.has_vendor_management.is_some(),
            infrastructure,
            notes,
            organization_name,
            target_date: None,
            website,
        })
    }
}

#[derive(Debug, Deserialize)]
pub struct EditQuoteForm {
    csrf: String,
    contact_selection_id: Uuid,
    organization_name: String,
    contact_name: String,
    employee_count: u32,
    frameworks: String,
    current_stage: String,
    infrastructure: String,
    data_sensitivity: String,
    has_security_program: bool,
    has_policies: bool,
    has_risk_assessment: bool,
    has_incident_response_plan: bool,
    has_vendor_management: bool,
    #[serde(default)]
    notes: String,
}

impl EditQuoteForm {
    fn into_request(self) -> Result<QuoteRequest, AppError> {
        let frameworks = comma_list(&self.frameworks, 12);
        let infrastructure = comma_list(&self.infrastructure, 12);
        let data_sensitivity = comma_list(&self.data_sensitivity, 12);
        if frameworks.is_empty() || infrastructure.is_empty() || data_sensitivity.is_empty() {
            return Err(AppError::BadRequest(
                "the stored quote scope is invalid".into(),
            ));
        }
        Ok(QuoteRequest {
            annual_revenue_band: None,
            answers_version: 1,
            contact_name: bounded_required(self.contact_name, 160, "contact name")?,
            contact_selection_id: self.contact_selection_id,
            context_key: "quote-analysis".into(),
            current_stage: self.current_stage,
            data_sensitivity,
            employee_count: self.employee_count,
            frameworks,
            has_incident_response_plan: self.has_incident_response_plan,
            has_policies: self.has_policies,
            has_risk_assessment: self.has_risk_assessment,
            has_security_program: self.has_security_program,
            has_vendor_management: self.has_vendor_management,
            infrastructure,
            notes: optional(self.notes),
            organization_name: bounded_required(self.organization_name, 200, "organization name")?,
            target_date: None,
            website: None,
        })
    }
}

fn selected(values: &[(&str, &Option<String>)]) -> Vec<String> {
    values
        .iter()
        .filter(|(_, selected)| selected.is_some())
        .map(|(value, _)| (*value).to_owned())
        .collect()
}

fn comma_list(value: &str, maximum: usize) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .take(maximum)
        .map(str::to_owned)
        .collect()
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

fn optional(value: String) -> Option<String> {
    let value = value.trim().to_owned();
    (!value.is_empty()).then_some(value)
}

fn valid_e164(value: &str) -> bool {
    (9..=16).contains(&value.len())
        && value.starts_with('+')
        && value.as_bytes().get(1).is_some_and(|byte| *byte != b'0')
        && value[1..].bytes().all(|byte| byte.is_ascii_digit())
}

fn quote_root(path: &str) -> &'static str {
    if path == PUBLIC_QUOTE_PATH || path.starts_with("/quote/") {
        PUBLIC_QUOTE_PATH
    } else {
        SIGNED_IN_QUOTE_PATH
    }
}

fn shared_auth_sign_in_url(app_base_url: &str, return_path: &str) -> reqwest::Url {
    let return_path = quote_root(return_path);
    let mut destination = reqwest::Url::parse(app_base_url)
        .expect("APP_BASE_URL was validated before application state construction");
    destination.set_path(SHARED_AUTH_BROWSER_SIGN_IN_PATH);
    destination.set_query(None);
    destination
        .query_pairs_mut()
        .append_pair("client_id", "canonical-web")
        .append_pair("return", return_path);
    destination
}

fn shared_auth_redirect(state: &AppState, return_path: &str) -> Redirect {
    let destination = shared_auth_sign_in_url(&state.config.app_base_url, return_path);
    Redirect::temporary(destination.as_str())
}

fn htmx_or_browser_auth_redirect(
    headers: &HeaderMap,
    state: &AppState,
    return_path: &str,
) -> Response {
    let destination = shared_auth_sign_in_url(&state.config.app_base_url, return_path);
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
    } else {
        (StatusCode::UNPROCESSABLE_ENTITY, fragment).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn form_maps_verified_contract_fields() {
        let form = QuoteForm {
            csrf: "csrf".into(),
            organization_name: "Example".into(),
            contact_name: "Casey".into(),
            employee_count: 15,
            website: "https://example.invalid".into(),
            current_stage: "readiness".into(),
            notes: String::new(),
            confirm_email: Some("on".into()),
            confirm_phone: Some("on".into()),
            soc2_type_1: None,
            soc2_type_2: Some("on".into()),
            nist_csf_2: None,
            nist_800_53: None,
            hipaa: Some("on".into()),
            iso_27001: None,
            pci_dss_4: None,
            fedramp: None,
            gdpr: None,
            infra_aws: Some("on".into()),
            infra_azure: None,
            infra_gcp: None,
            infra_supabase: None,
            infra_on_prem: None,
            infra_saas_only: None,
            data_confidential: Some("on".into()),
            data_pii: None,
            data_phi: Some("on".into()),
            data_pci: None,
            has_security_program: Some("on".into()),
            has_policies: None,
            has_risk_assessment: None,
            has_incident_response_plan: None,
            has_vendor_management: None,
        };
        let selection_id = Uuid::new_v4();
        let request = form.into_request(selection_id).unwrap();
        assert_eq!(request.contact_selection_id, selection_id);
        assert_eq!(request.frameworks, ["soc2_type_2", "hipaa"]);
        assert_eq!(request.infrastructure, ["aws"]);
        assert!(request.has_security_program);
    }

    #[test]
    fn auth_return_target_is_same_origin_and_bounded() {
        let destination = shared_auth_sign_in_url("https://app.canonical.plus", PUBLIC_QUOTE_PATH);
        assert_eq!(destination.host_str(), Some("app.canonical.plus"));
        assert_eq!(destination.path(), SHARED_AUTH_BROWSER_SIGN_IN_PATH);
        assert_eq!(
            destination
                .query_pairs()
                .find(|(name, _)| name == "return")
                .map(|(_, value)| value.into_owned()),
            Some(PUBLIC_QUOTE_PATH.into())
        );
    }

    #[test]
    fn phone_validation_requires_e164() {
        assert!(valid_e164("+14155550100"));
        assert!(!valid_e164("4155550100"));
        assert!(!valid_e164("+012345678"));
    }
}
