//! Application state and HTTP router assembly.

use std::sync::Arc;

use axum::{
    http::{header, HeaderName, HeaderValue},
    Router,
};
use tower_http::{
    compression::CompressionLayer,
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    sensitive_headers::SetSensitiveRequestHeadersLayer,
    set_header::SetResponseHeaderLayer,
};

use crate::{
    auth::{self, AuthProvider},
    config::Config,
    database,
    error::AppError,
    routes, telemetry, ws,
};

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: sea_orm::DatabaseConnection,
    pub auth: Arc<dyn AuthProvider>,
    pub sessions: auth::SessionService,
    pub hub: ws::Hub,
}

impl AppState {
    pub fn new(
        config: Config,
        db: sea_orm::DatabaseConnection,
        auth: Arc<dyn AuthProvider>,
    ) -> Result<Self, AppError> {
        let config = Arc::new(config);
        let sessions = auth::SessionService::new(
            db.clone(),
            auth.clone(),
            &config.session_encryption_key,
            config.session_ttl,
        )?;

        Ok(Self {
            config,
            db,
            auth,
            sessions,
            hub: ws::Hub::new(256),
        })
    }
}

pub async fn build_state(config: Config) -> Result<AppState, AppError> {
    let db = database::connect(&config.database_url, config.database_max_connections).await?;
    let auth = Arc::new(auth::SupabaseAuth::new(
        config.supabase_url.clone(),
        config.supabase_publishable_key.clone(),
    )?);
    AppState::new(config, db, auth)
}

pub fn build_app(state: AppState) -> Router {
    let request_id_header = HeaderName::from_static("x-request-id");
    let app = telemetry::instrument_http(routes::router(state));

    app.layer((
        SetSensitiveRequestHeadersLayer::new([header::AUTHORIZATION, header::COOKIE]),
        SetRequestIdLayer::new(request_id_header.clone(), MakeRequestUuid),
        PropagateRequestIdLayer::new(request_id_header),
        SetResponseHeaderLayer::if_not_present(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ),
        SetResponseHeaderLayer::if_not_present(
            header::REFERRER_POLICY,
            HeaderValue::from_static("strict-origin-when-cross-origin"),
        ),
        CompressionLayer::new(),
    ))
}
