use serde::{Deserialize, Serialize};
use thiserror::Error;
use ts_rs::TS;

/// Stable machine-readable category for a failed Agent run.
///
/// This public run-level code is owned by `talos-error`. Protocol, SDK, UI, and
/// TUI layers may expose or render it, but they must not derive it from error
/// message text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
#[ts(rename_all = "snake_case")]
pub enum TalosRunErrorCode {
    ContextWindowExceeded,
    SessionBudgetExceeded,
    UsageLimitExceeded,
    ContextCompactionFailed,
    ProviderUnavailable,
    PermissionDenied,
    ToolFailure,
    FileInputRejected,
    InternalError,
}

/// Optional structured error details carried by turn failure events.
///
/// The message is for display and diagnostics only. Callers must branch on
/// `code`, never parse `message`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, TS, PartialEq, Eq)]
pub struct TalosRunErrorInfo {
    pub code: TalosRunErrorCode,
    pub message: String,
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

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum TalosError {
    #[error("Agent not found: {0}")]
    AgentNotFound(String),

    #[error("Route not found: {0}")]
    RouteNotFound(String),

    #[error("Invalid configuration: {0}")]
    InvalidConfiguration(String),

    #[error("Config error: {0}")]
    ConfigError(String),

    #[error("Network error: {0}")]
    NetworkError(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    #[error("Auth error: {0}")]
    AuthError(String),

    #[error("Authorization denied")]
    AuthorizationDenied,

    #[error("Resource not found: {0}")]
    ResourceNotFound(String),

    #[error("Resource expired: {0}")]
    ResourceExpired(String),

    #[error("Conflict: {0}")]
    Conflict(String),

    #[error("Unsupported model: {0}")]
    UnsupportedModel(String),

    #[error("Idempotency conflict: {0}")]
    IdempotencyConflict(String),

    #[error("Idempotency error: {0}")]
    IdempotencyError(String),

    #[error("Retry active turn running: {0}")]
    RetryActiveTurnRunning(String),

    #[error("Active turn running: {0}")]
    ActiveTurnRunning(String),

    #[error("Retry source completed: {0}")]
    RetrySourceCompleted(String),

    #[error("Retry source mismatch: {0}")]
    RetrySourceMismatch(String),

    #[error("Timeout: {0}")]
    Timeout(String),

    #[error("Storage error: {0}")]
    StorageError(String),

    #[error("Gateway error: {0}")]
    GatewayError(String),

    #[error("Runtime unavailable: {0}")]
    RuntimeUnavailable(String),

    #[error("Transport unavailable: {0}")]
    TransportUnavailable(String),

    #[error("File system error ({kind:?}): {message}")]
    FsError {
        kind: TalosFsErrorKind,
        message: String,
    },

    #[error("File error ({kind:?}): {message}")]
    FileError {
        kind: TalosFileErrorKind,
        message: String,
    },

    #[error("Memory error ({kind:?}): {message}")]
    MemoryError {
        kind: TalosMemoryErrorKind,
        message: String,
    },

    #[error("Context window exceeded: {0}")]
    ContextWindowExceeded(String),

    #[error("Context compaction failed: {0}")]
    ContextCompactionFailed(String),

    #[error("Internal error: {0}")]
    InternalError(String),

    #[error("Validation error: {0}")]
    ValidationError(String),

    #[error("Tool error: {0}")]
    ToolError(String),

    #[error("Tool failure ({kind:?}): {message}")]
    ToolFailure {
        kind: TalosToolErrorKind,
        message: String,
        gate_source: Option<String>,
    },

    #[error("Tool not found: {0}")]
    ToolNotFound(String),

    #[error("Client error: {0}")]
    ClientError(String),

    #[error("Cancelled: {0}")]
    Cancelled(String),
}

pub type TalosResult<T> = Result<T, TalosError>;

impl TalosError {
    /// Map an internal product error to the public run error info carried on
    /// failed turn events.
    ///
    /// This is the single source of truth for turn-level error code mapping.
    /// Unknown or non-run errors intentionally return `None` instead of
    /// guessing from the display message.
    pub fn to_run_error_info(&self) -> Option<TalosRunErrorInfo> {
        let code = match self {
            TalosError::ContextWindowExceeded(_) => TalosRunErrorCode::ContextWindowExceeded,
            TalosError::ContextCompactionFailed(_) => TalosRunErrorCode::ContextCompactionFailed,
            TalosError::RuntimeUnavailable(_)
            | TalosError::TransportUnavailable(_)
            | TalosError::UnsupportedModel(_) => TalosRunErrorCode::ProviderUnavailable,
            TalosError::AuthorizationDenied
            | TalosError::AuthenticationFailed(_)
            | TalosError::AuthError(_) => TalosRunErrorCode::PermissionDenied,
            TalosError::ToolError(_)
            | TalosError::ToolFailure { .. }
            | TalosError::ToolNotFound(_) => TalosRunErrorCode::ToolFailure,
            TalosError::FileError { .. } => TalosRunErrorCode::FileInputRejected,
            TalosError::InternalError(_) => TalosRunErrorCode::InternalError,
            _ => return None,
        };
        Some(TalosRunErrorInfo {
            code,
            message: self.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{TalosError, TalosFileErrorKind, TalosRunErrorCode};

    #[test]
    fn run_error_info_maps_only_explicit_run_errors() {
        assert_eq!(
            TalosError::ContextWindowExceeded("too large".to_string())
                .to_run_error_info()
                .expect("context window should map")
                .code,
            TalosRunErrorCode::ContextWindowExceeded
        );
        assert_eq!(
            TalosError::ContextCompactionFailed("summary failed".to_string())
                .to_run_error_info()
                .expect("compaction should map")
                .code,
            TalosRunErrorCode::ContextCompactionFailed
        );
        assert_eq!(
            TalosError::UnsupportedModel("missing model".to_string())
                .to_run_error_info()
                .expect("provider should map")
                .code,
            TalosRunErrorCode::ProviderUnavailable
        );
        assert_eq!(
            TalosError::AuthorizationDenied
                .to_run_error_info()
                .expect("permission should map")
                .code,
            TalosRunErrorCode::PermissionDenied
        );
        assert_eq!(
            TalosError::ToolNotFound("read_file".to_string())
                .to_run_error_info()
                .expect("tool should map")
                .code,
            TalosRunErrorCode::ToolFailure
        );
        assert_eq!(
            TalosError::InternalError("broken invariant".to_string())
                .to_run_error_info()
                .expect("internal should map")
                .code,
            TalosRunErrorCode::InternalError
        );
        assert_eq!(
            TalosError::FileError {
                kind: TalosFileErrorKind::UnsupportedAttachment,
                message: "attachments are unsupported".to_string(),
            }
            .to_run_error_info()
            .expect("file input should map")
            .code,
            TalosRunErrorCode::FileInputRejected
        );
    }

    #[test]
    fn run_error_info_does_not_guess_budget_codes() {
        assert!(TalosError::ValidationError(
            "prompt budget remains exhausted after compaction".to_string()
        )
        .to_run_error_info()
        .is_none());
        assert!(TalosError::ConfigError("usage limit exceeded".to_string())
            .to_run_error_info()
            .is_none());
    }

    #[test]
    fn file_error_serializes_a_machine_readable_kind() {
        let error = TalosError::FileError {
            kind: TalosFileErrorKind::UnsupportedMediaType,
            message: "application/octet-stream is not supported".to_string(),
        };

        assert_eq!(
            serde_json::to_value(error).unwrap(),
            serde_json::json!({
                "FileError": {
                    "kind": "unsupported_media_type",
                    "message": "application/octet-stream is not supported"
                }
            })
        );
    }
}
