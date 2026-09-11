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

#[cfg(test)]
mod tests {
    use super::{health, info};
    use axum::Json;
    use serde_json::json;

    #[tokio::test]
    async fn health_uses_the_generated_interface_shape() {
        let Json(response) = health().await;
        assert_eq!(
            serde_json::to_value(response).unwrap(),
            json!({"status": "ok", "service": "canonical-web-server"})
        );
    }

    #[tokio::test]
    async fn info_uses_the_generated_interface_shape() {
        let Json(response) = info().await;
        let value = serde_json::to_value(response).unwrap();
        assert_eq!(value["service"], "canonical-web-server");
        assert_eq!(value["domain"], "canonical.cloud");
        assert_eq!(
            value["stack"],
            json!(["supabase", "maud", "axum", "seaorm", "htmx"])
        );
    }
}
