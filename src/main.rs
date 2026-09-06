use canonical_config::flags::{self, Contract};
use canonical_web_server::{command, telemetry, SERVICE};
use tracing::Instrument as _;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let raw_command = std::env::args().nth(1);
    let command = raw_command
        .as_deref()
        .filter(|value| matches!(*value, "serve" | "migrate"));
    if let Some(output) =
        flags::process_control(Contract::Web, SERVICE, env!("CARGO_PKG_VERSION"), command)
            .map_err(std::io::Error::other)?
    {
        print!("{output}");
        return Ok(());
    }
    let _telemetry = telemetry::init(SERVICE, "canonical-cloud");
    let service_span = tracing::info_span!(
        "service.run",
        service.name = SERVICE,
        service.namespace = "canonical-cloud",
    );
    command::run(command).instrument(service_span).await
}
