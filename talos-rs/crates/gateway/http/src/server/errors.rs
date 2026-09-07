use axum::extract::Request;
use axum::http::{header, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Json, Response};
use serde::Serialize;
use talos_error::{ApiError as PublicApiError, ErrorCode};
use talos_types::{ApiResponse, TalosError};

pub(super) type JsonResponse<T> = Json<ApiResponse<T>>;

pub(super) fn api_success<T>(data: T) -> JsonResponse<T> {
    Json(ApiResponse::success(data))
}

pub(super) fn api_success_with_status<T: Serialize>(status: StatusCode, data: T) -> Response {
    (status, Json(ApiResponse::success(data))).into_response()
}

#[derive(Debug)]
pub(super) struct ApiError {
    status: StatusCode,
    body: PublicApiError,
}

impl ApiError {
    pub(super) fn new(code: ErrorCode) -> Self {
        Self::from_talos_error(TalosError::new(code, code.default_message()))
    }

    pub(super) fn from_talos_error(error: TalosError) -> Self {
        talos_logging::report_error(&error);
        Self {
            status: status_for_error_code(error.error_code()),
            body: error.to_api_error(),
        }
    }
}

fn status_for_error_code(code: ErrorCode) -> StatusCode {
    match code {
        ErrorCode::InvalidRequest
        | ErrorCode::InvalidRetryRequest
        | ErrorCode::InvalidCursor
        | ErrorCode::InvalidPath
        | ErrorCode::SymlinkNotSupported
        | ErrorCode::NotDirectory
        | ErrorCode::InvalidName
        | ErrorCode::UnsupportedAttachment
        | ErrorCode::FileInputRejected
        | ErrorCode::ContextWindowExceeded => StatusCode::BAD_REQUEST,
        ErrorCode::Unauthenticated => StatusCode::UNAUTHORIZED,
        ErrorCode::PermissionDenied | ErrorCode::FsDisabled | ErrorCode::PathOutsideRoot => {
            StatusCode::FORBIDDEN
        }
        ErrorCode::ResourceNotFound | ErrorCode::SessionNotFound | ErrorCode::PathNotFound => {
            StatusCode::NOT_FOUND
        }
        ErrorCode::ResourceExpired | ErrorCode::EventCursorExpired => StatusCode::GONE,
        ErrorCode::Conflict
        | ErrorCode::ActiveTurnRunning
        | ErrorCode::MessageQueueFull
        | ErrorCode::MessageQueueBusy
        | ErrorCode::RetrySourceCompleted
        | ErrorCode::RetrySourceMismatch
        | ErrorCode::IdempotencyConflict
        | ErrorCode::AlreadyExists
        | ErrorCode::Cancelled => StatusCode::CONFLICT,
        ErrorCode::UploadTooLarge | ErrorCode::AttachmentLimitExceeded => {
            StatusCode::PAYLOAD_TOO_LARGE
        }
        ErrorCode::UnsupportedMediaType => StatusCode::UNSUPPORTED_MEDIA_TYPE,
        ErrorCode::SessionBudgetExceeded | ErrorCode::UsageLimitExceeded => {
            StatusCode::TOO_MANY_REQUESTS
        }
        ErrorCode::ProviderUnavailable | ErrorCode::ServiceUnavailable => {
            StatusCode::SERVICE_UNAVAILABLE
        }
        ErrorCode::ToolExecutionFailed => StatusCode::BAD_GATEWAY,
        ErrorCode::Timeout => StatusCode::GATEWAY_TIMEOUT,
        ErrorCode::ContextCompactionFailed
        | ErrorCode::RunForcedAbort
        | ErrorCode::RunCleanupUnconfirmed
        | ErrorCode::InternalError => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

pub(super) async fn normalize_extractor_rejection(request: Request, next: Next) -> Response {
    let is_api = request.uri().path().starts_with("/v1/");
    let response = next.run(request).await;
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if !is_api
        || content_type.starts_with("application/json")
        || content_type.starts_with("text/event-stream")
    {
        return response;
    }

    let (code, message) = match response.status() {
        StatusCode::BAD_REQUEST => (
            ErrorCode::InvalidRequest,
            "The request could not be parsed. Check its JSON, query parameters, and path values.",
        ),
        StatusCode::UNPROCESSABLE_ENTITY => (
            ErrorCode::InvalidRequest,
            "The request body does not match the expected fields or types.",
        ),
        StatusCode::PAYLOAD_TOO_LARGE => (
            ErrorCode::UploadTooLarge,
            "The request body exceeds the allowed upload size.",
        ),
        StatusCode::UNSUPPORTED_MEDIA_TYPE => (
            ErrorCode::UnsupportedMediaType,
            "The request Content-Type is not supported by this endpoint.",
        ),
        StatusCode::NOT_FOUND => (
            ErrorCode::ResourceNotFound,
            "The requested API endpoint was not found.",
        ),
        StatusCode::METHOD_NOT_ALLOWED => (
            ErrorCode::InvalidRequest,
            "The HTTP method is not supported for this endpoint.",
        ),
        status if status.is_server_error() => (
            ErrorCode::InternalError,
            "The API request failed unexpectedly; technical details were withheld.",
        ),
        _ => return response,
    };
    let error = TalosError::new(code, message);
    talos_logging::report_error(&error);
    let (mut parts, _) = response.into_parts();
    let (json_parts, body) = Json(ApiResponse::<()>::error(error.to_api_error()))
        .into_response()
        .into_parts();
    // Status and transport headers belong to the framework. Only representation
    // metadata becomes invalid when replacing its body with the shared envelope.
    for name in [
        header::CONTENT_LENGTH,
        header::CONTENT_ENCODING,
        header::CONTENT_RANGE,
        header::ETAG,
        header::LAST_MODIFIED,
        header::CONTENT_DISPOSITION,
    ] {
        parts.headers.remove(name);
    }
    parts.headers.extend(json_parts.headers);
    Response::from_parts(parts, body)
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(ApiResponse::<()>::error(self.body))).into_response()
    }
}
