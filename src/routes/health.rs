use crate::{AppState, SERVICE};
use axum::{extract::State, http::StatusCode, Json};
use canonical_interfaces::{HealthStatus, HealthStatusStatus, ServiceInfo};

pub async fn healthz() -> StatusCode {
    StatusCode::OK
}

pub async fn readyz(State(state): State<AppState>) -> StatusCode {
    match state.db.ping().await {
        Ok(()) => StatusCode::OK,
        Err(error) => {
            tracing::warn!(%error, "database readiness check failed");
            StatusCode::SERVICE_UNAVAILABLE
        }
    }
}

pub async fn health() -> Json<HealthStatus> {
    Json(HealthStatus {
        status: HealthStatusStatus::Ok,
        service: SERVICE.to_owned(),
    })
}

pub async fn info() -> Json<ServiceInfo> {
    Json(ServiceInfo {
        service: SERVICE.to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        domain: "canonical.cloud".to_owned(),
        stack: ["supabase", "maud", "axum", "seaorm", "htmx"]
            .into_iter()
            .map(str::to_owned)
            .collect(),
    })
}
