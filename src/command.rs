//! Process command dispatch kept separate from the binary entry point.

use crate::{config::Config, env_compat, server};

pub async fn run(command: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    env_compat::install_internal_auth_token_alias();
    match command {
        None | Some("serve") => server::run(Config::from_env()?).await?,
        Some("migrate") => {
            return Err(
                "database migrations are owned by canonical-orm-core; run the separate canonical-orm-migrate executable with the dedicated migrator identity"
                    .into(),
            );
        }
        Some(command) => {
            return Err(format!("unknown command {command:?}; expected `serve`").into());
        }
    }
    Ok(())
}
