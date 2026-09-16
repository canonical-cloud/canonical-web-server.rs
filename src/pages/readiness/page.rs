use ores_api_docs_client::{PageContext, PageDocument, PageResult};

#[ores_api_docs_macros::ores_page(
    renderer = "mash",
    delivery = "ssr_only",
    render = "static_only",
    title = "Readiness",
    summary = "Customer-facing compliance readiness overview.",
    auth = "session",
    stability = "experimental",
    database = "none",
    features("readiness.overview"),
    tags("canonical", "readiness")
)]
pub async fn page(_ctx: PageContext) -> PageResult {
    Ok(PageDocument::html(
        "<!doctype html><html><head><title>Readiness</title></head><body><main><h1>Readiness</h1><p>Compliance readiness overview.</p></main></body></html>",
    ))
}
