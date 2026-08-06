//! Application state and HTTP router assembly.

use std::{env, sync::Arc, time::Duration};

use axum::{
    http::{header, HeaderName, HeaderValue},
    Router,
};
use tokio::sync::Semaphore;
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
    metrics, routes, telemetry, ws,
};

#[derive(Clone)]
pub(crate) struct QuoteClient {
    pub base_url: String,
    pub http: reqwest::Client,
    pub maximum_assertion_age_seconds: i64,
    pub origin_assertion_secret: Vec<u8>,
    pub web_service_token: String,
}

impl QuoteClient {
    fn from_env() -> Result<Self, AppError> {
        let raw_url = env::var("CANONICAL_API_URL")
            .map_err(|_| AppError::Configuration("CANONICAL_API_URL is required"))?;
        let parsed = reqwest::Url::parse(&raw_url)
            .map_err(|_| AppError::Configuration("CANONICAL_API_URL must be an absolute URL"))?;
        if !matches!(parsed.scheme(), "http" | "https")
            || parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
            || parsed.path() != "/"
        {
            return Err(AppError::Configuration(
                "CANONICAL_API_URL must be an HTTP(S) origin without credentials or a path",
            ));
        }
        let secret = env::var("ORIGIN_ASSERTION_SECRET")
            .map_err(|_| AppError::Configuration("ORIGIN_ASSERTION_SECRET is required"))?
            .into_bytes();
        if secret.len() < 32 {
            return Err(AppError::Configuration(
                "ORIGIN_ASSERTION_SECRET must contain at least 32 bytes",
            ));
        }
        let web_service_token = env::var("CANONICAL_WEB_SERVICE_TOKEN")
            .map_err(|_| AppError::Configuration("CANONICAL_WEB_SERVICE_TOKEN is required"))?;
        if web_service_token.len() < 32 || web_service_token.trim() != web_service_token {
            return Err(AppError::Configuration(
                "CANONICAL_WEB_SERVICE_TOKEN must contain at least 32 non-whitespace-trimmed bytes",
            ));
        }
        let maximum_assertion_age_seconds = env::var("ORIGIN_ASSERTION_MAX_AGE_SECONDS")
            .unwrap_or_else(|_| "30".to_owned())
            .parse::<i64>()
            .map_err(|_| {
                AppError::Configuration("ORIGIN_ASSERTION_MAX_AGE_SECONDS must be an integer")
            })?;
        if !(5..=60).contains(&maximum_assertion_age_seconds) {
            return Err(AppError::Configuration(
                "ORIGIN_ASSERTION_MAX_AGE_SECONDS must be between 5 and 60",
            ));
        }
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(240))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("canonical-web-server/0.1")
            .build()?;
        Ok(Self {
            base_url: parsed.origin().ascii_serialization(),
            http,
            maximum_assertion_age_seconds,
            origin_assertion_secret: secret,
            web_service_token,
        })
    }
}

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: sea_orm::DatabaseConnection,
    pub auth: Arc<dyn AuthProvider>,
    pub login_rate_limiter: auth::LoginRateLimiter,
    pub(crate) login_auth_semaphore: Arc<Semaphore>,
    pub sessions: auth::SessionService,
    pub hub: ws::Hub,
    pub(crate) bearer_auth_semaphore: Arc<Semaphore>,
    pub(crate) quote: Option<Arc<QuoteClient>>,
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
        let login_rate_limiter = auth::LoginRateLimiter::new(
            config.login_rate_limit_attempts,
            config.login_rate_limit_global_attempts,
            config.login_rate_limit_window,
            config.login_rate_limit_max_keys,
        );
        let login_auth_semaphore = Arc::new(Semaphore::new(config.login_auth_max_concurrency));
        let bearer_auth_semaphore = Arc::new(Semaphore::new(config.bearer_auth_max_concurrency));

        Ok(Self {
            config,
            db,
            auth,
            login_rate_limiter,
            login_auth_semaphore,
            sessions,
            hub: ws::Hub::new(256),
            bearer_auth_semaphore,
            quote: None,
        })
    }
}

pub async fn build_state(config: Config) -> Result<AppState, AppError> {
    let db = database::connect(&config.database_url, config.database_max_connections).await?;
    // The long-lived customer process must fail closed unless it received the
    // exact non-owner, non-BYPASSRLS runtime identity. Schema changes are an
    // explicit `migrate` command with a separately mounted credential.
    crate::db::verify_runtime_database_role(&db).await?;

    #[cfg(feature = "test-auth")]
    if auth::test_provider::BrowserTestAuth::is_enabled() {
        tracing::warn!("browser-e2e test authentication provider enabled");
        return AppState::new(config, db, Arc::new(auth::test_provider::BrowserTestAuth));
    }

    let auth = Arc::new(auth::SupabaseAuth::new(
        config.supabase_url.clone(),
        config.supabase_publishable_key.clone(),
    )?);
    let mut state = AppState::new(config, db, auth)?;
    state.quote = Some(Arc::new(QuoteClient::from_env()?));
    Ok(state)
}

pub fn build_app(state: AppState) -> Router {
    let request_id_header = HeaderName::from_static("x-request-id");
    let app = telemetry::instrument_http(routes::router(state))
        .layer(axum::middleware::from_fn(metrics::record_http));

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
        SetResponseHeaderLayer::if_not_present(
            header::X_FRAME_OPTIONS,
            HeaderValue::from_static("DENY"),
        ),
        SetResponseHeaderLayer::if_not_present(
            HeaderName::from_static("permissions-policy"),
            HeaderValue::from_static(
                "camera=(), geolocation=(), microphone=(), payment=(), usb=()",
            ),
        ),
        SetResponseHeaderLayer::if_not_present(
            HeaderName::from_static("cross-origin-opener-policy"),
            HeaderValue::from_static("same-origin"),
        ),
        // Browsers only honor HSTS when delivered over HTTPS. The public edge
        // is responsible for redirecting cleartext traffic first.
        SetResponseHeaderLayer::if_not_present(
            header::STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static("max-age=31536000"),
        ),
        CompressionLayer::new(),
    ))
}
