//! Session-protected, in-memory readiness worksheets. No evidence upload or probe API.

use crate::{auth::SessionAuthenticated, error::AppError, AppState};
use axum::{
    extract::Path,
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Redirect, Response},
    routing::get,
    Router,
};

const HTML: &str = include_str!("../../vendor/canonical-auditor-readiness/readiness/index.html");
const CATALOG: &str =
    include_str!("../../vendor/canonical-auditor-readiness/readiness/catalog.json");
const CONTRACT: &str =
    include_str!("../../vendor/canonical-auditor-readiness/readiness/readiness.mjs");
const BROWSER: &str =
    include_str!("../../vendor/canonical-auditor-readiness/readiness/browser.mjs");
const CSS: &str = include_str!("../../vendor/canonical-auditor-readiness/readiness/readiness.css");
const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'; object-src 'none'";
const FRAMEWORKS: [&str; 15] = [
    "soc2",
    "nist-csf2",
    "iso27001",
    "gdpr",
    "hipaa",
    "pci-dss",
    "cis-controls",
    "csa-ccm",
    "iso27701",
    "iso42001",
    "nist-80053",
    "nist-800171",
    "nis2",
    "dora",
    "fedramp-rev5",
];

/// Complete paths relative to /app, avoiding ambiguous nested-root redirects.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/readiness", get(index))
        .route("/readiness/", get(index))
        .route("/readiness/{framework}", get(worksheet))
        .route("/readiness/assets/{name}", get(asset))
}

fn with_session(
    auth: Result<SessionAuthenticated, AppError>,
    render: impl FnOnce() -> Response,
) -> Response {
    match auth {
        Ok(_) => render(),
        Err(AppError::Unauthorized) => Redirect::to("/login").into_response(),
        Err(error) => error.into_response(),
    }
}

fn document(content_type: &'static str, contents: &'static str) -> Response {
    let mut response = contents.into_response();
    for (name, value) in [
        (header::CONTENT_TYPE, content_type),
        (header::CONTENT_SECURITY_POLICY, CSP),
        (header::CACHE_CONTROL, "no-store"),
        (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        (header::REFERRER_POLICY, "no-referrer"),
    ] {
        response
            .headers_mut()
            .insert(name, HeaderValue::from_static(value));
    }
    response
}

async fn index(auth: Result<SessionAuthenticated, AppError>) -> Response {
    with_session(auth, || document("text/html; charset=utf-8", HTML))
}

async fn worksheet(
    auth: Result<SessionAuthenticated, AppError>,
    Path(framework): Path<String>,
) -> Response {
    with_session(auth, || {
        if FRAMEWORKS.contains(&framework.as_str()) {
            document("text/html; charset=utf-8", HTML)
        } else {
            StatusCode::NOT_FOUND.into_response()
        }
    })
}

fn asset_document(name: &str) -> Option<Response> {
    let (content_type, contents) = match name {
        "catalog.json" => ("application/json", CATALOG),
        "readiness.mjs" => ("text/javascript; charset=utf-8", CONTRACT),
        "browser.mjs" => ("text/javascript; charset=utf-8", BROWSER),
        "readiness.css" => ("text/css; charset=utf-8", CSS),
        _ => return None,
    };
    Some(document(content_type, contents))
}

async fn asset(auth: Result<SessionAuthenticated, AppError>, Path(name): Path<String>) -> Response {
    with_session(auth, || {
        asset_document(&name).unwrap_or_else(|| StatusCode::NOT_FOUND.into_response())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_cover_exactly_the_pinned_catalog() {
        let catalog: serde_json::Value = serde_json::from_str(CATALOG).unwrap();
        let frameworks = catalog["frameworks"].as_array().unwrap();
        assert_eq!(frameworks.len(), FRAMEWORKS.len());
        for framework in frameworks {
            assert!(FRAMEWORKS.contains(&framework["id"].as_str().unwrap()));
            assert_eq!(framework["questions"].as_array().unwrap().len(), 10);
        }
    }

    #[test]
    fn embedded_assets_have_strict_headers_and_an_exact_allowlist() {
        for name in [
            "catalog.json",
            "readiness.mjs",
            "browser.mjs",
            "readiness.css",
        ] {
            let response = asset_document(name).unwrap();
            assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
            assert_eq!(response.headers()[header::CONTENT_SECURITY_POLICY], CSP);
            assert_eq!(
                response.headers()[header::X_CONTENT_TYPE_OPTIONS],
                "nosniff"
            );
        }
        for name in [
            "../Cargo.toml",
            "runtime-probe.mjs",
            "context.example.json",
            "unknown",
        ] {
            assert!(asset_document(name).is_none());
        }
        assert!(!CSP.contains("unsafe-inline"));
        assert!(!CSP.contains("unsafe-eval"));
    }

    #[tokio::test]
    async fn every_handler_refuses_an_unauthenticated_session() {
        let responses = [
            index(Err(AppError::Unauthorized)).await,
            worksheet(Err(AppError::Unauthorized), Path("soc2".to_owned())).await,
            asset(Err(AppError::Unauthorized), Path("catalog.json".to_owned())).await,
        ];
        for response in responses {
            assert_eq!(response.status(), StatusCode::SEE_OTHER);
            assert_eq!(response.headers()[header::LOCATION], "/login");
        }
    }
}
