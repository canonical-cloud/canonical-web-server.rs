from pathlib import Path


def replace_once(path: Path, old: str, new: str) -> None:
    source = path.read_text()
    if source.count(old) != 1:
        raise SystemExit(f"{path}: expected exactly one occurrence of {old!r}")
    path.write_text(source.replace(old, new))


client = Path("src/quote_api.rs")
replace_once(
    client,
    """pub struct QuoteApiClient {
    base_url: String,
    context_record_id: Uuid,
    http: Client,
""",
    """pub struct QuoteApiClient {
    base_url: String,
    http: Client,
""",
)
replace_once(
    client,
    """        let context_record_id = env::var("CANONICAL_CONTEXT_RECORD_ID")
            .map_err(|_| AppError::BadRequest("CANONICAL_CONTEXT_RECORD_ID is required".into()))?
            .parse::<Uuid>()
            .map_err(|_| {
                AppError::BadRequest("CANONICAL_CONTEXT_RECORD_ID must be a UUID".into())
            })?;

""",
    "",
)
replace_once(
    client,
    """        Ok(Self {
            base_url: parsed.origin().ascii_serialization(),
            context_record_id,
            http,
""",
    """        Ok(Self {
            base_url: parsed.origin().ascii_serialization(),
            http,
""",
)
replace_once(
    client,
    """        let payload = ApiCreateQuoteRequest {
            context_record_id: self.context_record_id,
            frameworks: &request.frameworks,
""",
    """        let payload = ApiCreateQuoteRequest {
            frameworks: &request.frameworks,
""",
)
replace_once(
    client,
    """struct ApiCreateQuoteRequest<'a> {
    context_record_id: Uuid,
    frameworks: &'a [String],
""",
    """struct ApiCreateQuoteRequest<'a> {
    frameworks: &'a [String],
""",
)
replace_once(
    client,
    """    #[test]
    fn maps_the_durable_api_record() {
""",
    """    #[test]
    fn browser_payload_cannot_select_a_database_context() {
        let frameworks = vec!["soc2".to_owned()];
        let payload = ApiCreateQuoteRequest {
            frameworks: &frameworks,
            notes: None,
            organization: ApiOrganization {
                employee_count: 10,
                industry: "Software",
                legal_name: "Example",
            },
        };
        let value = serde_json::to_value(payload).unwrap();
        assert!(value.get("context_record_id").is_none());
        assert!(value.get("markdown_context").is_none());
    }

    #[test]
    fn maps_the_durable_api_record() {
""",
)

env_file = Path(".env.example")
replace_once(
    env_file,
    "# Dedicated quote backend. All three values are required when the serve process\n",
    "# Dedicated quote backend. Both values are required when the serve process\n",
)
replace_once(
    env_file,
    "CANONICAL_CONTEXT_RECORD_ID=00000000-0000-0000-0000-000000000000\n",
    "",
)

readme = Path("README.md")
replace_once(
    readme,
    """  `x-canonical-subject`, authenticates with `CANONICAL_INTERNAL_AUTH_TOKEN`,
  fixes `CANONICAL_CONTEXT_RECORD_ID` server-side, and never exposes Gemini or
  database credentials.
""",
    """  `x-canonical-subject`, authenticates with `CANONICAL_INTERNAL_AUTH_TOKEN`,
  and never exposes or selects the owner-scoped database context, Gemini, or
  database credentials.
""",
)
replace_once(
    readme,
    """cannot choose the internal service token, authenticated subject, Canonical
context record, application Markdown, Gemini key, or Gemini model.
""",
    """cannot choose the internal service token, authenticated subject, Canonical
context record, application Markdown, Gemini key, or Gemini model. The API
selects the authenticated owner's single active context row.
""",
)

contract = Path("tests/gemini_model_contract.rs")
replace_once(
    contract,
    '    assert!(QUOTE_CLIENT_SOURCE.contains("CANONICAL_CONTEXT_RECORD_ID"));\n',
    '    assert!(!QUOTE_CLIENT_SOURCE.contains("CANONICAL_CONTEXT_RECORD_ID"));\n',
)
