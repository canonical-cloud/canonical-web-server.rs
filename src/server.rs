//! Network listener, backplane lifecycle, and graceful shutdown.

use std::net::SocketAddr;

use canonical_lib::audit_data::table;
use canonical_orm_core::{CapabilityProfile, DualOrmContext};
use sea_orm::DatabaseBackend;

use crate::{app, config::Config, error::AppError, ws, SERVICE};

const ADMIN_DATABASE_URL_ENV: &str = "CANONICAL_ADMIN_DATABASE_URL";
const AUDIT_DATABASE_URL_ENV: &str = "CANONICAL_AUDIT_DATABASE_URL";

pub async fn run(config: Config) -> Result<(), AppError> {
    if std::env::var_os(ADMIN_DATABASE_URL_ENV).is_some() {
        return Err(AppError::Configuration(
            "CANONICAL_ADMIN_DATABASE_URL belongs to the isolated admin plane and is forbidden in the customer web process",
        ));
    }

    let audit_database_url = std::env::var(AUDIT_DATABASE_URL_ENV).map_err(|_| {
        AppError::Configuration(
            "CANONICAL_AUDIT_DATABASE_URL is required and must use the dedicated audit-plane read-only credential",
        )
    })?;
    let audit_database_url = validate_audit_database_url(&config.database_url, &audit_database_url)?;

    let dual_orm =
        DualOrmContext::connect_read_only(audit_database_url, CapabilityProfile::WebReadOnly)
            .await?;
    dual_orm.ping_both().await?;
    dual_orm.assert_catalog_congruence().await?;
    tracing::info!(tenant_table = table::TENANTS, "dual ORM audit catalog verified");

    let port = config.port;
    let state = app::build_state(config).await?;
    let _backplane = if state.db.get_database_backend() == DatabaseBackend::Postgres {
        Some(ws::spawn_postgres_backplane(
            state.config.database_url.clone(),
            state.hub.clone(),
        ))
    } else {
        None
    };
    let app = app::build_app(state);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, service.name = SERVICE, "web server listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

fn validate_audit_database_url<'a>(
    customer_database_url: &str,
    audit_database_url: &'a str,
) -> Result<&'a str, AppError> {
    let audit_database_url = audit_database_url.trim();
    if audit_database_url.is_empty() {
        return Err(AppError::Configuration(
            "CANONICAL_AUDIT_DATABASE_URL must not be empty",
        ));
    }
    if audit_database_url == customer_database_url.trim() {
        return Err(AppError::Configuration(
            "CANONICAL_AUDIT_DATABASE_URL must not reuse DATABASE_URL; the legacy web-store login and audit-plane capability login have incompatible least-privilege contracts",
        ));
    }
    Ok(audit_database_url)
}

pub(crate) async fn shutdown_signal() {
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

#[cfg(test)]
mod tests {
    use super::validate_audit_database_url;

    #[test]
    fn audit_database_credential_is_required_to_be_distinct() {
        assert!(validate_audit_database_url("postgres://web@db/customer", "").is_err());
        assert!(
            validate_audit_database_url(
                "postgres://web@db/customer",
                "postgres://web@db/customer"
            )
            .is_err()
        );
        assert_eq!(
            validate_audit_database_url(
                "postgres://web@db/customer",
                " postgres://audit_ro@db/customer "
            )
            .expect("distinct audit credential"),
            "postgres://audit_ro@db/customer"
        );
    }
}
