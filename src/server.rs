//! Network listener, backplane lifecycle, and graceful shutdown.

use std::net::SocketAddr;

use sea_orm::DatabaseBackend;

use crate::{app, config::Config, error::AppError, ws, SERVICE};

// Keep canonical browser/server log correlation outside the fully assembled
// application. Existing OTLP providers, auth, CSRF, RLS and WebSocket rules are
// untouched; this dependency is not added to the isolated session revoker.
fn correlate_browser_requests(router: axum::Router) -> axum::Router {
    ores_otel_web::server::install(router, SERVICE)
}

pub async fn run(config: Config) -> Result<(), AppError> {
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
    let app = correlate_browser_requests(app::build_app(state));
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

#[cfg(test)]
mod tests {
    use super::correlate_browser_requests;
    use axum::{body::Body, http::{Request, StatusCode}, routing::get, Router};
    use ores_otel_web::TraceParent;
    use tower::ServiceExt;

    #[tokio::test]
    async fn browser_correlation_cannot_replace_an_authorization_denial() {
        let app = correlate_browser_requests(
            Router::new().route("/", get(|| async { StatusCode::UNAUTHORIZED })),
        );
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .header("traceparent", "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-00")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let trace: TraceParent = response.headers()["traceparent"].to_str().unwrap().parse().unwrap();
        assert_eq!(trace.trace_id(), "4bf92f3577b34da6a3ce929d0e0e4736");
        assert_ne!(trace.span_id(), "00f067aa0ba902b7");
        assert!(!trace.sampled());
    }
}
