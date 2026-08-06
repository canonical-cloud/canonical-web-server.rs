use axum::{
    extract::State,
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Form, Router,
};
use serde::Deserialize;
use serde_json::json;

use crate::{
    auth::require_origin,
    edge::{signed_headers, EdgeAuthenticated, EdgeIdentity},
    error::AppError,
    views, AppState,
};

const API_PATH: &str = "/v1/quotes";

pub fn router() -> Router<AppState> {
    Router::new().route("/quote", get(quote_page).post(submit_quote))
}

async fn quote_page(EdgeAuthenticated(identity): EdgeAuthenticated) -> Response {
    views::quote_page(&identity.email).into_response()
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct QuoteForm {
    company_name: String,
    company_size: String,
    industry: String,
    current_stage: String,
    target_timeline: String,
    data_types: String,
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

    fn validate(&self) -> Result<(), &'static str> {
        if self.company_name.trim().is_empty() || self.company_name.chars().count() > 200 {
            return Err("Company name is required and must be at most 200 characters.");
        }
        for (value, maximum) in [
            (&self.company_size, 80),
            (&self.industry, 120),
            (&self.current_stage, 120),
            (&self.target_timeline, 120),
        ] {
            if value.trim().is_empty() || value.chars().count() > maximum {
                return Err("Complete every required scoping field.");
            }
        }
        if self.frameworks().is_empty() {
            return Err("Choose at least one compliance framework.");
        }
        if self.notes.chars().count() > 4_000 {
            return Err("Notes must be at most 4000 characters.");
        }
        Ok(())
    }
}

async fn submit_quote(
    State(state): State<AppState>,
    headers: HeaderMap,
    EdgeAuthenticated(identity): EdgeAuthenticated,
    Form(form): Form<QuoteForm>,
) -> Result<Response, AppError> {
    require_origin(&headers, &state)?;
    if let Err(message) = form.validate() {
        return Ok(quote_error(&headers, message));
    }
    let quote = state
        .quote
        .as_ref()
        .ok_or(AppError::Configuration("quote client is not initialized"))?;
    let payload = json!({
        "company_name": form.company_name.trim(),
        "company_size": form.company_size.trim(),
        "frameworks": form.frameworks(),
        "industry": form.industry.trim(),
        "data_types": split_list(&form.data_types),
        "cloud_providers": split_list(&form.cloud_providers),
        "current_stage": form.current_stage.trim(),
        "target_timeline": form.target_timeline.trim(),
        "notes": form.notes.trim(),
    });
    let assertion_headers = signed_headers(
        &Method::POST,
        API_PATH,
        &EdgeIdentity {
            user_id: identity.user_id,
            email: identity.email,
        },
        &quote.origin_assertion_secret,
    )?;
    let response = quote
        .http
        .post(format!("{}{}", quote.base_url, API_PATH))
        .headers(assertion_headers)
        .json(&payload)
        .send()
        .await?;
    let status = response.status();
    if !status.is_success() {
        tracing::warn!(%status, "quote API rejected the request");
        let message = if status == StatusCode::UNPROCESSABLE_ENTITY
            || status == StatusCode::BAD_REQUEST
        {
            "Review the form fields and try again."
        } else {
            "Quote analysis is temporarily unavailable. Your information was not accepted; please try again."
        };
        return Ok(quote_error(&headers, message));
    }
    let quote = response.json::<serde_json::Value>().await?;
    Ok(views::quote_result(&quote).into_response())
}

fn split_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .take(12)
        .map(|item| item.chars().take(80).collect())
        .collect()
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
        let values = split_list("AWS, GCP, , Azure");
        assert_eq!(values, ["AWS", "GCP", "Azure"]);
    }

    #[test]
    fn a_framework_is_required() {
        let form = QuoteForm {
            company_name: "Canonical Example".into(),
            company_size: "51-200".into(),
            industry: "Software".into(),
            current_stage: "Starting".into(),
            target_timeline: "This quarter".into(),
            data_types: String::new(),
            cloud_providers: String::new(),
            notes: String::new(),
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
