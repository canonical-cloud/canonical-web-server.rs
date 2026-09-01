use std::collections::{BTreeMap, BTreeSet};

use axum::{
    extract::{Host, RawForm, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Redirect, Response},
};
use chrono::{SecondsFormat, Utc};
use uuid::Uuid;

use crate::{
    error::AppError,
    pre_interest_api::{InterestArea, PreInterestRegistrationRequest, RegistrationHost},
    AppState,
};

const MAX_FORM_FIELDS: usize = 32;
const MAX_KEY_BYTES: usize = 64;
const MAX_VALUE_BYTES: usize = 2_048;
const RECEIVED_PATH: &str = "/registration-received";
const SEC_FETCH_SITE: &str = "sec-fetch-site";

const ALLOWED_FIELDS: &[&str] = &[
    "consentRevision",
    "marketingConsentCopyRevision",
    "email",
    "displayName",
    "websiteUrl",
    "organizationName",
    "interestAreas",
    "registrationConsent",
    "marketingPermission",
];

pub async fn submit(
    State(state): State<AppState>,
    Host(raw_host): Host,
    headers: HeaderMap,
    RawForm(body): RawForm,
) -> Response {
    let host = match verify_same_origin(&raw_host, &headers) {
        Ok(host) => host,
        Err(AppError::Forbidden) => return forbidden_response(),
        Err(_) => return invalid_response(),
    };
    let form = match PreInterestForm::parse(&body) {
        Ok(form) => form,
        Err(_) => return invalid_response(),
    };
    let request = match form.into_request(
        host,
        Uuid::new_v4(),
        Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
    ) {
        Ok(request) => request,
        Err(_) => return invalid_response(),
    };
    let Some(client) = state.pre_interest_api.as_ref() else {
        return unavailable_response();
    };

    match client.create(&request).await {
        Ok(_) => Redirect::to(RECEIVED_PATH).into_response(),
        Err(AppError::BadRequest(_)) => invalid_response(),
        Err(AppError::RateLimited {
            retry_after_seconds,
        }) => AppError::RateLimited {
            retry_after_seconds,
        }
        .into_response(),
        Err(_) => unavailable_response(),
    }
}

fn verify_same_origin(raw_host: &str, headers: &HeaderMap) -> Result<RegistrationHost, AppError> {
    let host = RegistrationHost::parse(raw_host).ok_or(AppError::Forbidden)?;
    let expected_origin = format!("https://{}", host.as_str());
    let origin = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .ok_or(AppError::Forbidden)?;
    if origin != expected_origin {
        return Err(AppError::Forbidden);
    }
    if let Some(site) = headers
        .get(SEC_FETCH_SITE)
        .and_then(|value| value.to_str().ok())
    {
        if site != "same-origin" {
            return Err(AppError::Forbidden);
        }
    }
    Ok(host)
}

struct PreInterestForm {
    consent_revision: String,
    marketing_consent_copy_revision: String,
    email: String,
    display_name: Option<String>,
    website_url: Option<String>,
    organization_name: Option<String>,
    interest_areas: Vec<String>,
    registration_consent: String,
    marketing_permission: String,
}

impl PreInterestForm {
    fn parse(body: &[u8]) -> Result<Self, AppError> {
        let mut fields = parse_form(body)?;
        let consent_revision = take_required(&mut fields, "consentRevision")?;
        let marketing_consent_copy_revision =
            take_required(&mut fields, "marketingConsentCopyRevision")?;
        let email = take_required(&mut fields, "email")?;
        let display_name = take_optional(&mut fields, "displayName")?;
        let website_url = take_optional(&mut fields, "websiteUrl")?;
        let organization_name = take_optional(&mut fields, "organizationName")?;
        let interest_areas = fields.remove("interestAreas").unwrap_or_default();
        let registration_consent = take_required(&mut fields, "registrationConsent")?;
        let marketing_permission = take_required(&mut fields, "marketingPermission")?;
        if !fields.is_empty() {
            return Err(invalid_form());
        }
        Ok(Self {
            consent_revision,
            marketing_consent_copy_revision,
            email,
            display_name,
            website_url,
            organization_name,
            interest_areas,
            registration_consent,
            marketing_permission,
        })
    }

    fn into_request(
        self,
        host: RegistrationHost,
        request_id: Uuid,
        consented_at: String,
    ) -> Result<PreInterestRegistrationRequest, AppError> {
        let email = normalize_email(&self.email)?;
        let display_name = bounded_optional(self.display_name, 120)?;
        let organization_name = bounded_optional(self.organization_name, 200)?;
        match host {
            RegistrationHost::User if organization_name.is_some() => return Err(invalid_form()),
            RegistrationHost::Organization if organization_name.is_none() => {
                return Err(invalid_form())
            }
            _ => {}
        }
        let website_url = normalize_website(self.website_url)?;
        let consent_revision = portable_identifier(&self.consent_revision)?;
        if self.registration_consent != "true" {
            return Err(invalid_form());
        }

        let marketing_consent = match self.marketing_permission.as_str() {
            "yes" => true,
            "no" => false,
            _ => return Err(invalid_form()),
        };
        let marketing_consent_revision = if marketing_consent {
            Some(portable_identifier(&self.marketing_consent_copy_revision)?)
        } else {
            None
        };

        if self.interest_areas.is_empty() || self.interest_areas.len() > 9 {
            return Err(invalid_form());
        }
        let mut interests = BTreeSet::new();
        for value in self.interest_areas {
            let value = value.trim();
            let interest = InterestArea::parse(value).ok_or_else(invalid_form)?;
            if !interests.insert(interest) {
                return Err(invalid_form());
            }
        }

        Ok(PreInterestRegistrationRequest {
            request_id,
            email,
            party_type: host.party_type(),
            organization_name,
            interest_areas: interests.into_iter().collect(),
            consent_revision,
            consented_at,
            source_host: host.as_str().to_owned(),
            locale: None,
            referral_code: None,
            display_name,
            website_url,
            registration_consent: true,
            marketing_consent,
            marketing_consent_revision,
        })
    }
}

fn parse_form(body: &[u8]) -> Result<BTreeMap<String, Vec<String>>, AppError> {
    if body.is_empty() {
        return Err(invalid_form());
    }
    let mut fields = BTreeMap::<String, Vec<String>>::new();
    let mut count = 0_usize;
    for pair in body.split(|byte| *byte == b'&') {
        if pair.is_empty() {
            return Err(invalid_form());
        }
        count = count.saturating_add(1);
        if count > MAX_FORM_FIELDS {
            return Err(invalid_form());
        }
        let separator = pair
            .iter()
            .position(|byte| *byte == b'=')
            .ok_or_else(invalid_form)?;
        let key = decode_component(&pair[..separator], MAX_KEY_BYTES)?;
        let value = decode_component(&pair[separator + 1..], MAX_VALUE_BYTES)?;
        if !ALLOWED_FIELDS.contains(&key.as_str()) {
            return Err(invalid_form());
        }
        fields.entry(key).or_default().push(value);
    }
    Ok(fields)
}

fn decode_component(input: &[u8], max_bytes: usize) -> Result<String, AppError> {
    if input.len() > max_bytes.saturating_mul(3) {
        return Err(invalid_form());
    }
    let mut output = Vec::with_capacity(input.len().min(max_bytes));
    let mut index = 0_usize;
    while index < input.len() {
        let byte = match input[index] {
            b'+' => b' ',
            b'%' => {
                if index.saturating_add(2) >= input.len() {
                    return Err(invalid_form());
                }
                let high = hex(input[index + 1]).ok_or_else(invalid_form)?;
                let low = hex(input[index + 2]).ok_or_else(invalid_form)?;
                index += 2;
                high.saturating_mul(16).saturating_add(low)
            }
            byte => byte,
        };
        output.push(byte);
        if output.len() > max_bytes {
            return Err(invalid_form());
        }
        index += 1;
    }
    let decoded = String::from_utf8(output).map_err(|_| invalid_form())?;
    if decoded.chars().any(char::is_control) {
        return Err(invalid_form());
    }
    Ok(decoded)
}

const fn hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn take_required(
    fields: &mut BTreeMap<String, Vec<String>>,
    key: &str,
) -> Result<String, AppError> {
    let values = fields.remove(key).ok_or_else(invalid_form)?;
    if values.len() != 1 || values[0].is_empty() {
        return Err(invalid_form());
    }
    Ok(values.into_iter().next().expect("length checked"))
}

fn take_optional(
    fields: &mut BTreeMap<String, Vec<String>>,
    key: &str,
) -> Result<Option<String>, AppError> {
    let Some(values) = fields.remove(key) else {
        return Ok(None);
    };
    if values.len() != 1 {
        return Err(invalid_form());
    }
    let value = values.into_iter().next().expect("length checked");
    if value.trim().is_empty() {
        Ok(None)
    } else {
        Ok(Some(value))
    }
}

fn normalize_email(value: &str) -> Result<String, AppError> {
    let value = value.trim().to_lowercase();
    if value.chars().count() < 3
        || value.chars().count() > 320
        || value.chars().any(char::is_whitespace)
    {
        return Err(invalid_form());
    }
    let mut parts = value.split('@');
    let local = parts.next().unwrap_or_default();
    let domain = parts.next().unwrap_or_default();
    if local.is_empty()
        || domain.is_empty()
        || parts.next().is_some()
        || !domain.contains('.')
        || domain.starts_with('.')
        || domain.ends_with('.')
    {
        return Err(invalid_form());
    }
    Ok(value)
}

fn bounded_optional(value: Option<String>, max_chars: usize) -> Result<Option<String>, AppError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.trim().to_owned();
    if value.is_empty() || value.chars().count() > max_chars {
        return Err(invalid_form());
    }
    Ok(Some(value))
}

fn normalize_website(value: Option<String>) -> Result<Option<String>, AppError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.trim();
    if value.chars().count() > 2_048 {
        return Err(invalid_form());
    }
    let parsed = reqwest::Url::parse(value).map_err(|_| invalid_form())?;
    if parsed.scheme() != "https"
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.fragment().is_some()
    {
        return Err(invalid_form());
    }
    Ok(Some(parsed.to_string()))
}

fn portable_identifier(value: &str) -> Result<String, AppError> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 64
        || !value.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphanumeric() || (index > 0 && b"._-".contains(&byte))
        })
    {
        return Err(invalid_form());
    }
    Ok(value.to_owned())
}

fn invalid_form() -> AppError {
    AppError::BadRequest("review the registration fields and try again".into())
}

fn invalid_response() -> Response {
    (
        StatusCode::BAD_REQUEST,
        [(header::CACHE_CONTROL, HeaderValue::from_static("no-store"))],
        "We could not accept that registration. Review the fields and try again.",
    )
        .into_response()
}

fn forbidden_response() -> Response {
    (
        StatusCode::FORBIDDEN,
        [(header::CACHE_CONTROL, HeaderValue::from_static("no-store"))],
        "This registration must be submitted from its Canonical page.",
    )
        .into_response()
}

fn unavailable_response() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        [(header::CACHE_CONTROL, HeaderValue::from_static("no-store"))],
        "Registration is temporarily unavailable. Please try again later.",
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(origin: &'static str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::ORIGIN, HeaderValue::from_static(origin));
        headers.insert(SEC_FETCH_SITE, HeaderValue::from_static("same-origin"));
        headers
    }

    fn individual_form(marketing: &str) -> Vec<u8> {
        format!(
            "consentRevision=pre-interest-v1&marketingConsentCopyRevision=marketing-v1&email=Alex%40Example.COM&displayName=Alex+Mills&websiteUrl=https%3A%2F%2Fexample.com&interestAreas=soc2&interestAreas=nist&registrationConsent=true&marketingPermission={marketing}"
        )
        .into_bytes()
    }

    #[test]
    fn derives_host_party_time_and_opaque_request_identity() {
        let form = PreInterestForm::parse(&individual_form("yes")).unwrap();
        let request_id = Uuid::parse_str("d93d96af-d8a8-42c3-a152-7f371df71f6a").unwrap();
        let request = form
            .into_request(
                RegistrationHost::User,
                request_id,
                "2026-09-01T15:00:00.000Z".into(),
            )
            .unwrap();
        assert_eq!(request.request_id, request_id);
        assert_eq!(request.email, "alex@example.com");
        assert_eq!(request.source_host, "user.canonical.plus");
        assert_eq!(
            request.party_type,
            crate::pre_interest_api::PartyType::Individual
        );
        assert!(request.registration_consent);
        assert!(request.marketing_consent);
        assert_eq!(
            request.marketing_consent_revision.as_deref(),
            Some("marketing-v1")
        );
        assert_eq!(request.interest_areas.len(), 2);
    }

    #[test]
    fn declining_marketing_does_not_claim_a_marketing_revision() {
        let form = PreInterestForm::parse(&individual_form("no")).unwrap();
        let request = form
            .into_request(
                RegistrationHost::User,
                Uuid::nil(),
                "2026-09-01T15:00:00.000Z".into(),
            )
            .unwrap();
        assert!(!request.marketing_consent);
        assert!(request.marketing_consent_revision.is_none());
    }

    #[test]
    fn origin_and_host_must_match_exactly() {
        assert!(verify_same_origin(
            "user.canonical.plus",
            &headers("https://user.canonical.plus")
        )
        .is_ok());
        assert!(verify_same_origin(
            "user.canonical.plus",
            &headers("https://org.canonical.plus")
        )
        .is_err());
        assert!(verify_same_origin(
            "user.canonical.plus.evil.example",
            &headers("https://user.canonical.plus.evil.example")
        )
        .is_err());
    }

    #[test]
    fn unknown_fields_and_duplicate_singletons_fail_closed() {
        let mut unknown = individual_form("no");
        unknown.extend_from_slice(b"&notes=secret");
        assert!(PreInterestForm::parse(&unknown).is_err());

        let mut duplicate = individual_form("no");
        duplicate.extend_from_slice(b"&email=other%40example.com");
        assert!(PreInterestForm::parse(&duplicate).is_err());
    }

    #[test]
    fn organization_name_is_host_bound() {
        let mut body = individual_form("no");
        body.extend_from_slice(b"&organizationName=Example+Org");
        let form = PreInterestForm::parse(&body).unwrap();
        assert!(form
            .into_request(
                RegistrationHost::User,
                Uuid::nil(),
                "2026-09-01T15:00:00.000Z".into(),
            )
            .is_err());

        let form = PreInterestForm::parse(&body).unwrap();
        assert!(form
            .into_request(
                RegistrationHost::Organization,
                Uuid::nil(),
                "2026-09-01T15:00:00.000Z".into(),
            )
            .is_ok());
    }
}
