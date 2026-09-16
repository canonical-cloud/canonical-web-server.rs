use ores_api_docs_client::{PrerenderContext, PrerenderResult};

#[ores_api_docs_macros::ores_generate]
pub async fn generate_static_params(_ctx: PrerenderContext) -> PrerenderResult {
    // StaticWithFallback permits an empty deterministic seed set. Production
    // builds can later inject a versioned framework catalog through ctx.state.
    Ok(Vec::new())
}
