use serde::{Deserialize, Serialize};
use std::backtrace::Backtrace;
use std::fmt;
use std::panic::Location;
use std::sync::Arc;

use ts_rs::TS;

mod api_error;
pub use api_error::{ApiError, ErrorLocalization};

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
    RunForcedAbort,
    RunCleanupUnconfirmed,
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
            Self::RunForcedAbort => "Run required forced cancellation",
            Self::RunCleanupUnconfirmed => "Run cleanup could not be confirmed",
            Self::InternalError => "An internal error occurred",
        }
    }
}

impl ErrorCode {
    /// Internal is the single unknown-failure code; all other codes are explainable.
    pub const fn is_internal(self) -> bool {
        matches!(self, Self::InternalError)
    }
}

/// Non-serializable product failure carried across internal application layers.
///
/// Sources and first-capture diagnostics are shared across clones. Only
/// to_api_error may project this value for clients; Display is diagnostic-only.
#[derive(Debug, Clone)]
pub struct TalosError {
    state: Box<ErrorState>,
}

#[derive(Debug, Clone)]
struct ErrorState {
    error: ApiError,
    diagnostic: String,
    source: Option<Arc<dyn std::error::Error + Send + Sync>>,
    capture: Arc<ErrorCapture>,
}

#[derive(Debug)]
struct ErrorCapture {
    location: &'static Location<'static>,
    backtrace: Backtrace,
}

pub type TalosResult<T> = Result<T, TalosError>;

impl TalosError {
    /// Construct a failure with a reviewed public English explanation.
    /// Keep dependency details in a typed source or diagnostic context instead.
    #[track_caller]
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            state: Box::new(ErrorState {
                error: ApiError::with_message(code, message.clone()),
                diagnostic: message,
                source: None,
                capture: Arc::new(ErrorCapture {
                    location: Location::caller(),
                    backtrace: Backtrace::force_capture(),
                }),
            }),
        }
    }

    /// Construct one exact localizable business explanation without recoding it.
    #[track_caller]
    pub fn localized(localization: ErrorLocalization) -> Self {
        let mut error = Self::new(localization.code(), localization.english_message());
        error.state.error = ApiError::localized(localization);
        error
    }

    /// Wrap a dependency error without stringifying its source chain.
    #[track_caller]
    pub fn from_source(
        code: ErrorCode,
        message: impl Into<String>,
        source: impl Into<Box<dyn std::error::Error + Send + Sync>>,
    ) -> Self {
        Self::new(code, message).with_source(source)
    }

    /// Attach the original dependency cause without replacing classification or capture.
    pub fn with_source(
        mut self,
        source: impl Into<Box<dyn std::error::Error + Send + Sync>>,
    ) -> Self {
        self.state.source = Some(Arc::from(source.into()));
        self
    }

    /// Construct an unclassified failure with a static disclosable explanation.
    /// Dynamic dependency diagnostics belong in from_source/context, not this text.
    #[track_caller]
    pub fn internal(message: &'static str) -> Self {
        Self::new(ErrorCode::InternalError, message)
    }

    /// Wrap an unclassified technical source, retaining its error chain.
    #[track_caller]
    pub fn internal_with_source(
        message: impl Into<String>,
        source: impl Into<Box<dyn std::error::Error + Send + Sync>>,
    ) -> Self {
        Self::from_source(ErrorCode::InternalError, message, source)
    }

    /// Add internal operation context without reclassification or recapture.
    pub fn context(mut self, context: impl fmt::Display) -> Self {
        self.state.diagnostic = format!("{context}: {}", self.state.diagnostic);
        self
    }

    /// Pure public projection. The handling boundary owns diagnostic reporting.
    pub fn to_api_error(&self) -> ApiError {
        self.state.error.clone()
    }

    pub fn error_code(&self) -> ErrorCode {
        self.state.error.code
    }

    pub fn diagnostic_message(&self) -> &str {
        &self.state.diagnostic
    }

    pub fn location(&self) -> &'static Location<'static> {
        self.state.capture.location
    }

    /// First-wrapper native stack, not a reconstructed logical async stack.
    pub fn backtrace(&self) -> &Backtrace {
        &self.state.capture.backtrace
    }

    pub fn is_internal(&self) -> bool {
        self.state.error.code.is_internal()
    }

    pub fn is_code(&self, code: ErrorCode) -> bool {
        self.error_code() == code
    }

    #[track_caller]
    pub fn authorization_denied() -> Self {
        Self::new(ErrorCode::PermissionDenied, "authorization denied")
    }
}

impl fmt::Display for TalosError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_internal() {
            write!(f, "Internal error: {}", self.state.diagnostic)
        } else {
            write!(f, "{:?}: {}", self.state.error.code, self.state.diagnostic)
        }
    }
}

impl std::error::Error for TalosError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.state.source.as_deref().map(|source| source as _)
    }
}

/// SDK boundary conversion for a remote public failure. The captured stack is
/// the local receiving boundary, never the remote backend's diagnostic stack.
impl From<ApiError> for TalosError {
    #[track_caller]
    fn from(error: ApiError) -> Self {
        let mut received =
            Self::new(error.code, error.message.clone()).context("remote application failure");
        received.state.error = error;
        received
    }
}

#[cfg(test)]
#[path = "error_tests.rs"]
mod tests;
