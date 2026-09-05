use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::response::Json;
use serde::Deserialize;
use talos_agent_api::SessionHistoryResponse;
use talos_auth::RequestIdentity;
use talos_types::{
    SessionClearRequest, SessionCloseRequest, SessionCloseResponse, SessionCompactRequest,
    SessionDeleteRequest, SessionGetRequest, SessionHistoryRequest, SessionListRequest,
    SessionPatchRequest,
};

use super::access::shared_actor;
use super::errors::{api_success, ApiError, JsonResponse};
use super::state::HttpApiState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct SessionHistoryQuery {
    cursor: Option<String>,
    limit: Option<usize>,
}

pub(super) async fn list_sessions_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Query(request): Query<SessionListRequest>,
) -> Result<JsonResponse<talos_types::SessionListResponse>, ApiError> {
    let response = state
        .gateway
        .list_sessions(&shared_actor(&actor.0), request)
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}

pub(super) async fn get_session_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Path(session_id): Path<String>,
) -> Result<JsonResponse<talos_types::SessionGetResponse>, ApiError> {
    let session = state
        .gateway
        .get_session(&shared_actor(&actor.0), SessionGetRequest { session_id })
        .await
        .map_err(map_session_error)?;

    Ok(api_success(session))
}

pub(super) async fn get_session_history_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Path(session_id): Path<String>,
    Query(query): Query<SessionHistoryQuery>,
) -> Result<JsonResponse<SessionHistoryResponse>, ApiError> {
    let response = state
        .gateway
        .get_session_history(
            &shared_actor(&actor.0),
            SessionHistoryRequest {
                session_id,
                cursor: query.cursor,
                limit: query.limit,
            },
        )
        .await
        .map_err(map_session_error)?;

    Ok(api_success(response))
}

pub(super) async fn patch_session_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Path(session_id): Path<String>,
    Json(mut request): Json<SessionPatchRequest>,
) -> Result<JsonResponse<talos_types::SessionUpdateResponse>, ApiError> {
    request.session_id = session_id;
    let response = state
        .gateway
        .patch_session(&shared_actor(&actor.0), request)
        .await
        .map_err(map_session_error)?;

    Ok(api_success(response))
}

pub(super) async fn close_session_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Path(session_id): Path<String>,
) -> Result<JsonResponse<SessionCloseResponse>, ApiError> {
    let response = state
        .gateway
        .close_session(&shared_actor(&actor.0), SessionCloseRequest { session_id })
        .await
        .map_err(map_session_error)?;
    Ok(api_success(response))
}

pub(super) async fn delete_session_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Path(session_id): Path<String>,
) -> Result<JsonResponse<talos_types::SessionDeleteResponse>, ApiError> {
    let response = state
        .gateway
        .delete_session(&shared_actor(&actor.0), SessionDeleteRequest { session_id })
        .await
        .map_err(map_session_error)?;
    Ok(api_success(response))
}

pub(super) async fn compact_session_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Path(session_id): Path<String>,
) -> Result<JsonResponse<talos_types::SessionCompactResponse>, ApiError> {
    let session = state
        .gateway
        .compact_session(
            &shared_actor(&actor.0),
            SessionCompactRequest { session_id },
        )
        .await
        .map_err(map_session_error)?;
    Ok(api_success(session))
}

pub(super) async fn clear_session_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Path(session_id): Path<String>,
) -> Result<JsonResponse<talos_types::SessionClearResponse>, ApiError> {
    let session = state
        .gateway
        .clear_session(&shared_actor(&actor.0), SessionClearRequest { session_id })
        .await
        .map_err(map_session_error)?;
    Ok(api_success(session))
}

fn map_session_error(error: talos_error::TalosError) -> ApiError {
    match error.error_code() {
        talos_error::ErrorCode::ResourceNotFound => {
            ApiError::new(talos_error::ErrorCode::SessionNotFound)
        }
        _ => ApiError::from_talos_error(error),
    }
}
