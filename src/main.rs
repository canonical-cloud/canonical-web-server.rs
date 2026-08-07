use canonical_web_server::{command, telemetry, SERVICE};
use tracing::Instrument as _;

const INTERNAL_AUTH_TOKEN_ENV: &str = "CANONICAL_INTERNAL_AUTH_TOKEN";
const LEGACY_WEB_SERVICE_TOKEN_ENV: &str = "CANONICAL_WEB_SERVICE_TOKEN";

fn install_internal_auth_token_compatibility_alias() {
    if std::env::var_os(INTERNAL_AUTH_TOKEN_ENV).is_none() {
        if let Some(token) = std::env::var_os(LEGACY_WEB_SERVICE_TOKEN_ENV) {
            // Preserve compatibility during the web-to-API cutover without
            // weakening precedence: the reviewed internal-token variable
            // always wins when both names are present, and no token is logged.
            std::env::set_var(INTERNAL_AUTH_TOKEN_ENV, token);
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    install_internal_auth_token_compatibility_alias();
    let _telemetry = telemetry::init(SERVICE, "canonical-cloud");
    let service_span = tracing::info_span!(
        "service.run",
        service.name = SERVICE,
        service.namespace = "canonical-cloud",
    );
    let command = std::env::args().nth(1);
    command::run(command.as_deref())
        .instrument(service_span)
        .await
}
