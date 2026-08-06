const QUOTE_CLIENT_SOURCE: &str = include_str!("../src/quote_api.rs");
const LIB_SOURCE: &str = include_str!("../src/lib.rs");

#[test]
fn browser_tier_delegates_quote_analysis_without_gemini_credentials() {
    assert!(LIB_SOURCE.contains("pub mod quote_api;"));
    assert!(!LIB_SOURCE.contains("pub mod quotes;"));
    assert!(QUOTE_CLIENT_SOURCE.contains("CANONICAL_API_URL"));
    assert!(QUOTE_CLIENT_SOURCE.contains("CANONICAL_INTERNAL_AUTH_TOKEN"));
    assert!(!QUOTE_CLIENT_SOURCE.contains("CANONICAL_CONTEXT_RECORD_ID"));
    assert!(QUOTE_CLIENT_SOURCE.contains("x-canonical-subject"));
    assert!(!QUOTE_CLIENT_SOURCE.contains("GEMINI_API_KEY"));
    assert!(!QUOTE_CLIENT_SOURCE.contains("generateContent"));
}
