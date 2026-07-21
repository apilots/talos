use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use talos_auth::RequestIdentity;
use talos_error::TalosError;
use talos_types::{
    MemoryFileListRequest, MemoryFileReadRequest, MemoryFileWriteRequest, MemoryGetRequest,
    MemoryKind, MemorySearchRequest,
};

use super::access::shared_actor;
use super::errors::{ApiError, HttpApiErrorCode};
use super::state::HttpApiState;

#[derive(Debug, Deserialize, Default)]
pub(super) struct MemoryFilesQuery {
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    before_date: Option<String>,
    #[serde(default)]
    date_from: Option<String>,
    #[serde(default)]
    date_to: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(super) struct MemorySearchHttpQuery {
    query: String,
    #[serde(default)]
    limit: Option<u32>,
    #[serde(default)]
    kinds: Option<String>,
    #[serde(default)]
    since_at: Option<i64>,
    #[serde(default)]
    until_at: Option<i64>,
}

pub(super) async fn list_memory_files_handler(
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<RequestIdentity>,
    Query(query): Query<MemoryFilesQuery>,
) -> Result<Json<Value>, ApiError> {
    let actor = shared_actor(&auth.0);
    let request = MemoryFileListRequest {
        limit: query.limit,
        before_date: query.before_date,
        date_from: query.date_from,
        date_to: query.date_to,
    };
    let response = state
        .gateway
        .list_memory_files(&actor, request)
        .await
        .map_err(map_memory_error)?;
    Ok(Json(json!(response)))
}

pub(super) async fn get_memory_file_handler(
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<RequestIdentity>,
    Path(date): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let actor = shared_actor(&auth.0);
    let detail = state
        .gateway
        .read_memory_file(&actor, MemoryFileReadRequest { date })
        .await
        .map_err(map_memory_error)?;
    Ok(Json(json!(detail)))
}

pub(super) async fn put_memory_file_handler(
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<RequestIdentity>,
    Path(date): Path<String>,
    Json(request): Json<MemoryFileWriteRequest>,
) -> Result<Json<Value>, ApiError> {
    if request.date != date {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            HttpApiErrorCode::InvalidRequest,
            "memory file route date must match request date",
        ));
    }
    let actor = shared_actor(&auth.0);
    let detail = state
        .gateway
        .write_memory_file(&actor, request)
        .await
        .map_err(map_memory_error)?;
    Ok(Json(json!(detail)))
}

pub(super) async fn search_memory_files_handler(
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<RequestIdentity>,
    Query(query): Query<MemorySearchHttpQuery>,
) -> Result<Json<Value>, ApiError> {
    let actor = shared_actor(&auth.0);
    let request = MemorySearchRequest {
        query: query.query,
        limit: query.limit,
        kinds: query
            .kinds
            .map(|value| parse_memory_kinds(&value))
            .transpose()?,
        since_at: query.since_at,
        until_at: query.until_at,
    };
    let response = state
        .gateway
        .search_memory(&actor, request)
        .await
        .map_err(map_memory_error)?;
    Ok(Json(json!(response)))
}

fn parse_memory_kinds(value: &str) -> Result<Vec<MemoryKind>, ApiError> {
    value
        .split(',')
        .map(|kind| match kind {
            "preference" => Ok(MemoryKind::Preference),
            "fact" => Ok(MemoryKind::Fact),
            "decision" => Ok(MemoryKind::Decision),
            "task" => Ok(MemoryKind::Task),
            "episode" => Ok(MemoryKind::Episode),
            _ => Err(map_memory_error(TalosError::ValidationError(format!(
                "unknown memory kind: {kind}"
            )))),
        })
        .collect()
}

pub(super) async fn get_memory_handler(
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<RequestIdentity>,
    Path(memory_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let actor = shared_actor(&auth.0);
    let response = state
        .gateway
        .get_memory(&actor, MemoryGetRequest { memory_id })
        .await
        .map_err(map_memory_error)?;
    Ok(Json(json!(response)))
}

fn map_memory_error(error: TalosError) -> ApiError {
    match error {
        TalosError::ValidationError(message) => ApiError::new(
            StatusCode::BAD_REQUEST,
            HttpApiErrorCode::InvalidRequest,
            message,
        ),
        TalosError::ResourceNotFound(message) => ApiError::new(
            StatusCode::NOT_FOUND,
            HttpApiErrorCode::InvalidRequest,
            message,
        ),
        TalosError::Conflict(message) => ApiError::new(
            StatusCode::CONFLICT,
            HttpApiErrorCode::InvalidRequest,
            message,
        ),
        TalosError::AuthError(message) => {
            ApiError::new(StatusCode::FORBIDDEN, HttpApiErrorCode::Forbidden, message)
        }
        other => ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            HttpApiErrorCode::InternalError,
            other.to_string(),
        ),
    }
}
