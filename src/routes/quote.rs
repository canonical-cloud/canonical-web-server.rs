use crate::{
    auth::{require_csrf, require_origin, SessionAuthenticated},
    error::AppError,
    quotes::{self, QuoteRequest},
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

const QUOTE_RETURN_TO: &str = "https://app.canonical.plus/u/quote";
const DEFAULT_SHARED_AUTH_AUTHORIZE: &str = "https://auth.canonical.plus/authorize";
const DEFAULT_SHARED_AUTH_CLIENT_ID: &str = "canonical-plus-web";

pub async fn page(
    State(state): State<AppState>,
    auth: Result<SessionAuthenticated, AppError>,
) -> Response {
    let actor = match auth {
        Ok(SessionAuthenticated(actor)) => actor,
        Err(AppError::Unauthorized) => return shared_auth_redirect().into_response(),
        Err(error) => return error.into_response(),
    };
    match quotes::list_quotes(&state, actor.user_id, 20).await {
        Ok(records) => quotes::quote_page(&actor, &records).into_response(),
        Err(error) => error.into_response(),
    }
}

pub async fn detail(
    State(state): State<AppState>,
    auth: Result<SessionAuthenticated, AppError>,
    Path(raw_id): Path<String>,
) -> Response {
    let actor = match auth {
        Ok(SessionAuthenticated(actor)) => actor,
        Err(AppError::Unauthorized) => return shared_auth_redirect().into_response(),
        Err(error) => return error.into_response(),
    };
    let quote_id = match Uuid::parse_str(&raw_id) {
        Ok(id) => id,
        Err(_) => return AppError::NotFound.into_response(),
    };
    match quotes::get_quote(&state, actor.user_id, quote_id).await {
        Ok(record) => quotes::quote_detail_page(&actor, &record).into_response(),
        Err(error) => error.into_response(),
    }
}

pub async fn submit(
    State(state): State<AppState>,
    headers: HeaderMap,
    auth: Result<SessionAuthenticated, AppError>,
    Form(form): Form<QuoteForm>,
) -> Response {
    let actor = match auth {
        Ok(SessionAuthenticated(actor)) => actor,
        Err(AppError::Unauthorized) => return htmx_or_browser_auth_redirect(&headers),
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
        Err(AppError::BadRequest(message)) => return form_error(&headers, &message),
        Err(error) => return error.into_response(),
    };
    match quotes::create_quote(state, actor.user_id, request).await {
        Ok(record) if headers.contains_key("hx-request") => {
            quotes::quote_status_fragment(&record).into_response()
        }
        Ok(record) => Redirect::to(&format!("/u/quote/{}", record.id)).into_response(),
        Err(AppError::BadRequest(message)) => form_error(&headers, &message),
        Err(error) => error.into_response(),
    }
}

#[derive(Debug, Deserialize)]
pub struct QuoteForm {
    csrf: String,
    company: String,
    #[serde(default)]
    website: String,
    #[serde(default)]
    employee_count: String,
    #[serde(default)]
    target_date: String,
    #[serde(default)]
    cloud_providers: String,
    #[serde(default)]
    notes: String,
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
    fedramp: Option<String>,
    #[serde(default)]
    pci_dss: Option<String>,
    #[serde(default)]
    handles_phi: Option<String>,
    #[serde(default)]
    handles_card_data: Option<String>,
    #[serde(default)]
    government_customers: Option<String>,
}

impl QuoteForm {
    fn into_request(self) -> Result<QuoteRequest, AppError> {
        let employee_count =
            match self.employee_count.trim() {
                "" => None,
                value => Some(value.parse::<u32>().map_err(|_| {
                    AppError::BadRequest("employees must be a whole number".into())
                })?),
            };
        let frameworks = [
            ("soc2", self.soc2),
            ("nist_csf", self.nist_csf),
            ("nist_800_53", self.nist_800_53),
            ("hipaa", self.hipaa),
            ("iso_27001", self.iso_27001),
            ("fedramp", self.fedramp),
            ("pci_dss", self.pci_dss),
        ]
        .into_iter()
        .filter_map(|(name, selected)| selected.map(|_| name.to_owned()))
        .collect();
        let cloud_providers = self
            .cloud_providers
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .collect();
        QuoteRequest {
            company: self.company,
            website: optional(self.website),
            employee_count,
            frameworks,
            cloud_providers,
            handles_phi: self.handles_phi.is_some(),
            handles_card_data: self.handles_card_data.is_some(),
            government_customers: self.government_customers.is_some(),
            target_date: optional(self.target_date),
            notes: optional(self.notes),
        }
        .validated()
    }
}

fn optional(value: String) -> Option<String> {
    (!value.trim().is_empty()).then_some(value)
}

fn shared_auth_redirect() -> Redirect {
    let authorize = std::env::var("SHARED_AUTH_AUTHORIZE_URL")
        .unwrap_or_else(|_| DEFAULT_SHARED_AUTH_AUTHORIZE.to_owned());
    let client_id = std::env::var("SHARED_AUTH_CLIENT_ID")
        .unwrap_or_else(|_| DEFAULT_SHARED_AUTH_CLIENT_ID.to_owned());
    let mut destination = reqwest::Url::parse(&authorize)
        .ok()
        .filter(|url| {
            url.scheme() == "https"
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
        })
        .unwrap_or_else(|| reqwest::Url::parse(DEFAULT_SHARED_AUTH_AUTHORIZE).unwrap());
    destination
        .query_pairs_mut()
        .append_pair("client_id", &client_id)
        .append_pair("return_to", QUOTE_RETURN_TO);
    Redirect::temporary(destination.as_str())
}

fn htmx_or_browser_auth_redirect(headers: &HeaderMap) -> Response {
    let redirect = shared_auth_redirect();
    if headers.contains_key("hx-request") {
        let location = redirect.into_response();
        let destination = location
            .headers()
            .get(axum::http::header::LOCATION)
            .cloned()
            .unwrap_or_else(|| HeaderValue::from_static(DEFAULT_SHARED_AUTH_AUTHORIZE));
        let mut response = StatusCode::UNAUTHORIZED.into_response();
        response.headers_mut().insert("hx-redirect", destination);
        response
    } else {
        redirect.into_response()
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
    fn form_maps_selected_frameworks_and_clouds() {
        let form = QuoteForm {
            csrf: "csrf".into(),
            company: "Example".into(),
            website: "https://example.com".into(),
            employee_count: "15".into(),
            target_date: "2027-02-01".into(),
            cloud_providers: "aws, cloudflare".into(),
            notes: String::new(),
            soc2: Some("true".into()),
            nist_csf: None,
            nist_800_53: None,
            hipaa: Some("true".into()),
            iso_27001: None,
            fedramp: None,
            pci_dss: None,
            handles_phi: Some("true".into()),
            handles_card_data: None,
            government_customers: None,
        };
        let request = form.into_request().unwrap();
        assert_eq!(request.frameworks, ["hipaa", "soc2"]);
        assert_eq!(request.cloud_providers, ["aws", "cloudflare"]);
        assert!(request.handles_phi);
    }

    #[test]
    fn auth_return_target_is_fixed() {
        assert_eq!(QUOTE_RETURN_TO, "https://app.canonical.plus/u/quote");
    }
}
