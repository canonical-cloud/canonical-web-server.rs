use canonical_auth::SupabaseAuth;
use canonical_config::flags::{self, Contract};
use canonical_config::SessionRevokerConfig;
use canonical_session::SessionRevoker;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let raw_command = std::env::args().nth(1);
    let command = raw_command
        .as_deref()
        .filter(|value| matches!(*value, "run" | "check"));
    if let Some(output) = flags::process_control(
        Contract::SessionRevoker,
        "canonical-session-revoker",
        env!("CARGO_PKG_VERSION"),
        command,
    )
    .map_err(std::io::Error::other)?
    {
        print!("{output}");
        return Ok(());
    }
    tracing_subscriber::fmt()
        .with_env_filter(flags::var("RUST_LOG").ok().map_or_else(
            || "canonical_web_server=info,canonical_session_revoker=info".into(),
            tracing_subscriber::EnvFilter::new,
        ))
        .init();

    let config = SessionRevokerConfig::from_env()?;
    let db =
        canonical_store::connect_database(&config.database_url, config.database_max_connections)
            .await?;
    let auth = Arc::new(SupabaseAuth::new(
        config.supabase_url,
        config.supabase_publishable_key,
    )?);
    let revoker = SessionRevoker::new(db.clone(), auth, &config.session_encryption_key)?;
    revoker.verify_database_role().await?;

    match command {
        None | Some("run") => {
            let worker = revoker.spawn();
            tracing::info!(
                service = "canonical-session-revoker",
                "session revoker started"
            );
            shutdown_signal().await;
            worker.shutdown().await;
        }
        Some("check") => {}
        Some(command) => {
            return Err(format!("unknown command {command:?}; expected `run` or `check`").into());
        }
    }
    drop(revoker);
    db.close().await?;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            tracing::error!(%error, "failed to install Ctrl+C handler");
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(error) => tracing::error!(%error, "failed to install SIGTERM handler"),
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
}
