use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use talos_auth::RequestIdentity;
use talos_error::TalosError;
use talos_types::{
    MemoryFileListRequest, MemoryFileReadRequest, MemoryFileWriteRequest, MemorySearchRequest,
};

use super::access::shared_actor;
use super::errors::ApiError;
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
pub(super) struct MemorySearchHttpQuery {
    query: String,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    date_from: Option<String>,
    #[serde(default)]
    date_to: Option<String>,
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
            "invalid_request",
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
        date_from: query.date_from,
        date_to: query.date_to,
    };
    let response = state
        .gateway
        .search_memory(&actor, request)
        .await
        .map_err(map_memory_error)?;
    Ok(Json(json!(response)))
}

fn map_memory_error(error: TalosError) -> ApiError {
    match error {
        TalosError::ValidationError(message) => {
            ApiError::new(StatusCode::BAD_REQUEST, "invalid_request", message)
        }
        TalosError::ResourceNotFound(message) => {
            ApiError::new(StatusCode::NOT_FOUND, "invalid_request", message)
        }
        TalosError::Conflict(message) => {
            ApiError::new(StatusCode::CONFLICT, "invalid_request", message)
        }
        TalosError::AuthError(message) => {
            ApiError::new(StatusCode::FORBIDDEN, "forbidden", message)
        }
        other => ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            other.to_string(),
        ),
    }
}
