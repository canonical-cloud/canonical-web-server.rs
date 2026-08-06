use axum::{
    extract::{ws::WebSocketUpgrade, DefaultBodyLimit, Form, Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
    Json, Router,
};
use maud::html;
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    auth::{require_origin, EdgeAuthenticated},
    error::AppError,
    quotes::{self, QuoteInput},
    AppState,
};

pub fn web_router() -> Router<AppState> {
    Router::new()
        .route("/", get(page).post(submit_form))
        .route("/{id}", get(detail))
        .route("/{id}/status", get(status_fragment))
        .layer(DefaultBodyLimit::max(64 * 1024))
}

pub fn api_router() -> Router<AppState> {
    Router::new()
        .route("/", post(submit_json))
        .route("/{id}", get(get_json))
        .layer(DefaultBodyLimit::max(64 * 1024))
}

pub fn websocket_router() -> Router<AppState> {
    Router::new().route("/{id}", get(upgrade))
}

async fn page(EdgeAuthenticated(identity): EdgeAuthenticated) -> impl IntoResponse {
    quotes::views::quote_page(identity.email.as_deref(), None)
}

async fn detail(
    State(state): State<AppState>,
    EdgeAuthenticated(identity): EdgeAuthenticated,
    Path(id): Path<Uuid>,
) -> Result<Response, AppError> {
    let quote = state.quotes.get(&identity.subject, id).await?;
    Ok(quotes::views::quote_page(identity.email.as_deref(), Some(&quote)).into_response())
}

async fn status_fragment(
    State(state): State<AppState>,
    EdgeAuthenticated(identity): EdgeAuthenticated,
    Path(id): Path<Uuid>,
) -> Result<Response, AppError> {
    let quote = state.quotes.get(&identity.subject, id).await?;
    Ok(quotes::views::quote_status(&quote).into_response())
}

async fn submit_form(
    State(state): State<AppState>,
    EdgeAuthenticated(identity): EdgeAuthenticated,
    headers: HeaderMap,
    Form(form): Form<QuoteForm>,
) -> Result<Response, AppError> {
    // This endpoint is intentionally first-party and cookie-friendly. Reject a
    // missing or unapproved Origin before accepting a state-changing form.
    require_origin(&headers, &state)?;
    let hx_request = headers
        .get("hx-request")
        .and_then(|value| value.to_str().ok())
        == Some("true");
    match state.quotes.submit(&identity, form.into_input()).await {
        Ok(quote) if hx_request => Ok(quotes::views::quote_status(&quote).into_response()),
        Ok(quote) => Ok(Redirect::to(&format!("/u/quote/{}", quote.id)).into_response()),
        Err(AppError::BadRequest(message)) if hx_request => Ok((
            StatusCode::BAD_REQUEST,
            html! { p class="error" role="alert" { (message) } },
        )
            .into_response()),
        Err(error) => Err(error),
    }
}

async fn submit_json(
    State(state): State<AppState>,
    EdgeAuthenticated(identity): EdgeAuthenticated,
    Json(input): Json<QuoteInput>,
) -> Result<impl IntoResponse, AppError> {
    let quote = state.quotes.submit(&identity, input).await?;
    Ok((StatusCode::ACCEPTED, Json(quote)))
}

async fn get_json(
    State(state): State<AppState>,
    EdgeAuthenticated(identity): EdgeAuthenticated,
    Path(id): Path<Uuid>,
) -> Result<Json<quotes::QuoteRecord>, AppError> {
    Ok(Json(state.quotes.get(&identity.subject, id).await?))
}

async fn upgrade(
    State(state): State<AppState>,
    EdgeAuthenticated(identity): EdgeAuthenticated,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    websocket: WebSocketUpgrade,
) -> Result<Response, AppError> {
    // Browsers always provide Origin on the WebSocket handshake. Native SDKs
    // may omit it; the edge-secret and Shared Auth identity still authenticate
    // those requests.
    if headers.contains_key(header::ORIGIN) {
        require_origin(&headers, &state)?;
    }
    state.quotes.get(&identity.subject, id).await?;
    let permit = state
        .quotes
        .hub()
        .try_acquire_socket(&identity.subject)
        .ok_or(AppError::RateLimited {
            retry_after_seconds: 60,
        })?;
    let hub = state.quotes.hub().clone();
    let subject = identity.subject;
    Ok(websocket
        .max_message_size(16 * 1024)
        .max_frame_size(16 * 1024)
        .on_upgrade(move |socket| async move {
            hub.serve(socket, subject, id, permit).await;
        }))
}

#[derive(Debug, Deserialize)]
struct QuoteForm {
    company_name: String,
    #[serde(default)]
    website: String,
    employee_count: i32,
    target_timeline: String,
    current_controls: String,
    #[serde(default)]
    notes: String,
    #[serde(default)]
    framework_soc2: Option<String>,
    #[serde(default)]
    framework_nist_csf: Option<String>,
    #[serde(default)]
    framework_nist_800_53: Option<String>,
    #[serde(default)]
    framework_hipaa: Option<String>,
    #[serde(default)]
    framework_iso_27001: Option<String>,
    #[serde(default)]
    framework_pci_dss: Option<String>,
    #[serde(default)]
    framework_gdpr: Option<String>,
    #[serde(default)]
    framework_fedramp: Option<String>,
    #[serde(default)]
    cloud_aws: Option<String>,
    #[serde(default)]
    cloud_azure: Option<String>,
    #[serde(default)]
    cloud_gcp: Option<String>,
    #[serde(default)]
    cloud_other: Option<String>,
    #[serde(default)]
    data_pii: Option<String>,
    #[serde(default)]
    data_phi: Option<String>,
    #[serde(default)]
    data_pci: Option<String>,
    #[serde(default)]
    data_credentials: Option<String>,
    #[serde(default)]
    data_regulated: Option<String>,
    #[serde(default)]
    data_none: Option<String>,
}

impl QuoteForm {
    fn into_input(self) -> QuoteInput {
        QuoteInput {
            company_name: self.company_name,
            website: non_empty(self.website),
            employee_count: self.employee_count,
            frameworks: selected([
                ("soc2", self.framework_soc2),
                ("nist_csf", self.framework_nist_csf),
                ("nist_800_53", self.framework_nist_800_53),
                ("hipaa", self.framework_hipaa),
                ("iso_27001", self.framework_iso_27001),
                ("pci_dss", self.framework_pci_dss),
                ("gdpr", self.framework_gdpr),
                ("fedramp", self.framework_fedramp),
            ]),
            cloud_providers: selected([
                ("aws", self.cloud_aws),
                ("azure", self.cloud_azure),
                ("gcp", self.cloud_gcp),
                ("other", self.cloud_other),
            ]),
            sensitive_data: selected([
                ("pii", self.data_pii),
                ("phi", self.data_phi),
                ("pci", self.data_pci),
                ("credentials", self.data_credentials),
                ("regulated", self.data_regulated),
                ("none", self.data_none),
            ]),
            current_controls: self.current_controls,
            target_timeline: self.target_timeline,
            notes: non_empty(self.notes),
        }
    }
}

fn selected<const N: usize>(values: [(&'static str, Option<String>); N]) -> Vec<String> {
    values
        .into_iter()
        .filter_map(|(name, selected)| selected.map(|_| name.to_owned()))
        .collect()
}

fn non_empty(value: String) -> Option<String> {
    (!value.trim().is_empty()).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkbox_selection_ignores_untrusted_values() {
        assert_eq!(
            selected([
                ("soc2", Some("attacker-controlled".into())),
                ("hipaa", None)
            ]),
            vec!["soc2"]
        );
    }
}
