use crate::{
    auth::{
        require_csrf, require_origin, Authenticated, CredentialSource, SessionAuthenticated,
    },
    error::AppError,
    quotes::{self, QuoteRecord, QuoteRequest},
    AppState,
};
use axum::{
    extract::{ws::WebSocketUpgrade, Path, Query, State},
    http::HeaderMap,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuoteListQuery {
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    20
}

pub async fn list(
    State(state): State<AppState>,
    Authenticated(actor): Authenticated,
    Query(query): Query<QuoteListQuery>,
) -> Result<Json<Vec<QuoteRecord>>, AppError> {
    Ok(Json(
        quotes::list_quotes(&state, actor.user_id, query.limit).await?,
    ))
}

pub async fn get(
    State(state): State<AppState>,
    Authenticated(actor): Authenticated,
    Path(quote_id): Path<Uuid>,
) -> Result<Json<QuoteRecord>, AppError> {
    Ok(Json(
        quotes::get_quote(&state, actor.user_id, quote_id).await?,
    ))
}

pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Authenticated(actor): Authenticated,
    Json(request): Json<QuoteRequest>,
) -> Result<Response, AppError> {
    if actor.source == CredentialSource::SessionCookie {
        require_origin(&headers, &state)?;
        require_csrf(&actor, &headers, None)?;
    }
    let record = quotes::create_quote(state, actor.user_id, request).await?;
    Ok((axum::http::StatusCode::ACCEPTED, Json(record)).into_response())
}

pub async fn websocket(
    State(state): State<AppState>,
    headers: HeaderMap,
    auth: Result<SessionAuthenticated, AppError>,
    upgrade: WebSocketUpgrade,
) -> Response {
    let actor = match auth {
        Ok(SessionAuthenticated(actor)) => actor,
        Err(error) => return error.into_response(),
    };
    if let Err(error) = require_origin(&headers, &state) {
        return error.into_response();
    }
    let permit = match state.hub.try_acquire_socket(actor.user_id) {
        Some(permit) => permit,
        None => {
            return AppError::RateLimited {
                retry_after_seconds: 60,
            }
            .into_response()
        }
    };
    upgrade
        .protocols(["canonical.quote.v1"])
        .max_message_size(64 * 1024)
        .max_frame_size(64 * 1024)
        .on_upgrade(move |socket| async move {
            let _permit = permit;
            quotes::serve_websocket(socket, actor.user_id).await;
        })
}
