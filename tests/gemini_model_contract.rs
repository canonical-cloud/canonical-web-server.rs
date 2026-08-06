const QUOTE_SOURCE: &str = include_str!("../src/quotes.rs");

#[test]
fn default_quote_model_stays_on_the_verified_published_pro_model() {
    assert!(
        QUOTE_SOURCE.contains(
            "const DEFAULT_GEMINI_MODEL: &str = \"gemini-3.1-pro-preview\";"
        ),
        "the default model must remain the verified published Gemini Pro model; use GEMINI_MODEL for an explicitly enabled alternative"
    );
    assert!(
        !QUOTE_SOURCE.contains("gemini-3.6-pro"),
        "do not silently switch the default to an unverified model identifier"
    );
}
