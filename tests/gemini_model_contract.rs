const QUOTE_SOURCE: &str = include_str!("../src/quotes.rs");

#[test]
fn default_quote_model_stays_on_published_pro_endpoint() {
    assert!(
        QUOTE_SOURCE.contains(
            "const DEFAULT_GEMINI_MODEL: &str = \"gemini-3.1-pro-preview\";"
        ),
        "the default model must remain on the published Gemini Pro endpoint; use GEMINI_MODEL for an explicit runtime override"
    );
    assert!(
        !QUOTE_SOURCE.contains("gemini-3.6-pro"),
        "do not configure an unpublished Gemini model identifier"
    );
    assert!(
        QUOTE_SOURCE.contains("std::env::var(\"GEMINI_MODEL\")"),
        "the runtime model override must remain available"
    );
}
