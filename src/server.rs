//! Network listener, backplane lifecycle, and graceful shutdown.

use std::net::SocketAddr;

use canonical_lib::audit_data::table;
use canonical_orm_core::{CapabilityProfile, DualOrmContext};
use sea_orm::DatabaseBackend;

use crate::{app, config::Config, error::AppError, ws, SERVICE};

const ADMIN_DATABASE_URL_ENV: &str = "CANONICAL_ADMIN_DATABASE_URL";
const AUDIT_DATABASE_URL_ENV: &str = "CANONICAL_AUDIT_DATABASE_URL";

pub async fn run(config: Config) -> Result<(), AppError> {
    // The web/session database and the audit-domain database deliberately use
    // incompatible least-privilege identities. The local web store requires
    // the exact `canonical_web_server` login with no role memberships, while
    // canonical-orm-core requires a `WebReadOnly` audit capability. Never make
    // one credential satisfy both boundaries.
    if std::env::var_os(ADMIN_DATABASE_URL_ENV).is_some() {
        return Err(AppError::Configuration(
            "CANONICAL_ADMIN_DATABASE_URL belongs to the isolated admin plane and is forbidden in the customer web service",
        ));
    }
    let audit_database_url = std::env::var(AUDIT_DATABASE_URL_ENV)
        .map_err(|_| AppError::Configuration("CANONICAL_AUDIT_DATABASE_URL is required"))?;
    let audit_database_url = audit_database_url.trim();
    if audit_database_url.is_empty() {
        return Err(AppError::Configuration(
            "CANONICAL_AUDIT_DATABASE_URL must not be empty",
        ));
    }
    if audit_database_url == config.database_url.trim() {
        return Err(AppError::Configuration(
            "CANONICAL_AUDIT_DATABASE_URL must not equal the web/session DATABASE_URL",
        ));
    }

    let dual_orm =
        DualOrmContext::connect_read_only(audit_database_url, CapabilityProfile::WebReadOnly)
            .await?;
    dual_orm.ping_both().await?;
    dual_orm.assert_catalog_congruence().await?;
    tracing::info!(tenant_table = table::TENANTS, "audit dual ORM catalog verified");

    let port = config.port;
    // `config.database_url` remains exclusively the web/session database. Its
    // own startup path verifies the exact canonical_web_server identity before
    // returning application state.
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
