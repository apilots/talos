//! HTTP adapts the common preview API; resource origins carry only a scoped lease.
use super::{
    access::shared_actor,
    errors::{api_success, ApiError, JsonResponse},
    state::HttpApiState,
};
use axum::body::Body;
use axum::http::{header, Method, StatusCode};
use axum::{
    extract::{Request, State},
    middleware::Next,
    response::{IntoResponse, Response},
    Extension, Json,
};
use std::sync::Arc;
use talos_auth::RequestIdentity;
use talos_error::{ErrorCode, TalosError};
use talos_types::*;

pub(super) async fn prepare(
    State(state): State<Arc<HttpApiState>>,
    Extension(identity): Extension<RequestIdentity>,
    Json(request): Json<FilePreviewPrepareRequest>,
) -> Result<JsonResponse<FilePreviewDescriptor>, ApiError> {
    Ok(api_success(
        state
            .gateway
            .prepare_file_preview(&shared_actor(&identity), request)
            .await
            .map_err(ApiError::from_talos_error)?,
    ))
}
pub(super) async fn read(
    State(state): State<Arc<HttpApiState>>,
    Extension(identity): Extension<RequestIdentity>,
    Json(request): Json<FilePreviewReadRequest>,
) -> Result<JsonResponse<FilePreviewContent>, ApiError> {
    Ok(api_success(
        state
            .gateway
            .read_file_preview(&shared_actor(&identity), request)
            .await
            .map_err(ApiError::from_talos_error)?,
    ))
}
pub(super) async fn close(
    State(state): State<Arc<HttpApiState>>,
    Extension(identity): Extension<RequestIdentity>,
    Json(request): Json<FilePreviewCloseRequest>,
) -> Result<JsonResponse<FilePreviewCloseResponse>, ApiError> {
    Ok(api_success(
        state
            .gateway
            .close_file_preview(&shared_actor(&identity), request)
            .await
            .map_err(ApiError::from_talos_error)?,
    ))
}

pub(super) async fn resource_delivery(
    State(state): State<Arc<HttpApiState>>,
    request: Request,
    next: Next,
) -> Response {
    let resource = resource_identity(&state, &request);
    match resource {
        None => next.run(request).await,
        Some((preview_id, path)) => {
            if state.lifecycle.is_quiescing_or_later() {
                return ApiError::new(ErrorCode::ServiceUnavailable).into_response();
            }
            match deliver(state, request, preview_id, path).await {
                Ok(response) => response,
                Err(error) => ApiError::from_talos_error(error).into_response(),
            }
        }
    }
}

fn resource_identity(state: &HttpApiState, request: &Request) -> Option<(String, String)> {
    if let Some(base) = state
        .base_config
        .storage
        .files
        .preview_base_url
        .as_ref()
        .and_then(|value| url::Url::parse(value).ok())
    {
        if let Some(incoming) = request
            .headers()
            .get(header::HOST)
            .and_then(|host| host.to_str().ok())
            .and_then(|host| url::Url::parse(&format!("{}://{host}", base.scheme())).ok())
        {
            if incoming.port_or_known_default() == base.port_or_known_default() {
                let suffix = format!(".{}", base.host_str()?);
                if let Some(id) = incoming.host_str()?.strip_suffix(&suffix) {
                    return Some((
                        id.to_owned(),
                        request.uri().path().trim_start_matches('/').to_owned(),
                    ));
                }
            }
        }
    }
    let path = request
        .uri()
        .path()
        .strip_prefix("/v1/file-previews/resources/")?;
    let (id, resource) = path.split_once('/')?;
    Some((id.to_owned(), resource.to_owned()))
}

async fn deliver(
    state: Arc<HttpApiState>,
    request: Request,
    preview_id: String,
    path: String,
) -> talos_error::TalosResult<Response> {
    if request.method() == Method::OPTIONS {
        return Ok(Response::builder()
            .status(StatusCode::NO_CONTENT)
            .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
            .header(header::ACCESS_CONTROL_ALLOW_METHODS, "GET, HEAD, OPTIONS")
            .header(header::ACCESS_CONTROL_ALLOW_HEADERS, "Range")
            .body(Body::empty())
            .expect("static preview preflight response"));
    }
    if request.method() != Method::GET && request.method() != Method::HEAD {
        return Err(TalosError::new(
            ErrorCode::InvalidRequest,
            "Preview resources support GET and HEAD only.",
        ));
    }
    let resource_path = percent_encoding::percent_decode_str(&path)
        .decode_utf8()
        .map_err(|error| {
            TalosError::from_source(
                ErrorCode::InvalidRequest,
                "The preview resource path is invalid.",
                error,
            )
        })?
        .into_owned();
    let mut read = FilePreviewReadRequest {
        preview_id,
        resource_path,
        offset: 0,
        limit: 0,
    };
    let metadata = state
        .gateway
        .read_file_preview_capability(read.clone())
        .await?
        .metadata;
    let size = metadata.size_bytes;
    let range = request
        .headers()
        .get(header::RANGE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| http_range_header::parse_range_header(value).ok())
        .and_then(|value| value.validate(size).ok())
        .filter(|ranges| size > 0 && ranges.len() == 1)
        .and_then(|ranges| ranges.into_iter().next());
    // Unsupported multipart/invalid ranges are ignored, as permitted by HTTP.
    let (start, end, status) = range.map_or((0, size, StatusCode::OK), |range| {
        (*range.start(), range.end() + 1, StatusCode::PARTIAL_CONTENT)
    });
    let mut response = Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, &metadata.mime_type)
        .header(header::CONTENT_LENGTH, end - start)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CACHE_CONTROL, "private, no-store")
        .header(header::REFERRER_POLICY, "no-referrer")
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .header(
            header::ACCESS_CONTROL_EXPOSE_HEADERS,
            "Content-Length, Content-Range, Accept-Ranges",
        )
        .header(
            header::CONTENT_SECURITY_POLICY,
            "sandbox allow-scripts allow-forms",
        );
    if status == StatusCode::PARTIAL_CONTENT {
        response = response.header(
            header::CONTENT_RANGE,
            format!("bytes {start}-{}/{size}", end - 1),
        );
    }
    let body = if request.method() == Method::HEAD {
        Body::empty()
    } else {
        read.offset = start;
        let stream = futures_util::stream::try_unfold(
            (state, read, end - start, size),
            |(state, mut read, remaining, size)| async move {
                if remaining == 0 {
                    return Ok(None);
                }
                read.limit = remaining.min(u64::from(FILE_PREVIEW_MAX_READ_BYTES)) as u32;
                let result = state
                    .gateway
                    .read_file_preview_capability(read.clone())
                    .await
                    .map_err(std::io::Error::other)?;
                let bytes = result.data;
                if result.offset != read.offset
                    || result.metadata.size_bytes != size
                    || bytes.is_empty()
                    || bytes.len() as u64 > remaining
                {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "Preview content changed during the read.",
                    ));
                }
                read.offset += bytes.len() as u64;
                let length = bytes.len() as u64;
                Ok(Some((bytes, (state, read, remaining - length, size))))
            },
        );
        Body::from_stream(stream)
    };
    response.body(body).map_err(|error| {
        TalosError::internal_with_source("Build the preview resource response.", error)
    })
}
