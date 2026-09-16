use ores_api_docs_client::{PageContext, PageDocument, PageResult};

#[ores_api_docs_macros::ores_page(
    renderer = "mash",
    delivery = "ssr_only",
    render = "static_only",
    title = "Evidence",
    summary = "Evidence collection and audit artifact overview.",
    auth = "session",
    stability = "experimental",
    database = "none",
    features("evidence.overview"),
    tags("canonical", "evidence")
)]
pub async fn page(_ctx: PageContext) -> PageResult {
    Ok(PageDocument::html(
        "<!doctype html><html><head><title>Evidence</title></head><body><main><h1>Evidence</h1><p>Evidence collection and audit artifacts.</p></main></body></html>",
    ))
}
