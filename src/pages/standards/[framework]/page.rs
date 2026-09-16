use ores_api_docs_client::{PageContext, PageDocument, PageResult};

#[ores_api_docs_macros::ores_page(
    renderer = "mash",
    delivery = "ssr_only",
    render = "static_with_fallback",
    title = "Standard readiness",
    summary = "Readiness detail for one compliance framework.",
    auth = "session",
    stability = "experimental",
    database = "none",
    features("readiness.framework"),
    tags("canonical", "standards")
)]
pub async fn page(ctx: PageContext) -> PageResult {
    let framework = ctx
        .route_params
        .get("framework")
        .map(String::as_str)
        .unwrap_or("unknown");
    let framework = escape_html(framework);
    Ok(PageDocument::html(format!(
        "<!doctype html><html><head><title>Standard readiness</title></head><body><main><h1>{framework}</h1><p>Framework readiness detail.</p></main></body></html>"
    )))
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
