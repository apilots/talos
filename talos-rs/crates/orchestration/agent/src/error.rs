use std::collections::BTreeMap;

use talos_error::{ErrorCode, TalosError};
use thiserror::Error;

/// Agent-domain result used inside Harness, Runtime, and Coordinator adapters.
pub type AgentResult<T> = Result<T, AgentError>;

/// Stable error categories for the Agent framework.
///
/// These kinds are internal Agent-domain semantics. They must not be exported
/// as Gateway, SDK, app-protocol, or generated TypeScript DTOs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentErrorKind {
    AgentNotFound,
    AgentDisabled,
    AgentUnavailable,
    RuntimeUnavailable,
    CapabilityUnsupported,
    InvalidExecutionContext,
    RunFailure(ErrorCode),
    RunNotFound,
    ActiveTurnRunning,
    RunConflict,
    InputRejected,
    ApprovalExpired,
    Interrupted,
    RuntimeError,
    Timeout,
    ContinuationUnsupported,
    ContinuationNotFound,
    ContinuationExpired,
    ContinuationInvalid,
    ContinuationConflict,
    BackendTaskCompleted,
    BackendTaskLost,
}

/// Internal Agent-domain error object.
///
/// AgentError carries framework semantics between Runtime, Harness, and
/// Coordinator. Service-facing boundaries must map it back to TalosError or a
/// product error code before reaching Gateway or SDK callers.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
#[error("{kind:?}: {message}")]
pub struct AgentError {
    /// Stable framework category for routing and product error mapping.
    pub kind: AgentErrorKind,
    /// Human-readable diagnostic message.
    pub message: String,
    /// Whether the caller may retry the same operation safely.
    pub retryable: bool,
    /// Structured internal diagnostics safe for logs.
    pub details: BTreeMap<String, String>,
    /// Runtime-private source message kept inside the Agent domain.
    pub source_message: Option<String>,
}

impl AgentError {
    /// Build an Agent error with an explicit semantic kind.
    pub fn new(kind: AgentErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            retryable: false,
            details: BTreeMap::new(),
            source_message: None,
        }
    }

    /// Mark whether this error is retryable.
    pub fn retryable(mut self, retryable: bool) -> Self {
        self.retryable = retryable;
        self
    }

    /// Attach a structured diagnostic field.
    pub fn with_detail(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.details.insert(key.into(), value.into());
        self
    }

    /// Attach runtime-private source context for internal diagnostics.
    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source_message = Some(source.into());
        self
    }

    /// Error for unsupported standard Agent capabilities.
    pub fn capability_unsupported(agent_id: &str, capability: &str) -> Self {
        Self::new(
            AgentErrorKind::CapabilityUnsupported,
            format!("agent `{agent_id}` does not support capability `{capability}`"),
        )
        .with_detail("agent_id", agent_id)
        .with_detail("capability", capability)
    }

    /// Error for invalid cross-layer Agent execution context.
    pub fn invalid_execution_context(message: impl Into<String>) -> Self {
        Self::new(AgentErrorKind::InvalidExecutionContext, message)
    }

    /// Error for an Agent run that cannot be found or controlled.
    pub fn run_not_found(message: impl Into<String>) -> Self {
        Self::new(AgentErrorKind::RunNotFound, message)
    }

    /// Error for conflicting Agent run lifecycle state.
    pub fn run_conflict(message: impl Into<String>) -> Self {
        Self::new(AgentErrorKind::RunConflict, message)
    }

    /// Error for rejected structured input to an Agent run.
    pub fn input_rejected(message: impl Into<String>) -> Self {
        Self::new(AgentErrorKind::InputRejected, message)
    }

    /// Error for unexpected runtime or backend failures.
    pub fn runtime_error(message: impl Into<String>) -> Self {
        Self::new(AgentErrorKind::RuntimeError, message)
    }

    /// Convert the internal Agent error into the product error boundary.
    pub fn into_talos_error(self) -> TalosError {
        match self.kind {
            AgentErrorKind::AgentNotFound => {
                TalosError::public(talos_error::ErrorCode::ResourceNotFound, self.message)
            }
            AgentErrorKind::RunNotFound
            | AgentErrorKind::ContinuationNotFound
            | AgentErrorKind::BackendTaskCompleted => {
                TalosError::public(talos_error::ErrorCode::ResourceNotFound, self.message)
            }
            AgentErrorKind::ActiveTurnRunning => {
                TalosError::public(talos_error::ErrorCode::ActiveTurnRunning, self.message)
            }
            AgentErrorKind::RunConflict | AgentErrorKind::ContinuationConflict => {
                TalosError::public(talos_error::ErrorCode::Conflict, self.message)
            }
            AgentErrorKind::CapabilityUnsupported
            | AgentErrorKind::InvalidExecutionContext
            | AgentErrorKind::InputRejected
            | AgentErrorKind::AgentDisabled
            | AgentErrorKind::ContinuationUnsupported
            | AgentErrorKind::ContinuationExpired
            | AgentErrorKind::ContinuationInvalid
            | AgentErrorKind::ApprovalExpired => {
                TalosError::public(talos_error::ErrorCode::InvalidRequest, self.message)
            }
            AgentErrorKind::Interrupted => {
                TalosError::public(talos_error::ErrorCode::Cancelled, self.message)
            }
            AgentErrorKind::Timeout => {
                TalosError::public(talos_error::ErrorCode::Timeout, self.message)
            }
            AgentErrorKind::AgentUnavailable
            | AgentErrorKind::RuntimeUnavailable
            | AgentErrorKind::BackendTaskLost => {
                TalosError::public(talos_error::ErrorCode::ServiceUnavailable, self.message)
            }
            AgentErrorKind::RunFailure(ErrorCode::ProviderUnavailable)
            | AgentErrorKind::RunFailure(ErrorCode::ServiceUnavailable) => {
                TalosError::public(talos_error::ErrorCode::ServiceUnavailable, self.message)
            }
            AgentErrorKind::RunFailure(ErrorCode::ContextWindowExceeded) => {
                TalosError::public(talos_error::ErrorCode::ContextWindowExceeded, self.message)
            }
            AgentErrorKind::RunFailure(ErrorCode::ContextCompactionFailed) => TalosError::public(
                talos_error::ErrorCode::ContextCompactionFailed,
                self.message,
            ),
            AgentErrorKind::RunFailure(ErrorCode::PermissionDenied)
            | AgentErrorKind::RunFailure(ErrorCode::Unauthenticated) => {
                TalosError::public(talos_error::ErrorCode::Unauthenticated, self.message)
            }
            AgentErrorKind::RunFailure(ErrorCode::ToolExecutionFailed) => {
                TalosError::public(talos_error::ErrorCode::ToolExecutionFailed, self.message)
            }
            AgentErrorKind::RunFailure(ErrorCode::ActiveTurnRunning) => {
                TalosError::public(talos_error::ErrorCode::ActiveTurnRunning, self.message)
            }
            AgentErrorKind::RunFailure(ErrorCode::ResourceNotFound)
            | AgentErrorKind::RunFailure(ErrorCode::SessionNotFound)
            | AgentErrorKind::RunFailure(ErrorCode::PathNotFound) => {
                TalosError::public(talos_error::ErrorCode::ResourceNotFound, self.message)
            }
            AgentErrorKind::RunFailure(ErrorCode::ResourceExpired)
            | AgentErrorKind::RunFailure(ErrorCode::EventCursorExpired) => {
                TalosError::public(talos_error::ErrorCode::ResourceExpired, self.message)
            }
            AgentErrorKind::RunFailure(ErrorCode::Conflict)
            | AgentErrorKind::RunFailure(ErrorCode::IdempotencyConflict)
            | AgentErrorKind::RunFailure(ErrorCode::RetrySourceCompleted)
            | AgentErrorKind::RunFailure(ErrorCode::RetrySourceMismatch)
            | AgentErrorKind::RunFailure(ErrorCode::AlreadyExists) => {
                TalosError::public(talos_error::ErrorCode::Conflict, self.message)
            }
            AgentErrorKind::RunFailure(ErrorCode::Cancelled) => {
                TalosError::public(talos_error::ErrorCode::Cancelled, self.message)
            }
            AgentErrorKind::RunFailure(ErrorCode::Timeout) => {
                TalosError::public(talos_error::ErrorCode::Timeout, self.message)
            }
            AgentErrorKind::RunFailure(ErrorCode::FileInputRejected)
            | AgentErrorKind::RunFailure(ErrorCode::InvalidRequest)
            | AgentErrorKind::RunFailure(ErrorCode::InvalidRetryRequest)
            | AgentErrorKind::RunFailure(ErrorCode::InvalidCursor)
            | AgentErrorKind::RunFailure(ErrorCode::FsDisabled)
            | AgentErrorKind::RunFailure(ErrorCode::InvalidPath)
            | AgentErrorKind::RunFailure(ErrorCode::PathOutsideRoot)
            | AgentErrorKind::RunFailure(ErrorCode::SymlinkNotSupported)
            | AgentErrorKind::RunFailure(ErrorCode::NotDirectory)
            | AgentErrorKind::RunFailure(ErrorCode::InvalidName)
            | AgentErrorKind::RunFailure(ErrorCode::UploadTooLarge)
            | AgentErrorKind::RunFailure(ErrorCode::UnsupportedMediaType)
            | AgentErrorKind::RunFailure(ErrorCode::AttachmentLimitExceeded)
            | AgentErrorKind::RunFailure(ErrorCode::UnsupportedAttachment) => {
                TalosError::public(talos_error::ErrorCode::InvalidRequest, self.message)
            }
            AgentErrorKind::RunFailure(ErrorCode::InternalError)
            | AgentErrorKind::RunFailure(ErrorCode::UsageLimitExceeded)
            | AgentErrorKind::RunFailure(ErrorCode::SessionBudgetExceeded) => {
                TalosError::internal(self.message)
            }
            AgentErrorKind::RuntimeError => TalosError::internal(self.message),
        }
    }
}

impl From<TalosError> for AgentError {
    fn from(error: TalosError) -> Self {
        let code = error.error_code();
        let message = error.diagnostic_message().to_string();
        match code {
            ErrorCode::ResourceNotFound | ErrorCode::SessionNotFound | ErrorCode::PathNotFound => {
                Self::run_not_found(message)
            }
            ErrorCode::ActiveTurnRunning => Self::new(AgentErrorKind::ActiveTurnRunning, message),
            ErrorCode::Conflict | ErrorCode::IdempotencyConflict | ErrorCode::AlreadyExists => {
                Self::run_conflict(message)
            }
            ErrorCode::InvalidRequest
            | ErrorCode::InvalidRetryRequest
            | ErrorCode::InvalidCursor
            | ErrorCode::RetrySourceCompleted
            | ErrorCode::RetrySourceMismatch => Self::invalid_execution_context(message),
            ErrorCode::ContextWindowExceeded => Self::new(
                AgentErrorKind::RunFailure(ErrorCode::ContextWindowExceeded),
                message,
            ),
            ErrorCode::ContextCompactionFailed => Self::new(
                AgentErrorKind::RunFailure(ErrorCode::ContextCompactionFailed),
                message,
            ),
            ErrorCode::PermissionDenied | ErrorCode::Unauthenticated => Self::new(
                AgentErrorKind::RunFailure(ErrorCode::PermissionDenied),
                message,
            ),
            ErrorCode::ToolExecutionFailed => Self::new(
                AgentErrorKind::RunFailure(ErrorCode::ToolExecutionFailed),
                message,
            ),
            ErrorCode::FileInputRejected
            | ErrorCode::UploadTooLarge
            | ErrorCode::UnsupportedMediaType
            | ErrorCode::AttachmentLimitExceeded
            | ErrorCode::UnsupportedAttachment => Self::new(
                AgentErrorKind::RunFailure(ErrorCode::FileInputRejected),
                message,
            ),
            ErrorCode::Timeout => Self::new(AgentErrorKind::Timeout, message),
            ErrorCode::ProviderUnavailable => Self::new(
                AgentErrorKind::RunFailure(ErrorCode::ProviderUnavailable),
                message,
            ),
            ErrorCode::ServiceUnavailable => Self::new(
                AgentErrorKind::RunFailure(ErrorCode::ProviderUnavailable),
                message,
            ),
            ErrorCode::Cancelled => Self::new(AgentErrorKind::Interrupted, message),
            ErrorCode::ResourceExpired | ErrorCode::EventCursorExpired => Self::new(
                AgentErrorKind::RunFailure(ErrorCode::ResourceExpired),
                message,
            ),
            ErrorCode::FsDisabled
            | ErrorCode::InvalidPath
            | ErrorCode::PathOutsideRoot
            | ErrorCode::SymlinkNotSupported
            | ErrorCode::NotDirectory
            | ErrorCode::InvalidName => Self::invalid_execution_context(message),
            ErrorCode::SessionBudgetExceeded
            | ErrorCode::UsageLimitExceeded
            | ErrorCode::InternalError => {
                Self::runtime_error(message).with_source(error.to_string())
            }
        }
    }
}

#[cfg(test)]
#[path = "error_tests.rs"]
mod tests;
