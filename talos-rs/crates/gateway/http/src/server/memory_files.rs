use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use talos_application_service::{MemoryDate, MemoryListParams, MemorySearchParams};
use talos_auth::RequestIdentity;
use talos_error::TalosError;
use talos_types::DocUpdateRequest;

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
    q: String,
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
    let params = MemoryListParams {
        limit: query.limit,
        before_date: parse_optional_memory_date(query.before_date.as_deref())?,
        date_from: parse_optional_memory_date(query.date_from.as_deref())?,
        date_to: parse_optional_memory_date(query.date_to.as_deref())?,
    };
    let files = state
        .memory
        .list_files(&actor, params)
        .await
        .map_err(map_memory_error)?;
    Ok(Json(json!({ "data": files })))
}

pub(super) async fn get_memory_file_handler(
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<RequestIdentity>,
    Path(date): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let actor = shared_actor(&auth.0);
    let date = MemoryDate::parse(&date).map_err(map_memory_error)?;
    let detail = state
        .memory
        .get_file(&actor, date)
        .await
        .map_err(map_memory_error)?;
    Ok(Json(json!(detail)))
}

pub(super) async fn put_memory_file_handler(
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<RequestIdentity>,
    Path(date): Path<String>,
    Json(request): Json<DocUpdateRequest>,
) -> Result<Json<Value>, ApiError> {
    let actor = shared_actor(&auth.0);
    let date = MemoryDate::parse(&date).map_err(map_memory_error)?;
    let detail = state
        .memory
        .put_file(&actor, date, request)
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
    let params = MemorySearchParams {
        query: query.q,
        limit: query.limit,
        date_from: parse_optional_memory_date(query.date_from.as_deref())?,
        date_to: parse_optional_memory_date(query.date_to.as_deref())?,
    };
    let results = state
        .memory
        .search_files(&actor, params)
        .await
        .map_err(map_memory_error)?;
    Ok(Json(json!({ "data": results })))
}

fn parse_optional_memory_date(value: Option<&str>) -> Result<Option<MemoryDate>, ApiError> {
    value
        .map(MemoryDate::parse)
        .transpose()
        .map_err(map_memory_error)
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
