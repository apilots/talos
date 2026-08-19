use serde::{Deserialize, Serialize};
use std::fmt;
use std::panic::Location;

use ts_rs::TS;

/// Public application error code shared by server and SDKs.
///
/// This enum is the single source of truth for client-visible application
/// error semantics. Transport adapters may map codes to protocol-specific
/// envelopes or status codes, but they must not add new application semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum ErrorCode {
    InvalidRequest,
    InvalidRetryRequest,
    Unauthenticated,
    PermissionDenied,
    ResourceNotFound,
    SessionNotFound,
    ResourceExpired,
    Conflict,
    ActiveTurnRunning,
    MessageQueueFull,
    MessageQueueBusy,
    RetrySourceCompleted,
    RetrySourceMismatch,
    IdempotencyConflict,
    InvalidCursor,
    EventCursorExpired,
    FsDisabled,
    InvalidPath,
    PathOutsideRoot,
    SymlinkNotSupported,
    PathNotFound,
    NotDirectory,
    InvalidName,
    AlreadyExists,
    UploadTooLarge,
    UnsupportedMediaType,
    AttachmentLimitExceeded,
    UnsupportedAttachment,
    FileInputRejected,
    ContextWindowExceeded,
    SessionBudgetExceeded,
    UsageLimitExceeded,
    ContextCompactionFailed,
    ProviderUnavailable,
    ToolExecutionFailed,
    Timeout,
    Cancelled,
    ServiceUnavailable,
    InternalError,
}

impl ErrorCode {
    pub const fn default_message(self) -> &'static str {
        match self {
            Self::InvalidRequest => "Invalid request",
            Self::InvalidRetryRequest => "Invalid retry request",
            Self::Unauthenticated => "Unauthenticated",
            Self::PermissionDenied => "Permission denied",
            Self::ResourceNotFound => "Resource not found",
            Self::SessionNotFound => "Session not found",
            Self::ResourceExpired => "Resource expired",
            Self::Conflict => "Conflict",
            Self::ActiveTurnRunning => "Active turn is already running",
            Self::MessageQueueFull => "Message queue is full",
            Self::MessageQueueBusy => "Message queue message is busy",
            Self::RetrySourceCompleted => "Retry source is already completed",
            Self::RetrySourceMismatch => "Retry source does not match",
            Self::IdempotencyConflict => "Idempotency conflict",
            Self::InvalidCursor => "Invalid event cursor",
            Self::EventCursorExpired => "Event cursor expired",
            Self::FsDisabled => "File system access is disabled",
            Self::InvalidPath => "Invalid path",
            Self::PathOutsideRoot => "Path is outside the allowed root",
            Self::SymlinkNotSupported => "Symbolic links are not supported",
            Self::PathNotFound => "Path not found",
            Self::NotDirectory => "Path is not a directory",
            Self::InvalidName => "Invalid name",
            Self::AlreadyExists => "Resource already exists",
            Self::UploadTooLarge => "Upload is too large",
            Self::UnsupportedMediaType => "Unsupported media type",
            Self::AttachmentLimitExceeded => "Attachment limit exceeded",
            Self::UnsupportedAttachment => "Unsupported attachment",
            Self::FileInputRejected => "File input was rejected",
            Self::ContextWindowExceeded => "Context window exceeded",
            Self::SessionBudgetExceeded => "Session budget exceeded",
            Self::UsageLimitExceeded => "Usage limit exceeded",
            Self::ContextCompactionFailed => "Context compaction failed",
            Self::ProviderUnavailable => "Provider unavailable",
            Self::ToolExecutionFailed => "Tool execution failed",
            Self::Timeout => "Operation timed out",
            Self::Cancelled => "Operation was cancelled",
            Self::ServiceUnavailable => "Service unavailable",
            Self::InternalError => "An internal error occurred",
        }
    }
}

/// Public application error object shared by server and SDKs.
///
/// `message` is an English fallback derived from `code`. Clients must branch
/// on `code` and apply localization outside the backend.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, TS, PartialEq, Eq)]
pub struct ApiError {
    pub code: ErrorCode,
    pub message: String,
}

impl ApiError {
    pub fn new(code: ErrorCode) -> Self {
        Self {
            code,
            message: code.default_message().to_string(),
        }
    }
}

impl From<ErrorCode> for ApiError {
    fn from(code: ErrorCode) -> Self {
        Self::new(code)
    }
}

/// Stable internal kind for filesystem domain errors.
///
/// This kind is owned by `talos-error` because filesystem failures cross
/// service, gateway, SDK, and UI boundaries. Transport layers may map it to
/// their own public error codes, but they must not derive filesystem semantics
/// from the display message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TalosFsErrorKind {
    InvalidPath,
    FsDisabled,
    PathOutsideRoot,
    SymlinkNotSupported,
    PathNotFound,
    NotDirectory,
    InvalidName,
    AlreadyExists,
}

/// Stable internal kind for uploaded file and model attachment failures.
///
/// File services, providers, and transports branch on this kind. Generic
/// not-found, validation, conflict, configuration, and storage failures keep
/// using the existing `TalosError` variants instead of being duplicated here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TalosFileErrorKind {
    UploadTooLarge,
    UnsupportedMediaType,
    AttachmentLimitExceeded,
    UnsupportedAttachment,
}

/// Stable internal kind for tool execution failures.
///
/// This kind is the single source of truth for tool-domain failure semantics.
/// Tool registries, kernels, and transport layers may carry it across their
/// boundaries, but they must not encode the kind in a human-readable message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TalosToolErrorKind {
    ValidationFailure,
    PermissionRejection,
    GovernanceRejection,
    StaleWriteFailure,
    DeferredToolUnavailable,
    RuntimeFailure,
}

/// Stable internal kind for memory and session-summary failures.
///
/// Memory-domain code is defined once here because these failures can cross
/// kernel, service, gateway, and client boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TalosMemoryErrorKind {
    SummaryParseFailed,
}

#[derive(Debug)]
pub struct TalosError {
    repr: TalosErrorRepr,
}

#[derive(Debug)]
enum TalosErrorRepr {
    Public {
        error: ApiError,
        diagnostic: String,
        tool_failure: Option<TalosToolFailureDiagnostic>,
        location: &'static Location<'static>,
    },
    Internal {
        context: String,
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
        memory_kind: Option<TalosMemoryErrorKind>,
        location: &'static Location<'static>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TalosToolFailureDiagnostic {
    pub kind: TalosToolErrorKind,
    pub gate_source: Option<String>,
}

pub type TalosResult<T> = Result<T, TalosError>;

impl TalosError {
    /// Build a public application error while keeping internal diagnostics.
    #[track_caller]
    pub fn public(code: ErrorCode, diagnostic: impl Into<String>) -> Self {
        Self {
            repr: TalosErrorRepr::Public {
                error: ApiError::new(code),
                diagnostic: diagnostic.into(),
                tool_failure: None,
                location: Location::caller(),
            },
        }
    }

    /// Build an internal program error with caller location for server logs.
    #[track_caller]
    pub fn internal(context: impl Into<String>) -> Self {
        Self {
            repr: TalosErrorRepr::Internal {
                context: context.into(),
                source: None,
                memory_kind: None,
                location: Location::caller(),
            },
        }
    }

    /// Build an internal program error with a source error for diagnostics.
    #[track_caller]
    pub fn internal_with_source(
        context: impl Into<String>,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self {
            repr: TalosErrorRepr::Internal {
                context: context.into(),
                source: Some(Box::new(source)),
                memory_kind: None,
                location: Location::caller(),
            },
        }
    }

    /// Convert the current internal error model to a public application error.
    ///
    /// This helper is deterministic and does not log. Public boundary owners
    /// are responsible for logging internal failures before exposing the
    /// returned error.
    pub fn to_api_error(&self) -> ApiError {
        match self {
            Self {
                repr: TalosErrorRepr::Public { error, .. },
            } => error.clone(),
            Self {
                repr: TalosErrorRepr::Internal { .. },
            } => ApiError::new(ErrorCode::InternalError),
        }
    }

    pub fn error_code(&self) -> ErrorCode {
        self.to_api_error().code
    }

    pub fn diagnostic_message(&self) -> &str {
        match self {
            Self {
                repr: TalosErrorRepr::Public { diagnostic, .. },
            } => diagnostic,
            Self {
                repr: TalosErrorRepr::Internal { context, .. },
            } => context,
        }
    }

    pub fn location(&self) -> &'static Location<'static> {
        match self {
            Self {
                repr: TalosErrorRepr::Public { location, .. },
            }
            | Self {
                repr: TalosErrorRepr::Internal { location, .. },
            } => location,
        }
    }

    pub fn is_internal(&self) -> bool {
        matches!(self.repr, TalosErrorRepr::Internal { .. })
    }

    pub fn is_code(&self, code: ErrorCode) -> bool {
        self.error_code() == code
    }

    pub fn is_file_error_kind(&self, kind: TalosFileErrorKind) -> bool {
        self.is_code(error_code_for_file_kind(kind))
    }

    pub fn is_fs_error_kind(&self, kind: TalosFsErrorKind) -> bool {
        self.is_code(error_code_for_fs_kind(kind))
    }

    pub fn tool_failure_diagnostic(&self) -> Option<&TalosToolFailureDiagnostic> {
        match &self.repr {
            TalosErrorRepr::Public { tool_failure, .. } => tool_failure.as_ref(),
            TalosErrorRepr::Internal { .. } => None,
        }
    }

    pub fn memory_error_kind(&self) -> Option<TalosMemoryErrorKind> {
        match &self.repr {
            TalosErrorRepr::Internal { memory_kind, .. } => *memory_kind,
            TalosErrorRepr::Public { .. } => None,
        }
    }
}

impl TalosError {
    #[track_caller]
    pub fn authorization_denied() -> Self {
        Self::public(ErrorCode::PermissionDenied, "authorization denied")
    }

    #[track_caller]
    pub fn fs_error(kind: TalosFsErrorKind, message: impl Into<String>) -> Self {
        Self::public(error_code_for_fs_kind(kind), message)
    }

    #[track_caller]
    pub fn file_error(kind: TalosFileErrorKind, message: impl Into<String>) -> Self {
        Self::public(error_code_for_file_kind(kind), message)
    }

    #[track_caller]
    pub fn memory_error(kind: TalosMemoryErrorKind, message: impl Into<String>) -> Self {
        Self {
            repr: TalosErrorRepr::Internal {
                context: message.into(),
                source: None,
                memory_kind: Some(kind),
                location: Location::caller(),
            },
        }
    }

    #[track_caller]
    pub fn tool_failure(
        kind: TalosToolErrorKind,
        message: impl Into<String>,
        gate_source: Option<String>,
    ) -> Self {
        Self {
            repr: TalosErrorRepr::Public {
                error: ApiError::new(error_code_for_tool_kind(kind)),
                diagnostic: message.into(),
                tool_failure: Some(TalosToolFailureDiagnostic { kind, gate_source }),
                location: Location::caller(),
            },
        }
    }
}

fn error_code_for_fs_kind(kind: TalosFsErrorKind) -> ErrorCode {
    match kind {
        TalosFsErrorKind::InvalidPath => ErrorCode::InvalidPath,
        TalosFsErrorKind::FsDisabled => ErrorCode::FsDisabled,
        TalosFsErrorKind::PathOutsideRoot => ErrorCode::PathOutsideRoot,
        TalosFsErrorKind::SymlinkNotSupported => ErrorCode::SymlinkNotSupported,
        TalosFsErrorKind::PathNotFound => ErrorCode::PathNotFound,
        TalosFsErrorKind::NotDirectory => ErrorCode::NotDirectory,
        TalosFsErrorKind::InvalidName => ErrorCode::InvalidName,
        TalosFsErrorKind::AlreadyExists => ErrorCode::AlreadyExists,
    }
}

fn error_code_for_file_kind(kind: TalosFileErrorKind) -> ErrorCode {
    match kind {
        TalosFileErrorKind::UploadTooLarge => ErrorCode::UploadTooLarge,
        TalosFileErrorKind::UnsupportedMediaType => ErrorCode::UnsupportedMediaType,
        TalosFileErrorKind::AttachmentLimitExceeded => ErrorCode::AttachmentLimitExceeded,
        TalosFileErrorKind::UnsupportedAttachment => ErrorCode::UnsupportedAttachment,
    }
}

fn error_code_for_tool_kind(_kind: TalosToolErrorKind) -> ErrorCode {
    ErrorCode::ToolExecutionFailed
}

impl fmt::Display for TalosError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self {
                repr:
                    TalosErrorRepr::Public {
                        error, diagnostic, ..
                    },
            } => {
                write!(f, "{:?}: {diagnostic}", error.code)
            }
            Self {
                repr: TalosErrorRepr::Internal { context, .. },
            } => write!(f, "Internal error: {context}"),
        }
    }
}

impl std::error::Error for TalosError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.repr {
            TalosErrorRepr::Internal {
                source: Some(source),
                ..
            } => Some(source.as_ref()),
            TalosErrorRepr::Public { .. } | TalosErrorRepr::Internal { source: None, .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ApiError, ErrorCode, TalosError};

    #[test]
    fn api_error_uses_code_default_message() {
        assert_eq!(
            ApiError::new(ErrorCode::ResourceNotFound),
            ApiError {
                code: ErrorCode::ResourceNotFound,
                message: "Resource not found".to_string(),
            }
        );
    }

    #[test]
    fn error_code_serializes_as_camel_case() {
        let value = serde_json::to_string(&ErrorCode::ActiveTurnRunning).unwrap();
        assert_eq!(value, "\"activeTurnRunning\"");
        let decoded: ErrorCode = serde_json::from_str(&value).unwrap();
        assert_eq!(decoded, ErrorCode::ActiveTurnRunning);
    }

    #[test]
    fn message_queue_full_has_stable_wire_value_and_message() {
        let value = serde_json::to_string(&ErrorCode::MessageQueueFull).unwrap();

        assert_eq!(value, "\"messageQueueFull\"");
        assert_eq!(
            ApiError::new(ErrorCode::MessageQueueFull),
            ApiError {
                code: ErrorCode::MessageQueueFull,
                message: "Message queue is full".to_string(),
            }
        );
    }

    #[test]
    fn message_queue_busy_has_stable_wire_value_and_message() {
        let value = serde_json::to_string(&ErrorCode::MessageQueueBusy).unwrap();

        assert_eq!(value, "\"messageQueueBusy\"");
        assert_eq!(
            ApiError::new(ErrorCode::MessageQueueBusy),
            ApiError {
                code: ErrorCode::MessageQueueBusy,
                message: "Message queue message is busy".to_string(),
            }
        );
    }

    #[test]
    fn unregistered_error_code_fails_to_decode() {
        let error = serde_json::from_str::<ErrorCode>("\"futureCode\"").unwrap_err();
        assert!(error.is_data());
    }

    #[test]
    fn talos_error_maps_to_public_api_error_without_source_message() {
        assert_eq!(
            TalosError::public(ErrorCode::InvalidRequest, "bad request detail").to_api_error(),
            ApiError::new(ErrorCode::InvalidRequest)
        );
        assert_eq!(
            TalosError::internal("database path leaked").to_api_error(),
            ApiError::new(ErrorCode::InternalError)
        );
    }

    #[test]
    fn talos_error_keeps_internal_diagnostics_out_of_public_error() {
        let error = TalosError::internal("secret path leaked");

        assert!(error.is_internal());
        assert_eq!(error.diagnostic_message(), "secret path leaked");
        assert_eq!(
            error.to_api_error(),
            ApiError::new(ErrorCode::InternalError)
        );
    }
}
