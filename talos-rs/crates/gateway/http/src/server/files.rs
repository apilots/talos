use std::collections::BTreeMap;
use std::io;
use std::sync::Arc;

use axum::extract::multipart::{Field, MultipartRejection};
use axum::extract::{Multipart, Path, RawQuery, State};
use axum::response::Json;
use bytes::Bytes;
use serde::Deserialize;
use talos_auth::RequestIdentity;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_service::FileByteStream;
use talos_types::{
    FileDeleteRequest, FileDeleteResponse, FileListRequest, FileListResponse, FileReadRequest,
    FileReadResponse, FileScope, FileUpdateRequest, FileUpdateResponse, FileUploadRequest,
    FileUploadResponse,
};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use super::access::shared_actor;
use super::errors::{api_success, ApiError, JsonResponse};
use super::state::HttpApiState;

const METADATA_PART_MAX_BYTES: usize = 64 * 1024;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum FileListHttpScope {
    All,
    Personal,
    Project,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FileListHttpQuery {
    cursor: Option<String>,
    limit: Option<u32>,
    scope: Option<FileListHttpScope>,
    project_id: Option<String>,
    #[serde(default, rename = "tag")]
    tags: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct FileUpdateBody {
    tags: Option<Vec<String>>,
    metadata: Option<BTreeMap<String, String>>,
}

pub(super) async fn upload_file_handler(
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<RequestIdentity>,
    multipart: Result<Multipart, MultipartRejection>,
) -> Result<JsonResponse<FileUploadResponse>, ApiError> {
    let mut multipart =
        multipart.map_err(|error| ApiError::from_talos_error(multipart_failure(error)))?;
    let metadata_field = multipart
        .next_field()
        .await
        .map_err(multipart_error)?
        .ok_or_else(|| invalid_request("File upload requires a metadata part."))?;
    if metadata_field.name() != Some("metadata") {
        return Err(invalid_request("The first upload part must be metadata."));
    }
    let request = read_metadata(metadata_field).await?;

    let max_upload_bytes = state.app.files.limits().max_upload_bytes;
    let (tx, rx) = mpsc::channel::<io::Result<Bytes>>(1);
    let body: FileByteStream = Box::pin(ReceiverStream::new(rx));
    let actor = shared_actor(&auth.0);
    let upload = state.gateway.upload_file(&actor, request, body);
    let forward = async {
        forward_required_file_part(&mut multipart, &tx, max_upload_bytes).await?;
        match multipart.next_field().await {
            Ok(None) => {}
            Ok(Some(_)) => {
                let error = TalosError::new(
                    ErrorCode::InvalidRequest,
                    "file upload accepts exactly one metadata part and one file part",
                );
                let _ = tx.send(Err(io::Error::other(error.clone()))).await;
                return Err(error);
            }
            Err(error) => {
                let error = multipart_failure(error);
                let _ = tx.send(Err(io::Error::other(error.clone()))).await;
                return Err(error);
            }
        }
        drop(tx);
        Ok(())
    };

    let (upload_result, forward_result) = tokio::join!(upload, forward);
    // Forwarding owns framing failures, also delivered to the upload reader to
    // abort storage. Report the original once, not its echoed reader failure.
    if let Err(error) = forward_result {
        return Err(ApiError::from_talos_error(error));
    }
    let response = upload_result.map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}

async fn forward_required_file_part(
    multipart: &mut Multipart,
    tx: &mpsc::Sender<io::Result<Bytes>>,
    max_upload_bytes: u64,
) -> TalosResult<()> {
    let mut field = match multipart.next_field().await {
        Ok(Some(field)) => field,
        Ok(None) => {
            let error = TalosError::new(
                ErrorCode::InvalidRequest,
                "file upload requires a file part",
            );
            let _ = tx.send(Err(io::Error::other(error.clone()))).await;
            return Err(error);
        }
        Err(source) => {
            let error = multipart_failure(source);
            let _ = tx.send(Err(io::Error::other(error.clone()))).await;
            return Err(error);
        }
    };
    if field.name() != Some("file") {
        let error = TalosError::new(
            ErrorCode::InvalidRequest,
            "file upload second part must be file",
        );
        let _ = tx.send(Err(io::Error::other(error.clone()))).await;
        return Err(error);
    }
    let mut size_bytes = 0_u64;
    loop {
        let chunk = match field.chunk().await {
            Ok(Some(chunk)) => chunk,
            Ok(None) => return Ok(()),
            Err(error) => {
                let error = multipart_failure(error);
                let _ = tx.send(Err(io::Error::other(error.clone()))).await;
                return Err(error);
            }
        };
        let Some(next_size) = size_bytes.checked_add(chunk.len() as u64) else {
            let error = TalosError::new(
                ErrorCode::UploadTooLarge,
                "file upload size overflow".to_string(),
            );
            let _ = tx.send(Err(io::Error::other(error.clone()))).await;
            return Err(error);
        };
        size_bytes = next_size;
        if size_bytes > max_upload_bytes {
            let error = TalosError::new(
                ErrorCode::UploadTooLarge,
                format!("file upload exceeds {max_upload_bytes} bytes"),
            );
            let _ = tx.send(Err(io::Error::other(error.clone()))).await;
            return Err(error);
        }
        if tx.send(Ok(chunk)).await.is_err() {
            // The upload future has already finished and owns its Result.
            return Ok(());
        }
    }
}

pub(super) async fn list_files_handler(
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<RequestIdentity>,
    RawQuery(raw_query): RawQuery,
) -> Result<JsonResponse<FileListResponse>, ApiError> {
    let query: FileListHttpQuery = serde_html_form::from_str(raw_query.as_deref().unwrap_or(""))
        .map_err(|error| {
            ApiError::from_talos_error(TalosError::from_source(
                ErrorCode::InvalidRequest,
                "The file list query is invalid.",
                error,
            ))
        })?;
    let request = file_list_request(query)?;
    let response = state
        .gateway
        .list_files(&shared_actor(&auth.0), request)
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}

pub(super) async fn read_file_handler(
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<RequestIdentity>,
    Path(file_id): Path<String>,
) -> Result<JsonResponse<FileReadResponse>, ApiError> {
    let response = state
        .gateway
        .read_file(&shared_actor(&auth.0), FileReadRequest { file_id })
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}

pub(super) async fn update_file_handler(
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<RequestIdentity>,
    Path(file_id): Path<String>,
    Json(body): Json<FileUpdateBody>,
) -> Result<JsonResponse<FileUpdateResponse>, ApiError> {
    let response = state
        .gateway
        .update_file(
            &shared_actor(&auth.0),
            FileUpdateRequest {
                file_id,
                tags: body.tags,
                metadata: body.metadata,
            },
        )
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}

pub(super) async fn delete_file_handler(
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<RequestIdentity>,
    Path(file_id): Path<String>,
) -> Result<JsonResponse<FileDeleteResponse>, ApiError> {
    let response = state
        .gateway
        .delete_file(&shared_actor(&auth.0), FileDeleteRequest { file_id })
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}

async fn read_metadata(mut field: Field<'_>) -> Result<FileUploadRequest, ApiError> {
    let mut data = Vec::new();
    while let Some(chunk) = field.chunk().await.map_err(multipart_error)? {
        if data.len().saturating_add(chunk.len()) > METADATA_PART_MAX_BYTES {
            return Err(invalid_request("The upload metadata exceeds 64 KiB."));
        }
        data.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&data).map_err(|error| {
        ApiError::from_talos_error(TalosError::from_source(
            ErrorCode::InvalidRequest,
            "The upload metadata is not a valid file upload request.",
            error,
        ))
    })
}

fn file_list_request(query: FileListHttpQuery) -> Result<FileListRequest, ApiError> {
    let scope = match (query.scope, query.project_id) {
        (None | Some(FileListHttpScope::All), None) => None,
        (Some(FileListHttpScope::Personal), None) => Some(FileScope::Personal),
        (Some(FileListHttpScope::Project), Some(project_id)) => {
            Some(FileScope::Project { project_id })
        }
        (Some(FileListHttpScope::Project), None) => {
            return Err(invalid_request(
                "The file list scope and project id do not agree.",
            ));
        }
        (None | Some(FileListHttpScope::All | FileListHttpScope::Personal), Some(_)) => {
            return Err(invalid_request(
                "The file list scope and project id do not agree.",
            ));
        }
    };
    Ok(FileListRequest {
        cursor: query.cursor,
        limit: query.limit,
        scope,
        tags: (!query.tags.is_empty()).then_some(query.tags),
    })
}

fn multipart_error(error: axum::extract::multipart::MultipartError) -> ApiError {
    ApiError::from_talos_error(multipart_failure(error))
}

fn multipart_failure(error: impl std::error::Error + Send + Sync + 'static) -> TalosError {
    TalosError::from_source(
        ErrorCode::InvalidRequest,
        "The file upload multipart body is invalid.",
        error,
    )
}

fn invalid_request(message: &'static str) -> ApiError {
    ApiError::from_talos_error(TalosError::new(ErrorCode::InvalidRequest, message))
}
