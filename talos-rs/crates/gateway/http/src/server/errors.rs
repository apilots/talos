use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use serde::Serialize;
use talos_error::{TalosFileErrorKind, TalosFsErrorKind};

/// Stable REST API error codes owned by the HTTP gateway.
///
/// This enum only contains HTTP, transport, and resource-boundary codes. Domain
/// codes such as filesystem errors must be passed through their domain kind
/// instead of being duplicated here.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum HttpApiErrorCode {
    ActiveTurnFailed,
    ActiveTurnRunning,
    ActiveTurnSerializeFailed,
    Conflict,
    EventCursorExpired,
    EventRecordFailed,
    EventReplayFailed,
    FirstEventTimeout,
    Forbidden,
    IdempotencyConflict,
    IdempotencyFailed,
    InternalError,
    InvalidCursor,
    InvalidRequest,
    InvalidRetryRequest,
    NotFound,
    PermissionDenied,
    RetryActiveTurnRunning,
    RetrySourceCompleted,
    RetrySourceMismatch,
    ServerShuttingDown,
    SessionNotFound,
    Unauthorized,
}

/// Public HTTP error code payload.
///
/// The HTTP response may expose a gateway-owned code or a lower domain code
/// that intentionally crosses the boundary, such as `TalosFsErrorKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ApiErrorCode {
    Http(HttpApiErrorCode),
    File(TalosFileErrorKind),
    Fs(TalosFsErrorKind),
}

impl Serialize for ApiErrorCode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Http(code) => code.serialize(serializer),
            Self::File(kind) => kind.serialize(serializer),
            Self::Fs(kind) => kind.serialize(serializer),
        }
    }
}

impl From<HttpApiErrorCode> for ApiErrorCode {
    fn from(code: HttpApiErrorCode) -> Self {
        Self::Http(code)
    }
}

impl From<TalosFsErrorKind> for ApiErrorCode {
    fn from(kind: TalosFsErrorKind) -> Self {
        Self::Fs(kind)
    }
}

impl From<TalosFileErrorKind> for ApiErrorCode {
    fn from(kind: TalosFileErrorKind) -> Self {
        Self::File(kind)
    }
}

#[derive(Debug, Serialize)]
pub(super) struct ApiErrorBody {
    code: ApiErrorCode,
    message: String,
}

#[derive(Debug)]
pub(super) struct ApiError {
    status: StatusCode,
    body: ApiErrorBody,
}

impl ApiError {
    pub(super) fn new(
        status: StatusCode,
        code: impl Into<ApiErrorCode>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            status,
            body: ApiErrorBody {
                code: code.into(),
                message: message.into(),
            },
        }
    }

    pub(super) fn from_talos_error(error: talos_error::TalosError) -> Self {
        match error {
            talos_error::TalosError::AuthError(message) => Self::new(
                StatusCode::UNAUTHORIZED,
                HttpApiErrorCode::Unauthorized,
                message,
            ),
            talos_error::TalosError::AuthenticationFailed(message) => Self::new(
                StatusCode::UNAUTHORIZED,
                HttpApiErrorCode::Unauthorized,
                message,
            ),
            talos_error::TalosError::AuthorizationDenied => Self::new(
                StatusCode::FORBIDDEN,
                HttpApiErrorCode::Forbidden,
                "authorization denied",
            ),
            talos_error::TalosError::ValidationError(message) => Self::new(
                StatusCode::BAD_REQUEST,
                HttpApiErrorCode::InvalidRequest,
                message,
            ),
            talos_error::TalosError::ResourceNotFound(message) => {
                Self::new(StatusCode::NOT_FOUND, HttpApiErrorCode::NotFound, message)
            }
            talos_error::TalosError::Conflict(message) => {
                Self::new(StatusCode::CONFLICT, HttpApiErrorCode::Conflict, message)
            }
            talos_error::TalosError::FsError { kind, message } => {
                Self::new(fs_status(kind), kind, message)
            }
            talos_error::TalosError::FileError { kind, message } => {
                Self::new(file_status(kind), kind, message)
            }
            other => Self::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                HttpApiErrorCode::InternalError,
                other.to_string(),
            ),
        }
    }
}

fn file_status(kind: TalosFileErrorKind) -> StatusCode {
    match kind {
        TalosFileErrorKind::UploadTooLarge | TalosFileErrorKind::AttachmentLimitExceeded => {
            StatusCode::PAYLOAD_TOO_LARGE
        }
        TalosFileErrorKind::UnsupportedMediaType | TalosFileErrorKind::UnsupportedAttachment => {
            StatusCode::BAD_REQUEST
        }
    }
}

fn fs_status(kind: TalosFsErrorKind) -> StatusCode {
    match kind {
        TalosFsErrorKind::FsDisabled | TalosFsErrorKind::PathOutsideRoot => StatusCode::FORBIDDEN,
        TalosFsErrorKind::PathNotFound => StatusCode::NOT_FOUND,
        TalosFsErrorKind::AlreadyExists => StatusCode::CONFLICT,
        TalosFsErrorKind::InvalidPath
        | TalosFsErrorKind::SymlinkNotSupported
        | TalosFsErrorKind::NotDirectory
        | TalosFsErrorKind::InvalidName => StatusCode::BAD_REQUEST,
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(self.body)).into_response()
    }
}
