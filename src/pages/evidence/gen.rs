use ores_api_docs_client::{PrerenderContext, PrerenderPath, PrerenderResult};

#[ores_api_docs_macros::ores_generate]
pub async fn generate_static_params(_ctx: PrerenderContext) -> PrerenderResult {
    Ok(vec![PrerenderPath::default()])
}
