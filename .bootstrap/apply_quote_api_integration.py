from pathlib import Path


def replace_once(path: Path, old: str, new: str) -> None:
    source = path.read_text()
    if source.count(old) != 1:
        raise SystemExit(f"{path}: expected exactly one occurrence of {old!r}")
    path.write_text(source.replace(old, new))


app = Path("src/app.rs")
replace_once(
    app,
    "    pub shared_auth: auth::SharedAuthVerifier,\n    pub hub: ws::Hub,\n",
    "    pub shared_auth: auth::SharedAuthVerifier,\n    pub(crate) quote_api: Option<Arc<crate::quote_api::QuoteApiClient>>,\n    pub hub: ws::Hub,\n",
)
replace_once(
    app,
    "            shared_auth,\n            hub: ws::Hub::new(256),\n",
    "            shared_auth,\n            quote_api: None,\n            hub: ws::Hub::new(256),\n",
)
replace_once(
    app,
    """    #[cfg(feature = "test-auth")]
    if auth::test_provider::BrowserTestAuth::is_enabled() {
        tracing::warn!("browser-e2e test authentication provider enabled");
        return AppState::new(config, db, Arc::new(auth::test_provider::BrowserTestAuth));
    }

    let auth = Arc::new(auth::SupabaseAuth::new(
        config.supabase_url.clone(),
        config.supabase_publishable_key.clone(),
    )?);
    AppState::new(config, db, auth)
""",
    """    #[cfg(feature = "test-auth")]
    if auth::test_provider::BrowserTestAuth::is_enabled() {
        tracing::warn!("browser-e2e test authentication provider enabled");
        let mut state =
            AppState::new(config, db, Arc::new(auth::test_provider::BrowserTestAuth))?;
        state.quote_api = Some(Arc::new(crate::quote_api::QuoteApiClient::from_env()?));
        return Ok(state);
    }

    let auth = Arc::new(auth::SupabaseAuth::new(
        config.supabase_url.clone(),
        config.supabase_publishable_key.clone(),
    )?);
    let mut state = AppState::new(config, db, auth)?;
    state.quote_api = Some(Arc::new(crate::quote_api::QuoteApiClient::from_env()?));
    Ok(state)
""",
)

lib = Path("src/lib.rs")
replace_once(
    lib,
    """// The quote workflow remains in this process temporarily while the dedicated
// `canonical-api-server.rs` service takes over this boundary.
#[allow(dead_code, unused_imports)]
pub mod quotes;
pub mod routes;
""",
    """pub mod quote_api;
pub mod routes;
""",
)

error = Path("src/error.rs")
replace_once(
    error,
    "    #[error(\"upstream authentication service failed\")]\n    AuthUpstream,\n",
    "    #[error(\"upstream authentication service failed\")]\n    AuthUpstream,\n    #[error(\"upstream application service failed\")]\n    ServiceUpstream,\n",
)
replace_once(
    error,
    """            Self::AuthUpstream => (
                StatusCode::SERVICE_UNAVAILABLE,
                "auth_upstream_unavailable",
                "authentication service is temporarily unavailable",
            ),
            Self::RateLimited { .. } => (
""",
    """            Self::AuthUpstream => (
                StatusCode::SERVICE_UNAVAILABLE,
                "auth_upstream_unavailable",
                "authentication service is temporarily unavailable",
            ),
            Self::ServiceUpstream => (
                StatusCode::SERVICE_UNAVAILABLE,
                "service_upstream_unavailable",
                "quote analysis is temporarily unavailable",
            ),
            Self::RateLimited { .. } => (
""",
)

api_routes = Path("src/routes/api/mod.rs")
replace_once(api_routes, "mod quotes;\n\n", "")
for route in [
    '        .route("/quotes", get(quotes::list).post(quotes::create))\n',
    '        .route("/quotes/ws", get(quotes::websocket))\n',
    '        .route("/quotes/{id}", get(quotes::get))\n',
]:
    replace_once(api_routes, route, "")

quote_route = Path("src/routes/quote.rs")
for old, new in [
    ('("nist_csf", self.nist_csf),', '("nist-csf", self.nist_csf),'),
    ('("nist_800_53", self.nist_800_53),', '("nist-800-53", self.nist_800_53),'),
    ('("iso_27001", self.iso_27001),', '("iso-27001", self.iso_27001),'),
    ('("pci_dss", self.pci_dss),', '("pci-dss", self.pci_dss),'),
]:
    replace_once(quote_route, old, new)
replace_once(
    quote_route,
    "            (\"fedramp\", self.fedramp),\n",
    "            (\"fedramp\", self.fedramp),\n            (\"gdpr\", self.gdpr),\n",
)
replace_once(
    quote_route,
    "    #[serde(default)]\n    fedramp: Option<String>,\n",
    "    #[serde(default)]\n    fedramp: Option<String>,\n    #[serde(default)]\n    gdpr: Option<String>,\n",
)
replace_once(
    quote_route,
    "            fedramp: None,\n            csrf: \"token\".into(),\n",
    "            fedramp: None,\n            gdpr: None,\n            csrf: \"token\".into(),\n",
)

env_file = Path(".env.example")
replace_once(
    env_file,
    "CANONICAL_WEB_SERVICE_TOKEN=replace-with-at-least-32-random-bytes\n",
    "CANONICAL_INTERNAL_AUTH_TOKEN=replace-with-at-least-32-random-bytes\nCANONICAL_CONTEXT_RECORD_ID=00000000-0000-0000-0000-000000000000\n",
)

readme = Path("README.md")
replace_once(
    readme,
    """- `src/quote_api.rs` — bounded client and Maud views for the separately deployed
  `canonical-api-server.rs`; it sends a verified user id under a dedicated
  service credential and never exposes Gemini or database credentials.
""",
    """- `src/quote_api.rs` — bounded client and Maud views for the separately deployed
  `canonical-api-server.rs`; it sends the Shared Auth subject under
  `x-canonical-subject`, authenticates with `CANONICAL_INTERNAL_AUTH_TOKEN`,
  fixes `CANONICAL_CONTEXT_RECORD_ID` server-side, and never exposes Gemini or
  database credentials.
""",
)
replace_once(
    readme,
    """`/api/health` and `/api/info` remain compatibility aliases. Unknown API and
application paths have JSON and HTML 404s respectively rather than falling
through to the marketing SPA.
""",
    """`/api/health` and `/api/info` remain compatibility aliases. Unknown API and
application paths have JSON and HTML 404s respectively rather than falling
through to the marketing SPA.

The `/u/quote` handlers verify the host-only Shared Auth session at the origin,
then call the dedicated API over its private Kubernetes origin. Browser input
cannot choose the internal service token, authenticated subject, Canonical
context record, application Markdown, Gemini key, or Gemini model.
""",
)

model_contract = Path("tests/gemini_model_contract.rs")
model_contract.write_text(
    """const QUOTE_CLIENT_SOURCE: &str = include_str!(\"../src/quote_api.rs\");
const LIB_SOURCE: &str = include_str!(\"../src/lib.rs\");

#[test]
fn browser_tier_delegates_quote_analysis_without_gemini_credentials() {
    assert!(LIB_SOURCE.contains(\"pub mod quote_api;\"));
    assert!(!LIB_SOURCE.contains(\"pub mod quotes;\"));
    assert!(QUOTE_CLIENT_SOURCE.contains(\"CANONICAL_API_URL\"));
    assert!(QUOTE_CLIENT_SOURCE.contains(\"CANONICAL_INTERNAL_AUTH_TOKEN\"));
    assert!(QUOTE_CLIENT_SOURCE.contains(\"CANONICAL_CONTEXT_RECORD_ID\"));
    assert!(QUOTE_CLIENT_SOURCE.contains(\"x-canonical-subject\"));
    assert!(!QUOTE_CLIENT_SOURCE.contains(\"GEMINI_API_KEY\"));
    assert!(!QUOTE_CLIENT_SOURCE.contains(\"generateContent\"));
}
"""
)

for obsolete in [Path("src/quotes.rs"), Path("src/routes/api/quotes.rs")]:
    obsolete.unlink(missing_ok=False)
