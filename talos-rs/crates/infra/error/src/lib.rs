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

/// Product failure classification, independent of transport status and visibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Business,
    Internal,
}

impl ErrorCode {
    /// Classify every code explicitly when the shared taxonomy is extended.
    pub const fn kind(self) -> ErrorKind {
        match self {
            Self::InternalError => ErrorKind::Internal,
            Self::InvalidRequest
            | Self::InvalidRetryRequest
            | Self::Unauthenticated
            | Self::PermissionDenied
            | Self::ResourceNotFound
            | Self::SessionNotFound
            | Self::ResourceExpired
            | Self::Conflict
            | Self::ActiveTurnRunning
            | Self::MessageQueueFull
            | Self::MessageQueueBusy
            | Self::RetrySourceCompleted
            | Self::RetrySourceMismatch
            | Self::IdempotencyConflict
            | Self::InvalidCursor
            | Self::EventCursorExpired
            | Self::FsDisabled
            | Self::InvalidPath
            | Self::PathOutsideRoot
            | Self::SymlinkNotSupported
            | Self::PathNotFound
            | Self::NotDirectory
            | Self::InvalidName
            | Self::AlreadyExists
            | Self::UploadTooLarge
            | Self::UnsupportedMediaType
            | Self::AttachmentLimitExceeded
            | Self::UnsupportedAttachment
            | Self::FileInputRejected
            | Self::ContextWindowExceeded
            | Self::SessionBudgetExceeded
            | Self::UsageLimitExceeded
            | Self::ContextCompactionFailed
            | Self::ProviderUnavailable
            | Self::ToolExecutionFailed
            | Self::Timeout
            | Self::Cancelled
            | Self::ServiceUnavailable
            | Self::RunForcedAbort
            | Self::RunCleanupUnconfirmed => ErrorKind::Business,
        }
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
    tool_failure: Option<TalosToolFailureDiagnostic>,
    memory_kind: Option<TalosMemoryErrorKind>,
}

#[derive(Debug)]
struct ErrorCapture {
    location: &'static Location<'static>,
    backtrace: Backtrace,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TalosToolFailureDiagnostic {
    pub kind: TalosToolErrorKind,
    pub gate_source: Option<String>,
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
                tool_failure: None,
                memory_kind: None,
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

    pub fn kind(&self) -> ErrorKind {
        self.state.error.code.kind()
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
        self.kind() == ErrorKind::Internal
    }

    pub fn is_code(&self, code: ErrorCode) -> bool {
        self.error_code() == code
    }

    pub fn tool_failure_diagnostic(&self) -> Option<&TalosToolFailureDiagnostic> {
        self.state.tool_failure.as_ref()
    }

    pub fn memory_error_kind(&self) -> Option<TalosMemoryErrorKind> {
        self.state.memory_kind
    }

    #[track_caller]
    pub fn authorization_denied() -> Self {
        Self::new(ErrorCode::PermissionDenied, "authorization denied")
    }

    #[track_caller]
    pub fn memory_error(kind: TalosMemoryErrorKind, message: impl Into<String>) -> Self {
        let mut error =
            Self::internal("Failed to parse the memory summary; technical details were withheld.")
                .context(message.into());
        error.state.memory_kind = Some(kind);
        error
    }

    #[track_caller]
    pub fn tool_failure(
        kind: TalosToolErrorKind,
        message: impl Into<String>,
        gate_source: Option<String>,
    ) -> Self {
        let mut error = Self::new(ErrorCode::ToolExecutionFailed, message);
        error.state.tool_failure = Some(TalosToolFailureDiagnostic { kind, gate_source });
        error
    }
}

impl fmt::Display for TalosError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind() {
            ErrorKind::Business => {
                write!(f, "{:?}: {}", self.state.error.code, self.state.diagnostic)
            }
            ErrorKind::Internal => write!(f, "Internal error: {}", self.state.diagnostic),
        }?;
        if let Some(source) = &self.state.source {
            write!(f, ": {source}")?;
        }
        Ok(())
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
