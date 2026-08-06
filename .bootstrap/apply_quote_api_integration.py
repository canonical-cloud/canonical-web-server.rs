from pathlib import Path


def replace_once(path: Path, old: str, new: str) -> None:
    source = path.read_text()
    if source.count(old) != 1:
        raise SystemExit(f"{path}: expected exactly one occurrence of {old!r}")
    path.write_text(source.replace(old, new))


app = Path("src/app.rs")
replace_once(
    app,
    """    #[cfg(feature = "test-auth")]
    if auth::test_provider::BrowserTestAuth::is_enabled() {
        tracing::warn!("browser-e2e test authentication provider enabled");
        return AppState::new(config, db, Arc::new(auth::test_provider::BrowserTestAuth));
    }
""",
    """    #[cfg(feature = "test-auth")]
    if auth::test_provider::BrowserTestAuth::is_enabled() {
        tracing::warn!("browser-e2e test authentication provider enabled");
        let mut state =
            AppState::new(config, db, Arc::new(auth::test_provider::BrowserTestAuth))?;
        state.quote_api = crate::quote_api::QuoteApiClient::from_env()
            .ok()
            .map(Arc::new);
        return Ok(state);
    }
""",
)

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
    "            (\"fedramp\", self.fedramp),\n            (\"pci-dss\", self.pci_dss),\n",
    "            (\"fedramp\", self.fedramp),\n            (\"pci-dss\", self.pci_dss),\n            (\"gdpr\", self.gdpr),\n",
)
replace_once(
    quote_route,
    "    #[serde(default)]\n    pci_dss: Option<String>,\n    #[serde(default)]\n    handles_phi: Option<String>,\n",
    "    #[serde(default)]\n    pci_dss: Option<String>,\n    #[serde(default)]\n    gdpr: Option<String>,\n    #[serde(default)]\n    handles_phi: Option<String>,\n",
)
replace_once(
    quote_route,
    "            pci_dss: None,\n            handles_phi: Some(\"on\".into()),\n",
    "            pci_dss: None,\n            gdpr: None,\n            handles_phi: Some(\"on\".into()),\n",
)

env_file = Path(".env.example")
replace_once(
    env_file,
    "# Dedicated quote backend. Both values are required when the serve process\n",
    "# Dedicated quote backend. All three values are required when the serve process\n",
)
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

Path("tests/gemini_model_contract.rs").write_text(
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

for obsolete in (Path("src/quotes.rs"), Path("src/routes/api/quotes.rs")):
    if obsolete.exists():
        obsolete.unlink()
