const QUOTE_SOURCE: &str = include_str!("../src/quotes.rs");

#[test]
fn default_quote_model_stays_on_operator_selected_pro_model() {
    assert!(
        QUOTE_SOURCE.contains("const DEFAULT_GEMINI_MODEL: &str = \"gemini-3.6-pro\";"),
        "the default model must remain the operator-selected Gemini 3.6 Pro model; use GEMINI_MODEL for an explicit runtime override"
    );
    assert!(
        !QUOTE_SOURCE.contains("gemini-3.1-pro-preview"),
        "do not silently revert the operator-selected default model"
    );
    assert!(
        QUOTE_SOURCE.contains("std::env::var(\"GEMINI_MODEL\")"),
        "the runtime model override must remain available"
    );
}
