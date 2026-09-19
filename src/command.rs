//! Process command dispatch kept separate from the binary entry point.

use crate::{
    config::{Config, MigrationConfig},
    database, env_compat, server,
};

pub async fn run(command: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    env_compat::install_internal_auth_token_alias();
    match command {
        Some("migrate") => {
            // Still owns the web-only tables (sessions, profiles, sync, admin,
            // legacy engagements). canonical-orm-core owns the B2B audit chain
            // in `canonical_orm_migrations`; retire this only once these tables
            // are adopted there as a baseline, or a fresh database cannot be built.
            let config = MigrationConfig::from_env()?;
            database::run_migrations(&config.database_url, config.database_max_connections).await?;
            tracing::info!("database migrations complete");
        }
        None | Some("serve") => server::run(Config::from_env()?).await?,
        Some(command) => {
            return Err(
                format!("unknown command {command:?}; expected `serve` or `migrate`").into(),
            );
        }
    }
    Ok(())
}
