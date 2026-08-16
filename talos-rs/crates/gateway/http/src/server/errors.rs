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
        Self {
            status: status_for_error_code(code),
            body: PublicApiError::new(code),
        }
    }

    pub(super) fn from_talos_error(error: TalosError) -> Self {
        if error.is_internal() {
            tracing::error!(
                context = error.diagnostic_message(),
                source = ?std::error::Error::source(&error),
                location = %error.location(),
                "internal HTTP application error"
            );
        }
        Self::new(error.to_api_error().code)
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
        ErrorCode::ContextCompactionFailed | ErrorCode::InternalError => {
            StatusCode::INTERNAL_SERVER_ERROR
        }
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

    let code = match response.status() {
        StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY => ErrorCode::InvalidRequest,
        StatusCode::PAYLOAD_TOO_LARGE => ErrorCode::UploadTooLarge,
        StatusCode::UNSUPPORTED_MEDIA_TYPE => ErrorCode::UnsupportedMediaType,
        _ => return response,
    };
    ApiError::new(code).into_response()
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(ApiResponse::<()>::error(self.body))).into_response()
    }
}
