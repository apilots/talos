use std::collections::BTreeMap;

use talos_error::{TalosError, TalosRunErrorCode};
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
    RunFailure(TalosRunErrorCode),
    RunNotFound,
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
            AgentErrorKind::AgentNotFound => TalosError::AgentNotFound(self.message),
            AgentErrorKind::RunNotFound
            | AgentErrorKind::ContinuationNotFound
            | AgentErrorKind::BackendTaskCompleted => TalosError::ResourceNotFound(self.message),
            AgentErrorKind::RunConflict | AgentErrorKind::ContinuationConflict => {
                TalosError::Conflict(self.message)
            }
            AgentErrorKind::CapabilityUnsupported
            | AgentErrorKind::InvalidExecutionContext
            | AgentErrorKind::InputRejected
            | AgentErrorKind::AgentDisabled
            | AgentErrorKind::ContinuationUnsupported
            | AgentErrorKind::ContinuationExpired
            | AgentErrorKind::ContinuationInvalid
            | AgentErrorKind::ApprovalExpired => TalosError::ValidationError(self.message),
            AgentErrorKind::Interrupted => TalosError::Cancelled(self.message),
            AgentErrorKind::Timeout => TalosError::Timeout(self.message),
            AgentErrorKind::AgentUnavailable
            | AgentErrorKind::RuntimeUnavailable
            | AgentErrorKind::BackendTaskLost => TalosError::RuntimeUnavailable(self.message),
            AgentErrorKind::RunFailure(TalosRunErrorCode::ProviderUnavailable) => {
                TalosError::RuntimeUnavailable(self.message)
            }
            AgentErrorKind::RunFailure(TalosRunErrorCode::ContextWindowExceeded) => {
                TalosError::ContextWindowExceeded(self.message)
            }
            AgentErrorKind::RunFailure(TalosRunErrorCode::ContextCompactionFailed) => {
                TalosError::ContextCompactionFailed(self.message)
            }
            AgentErrorKind::RunFailure(TalosRunErrorCode::PermissionDenied) => {
                TalosError::AuthError(self.message)
            }
            AgentErrorKind::RunFailure(TalosRunErrorCode::ToolFailure) => {
                TalosError::ToolError(self.message)
            }
            AgentErrorKind::RunFailure(TalosRunErrorCode::InternalError)
            | AgentErrorKind::RunFailure(TalosRunErrorCode::UsageLimitExceeded)
            | AgentErrorKind::RunFailure(TalosRunErrorCode::SessionBudgetExceeded) => {
                TalosError::InternalError(self.message)
            }
            AgentErrorKind::RuntimeError => TalosError::InternalError(self.message),
        }
    }
}

impl From<TalosError> for AgentError {
    fn from(error: TalosError) -> Self {
        match error {
            TalosError::AgentNotFound(message) => Self::new(AgentErrorKind::AgentNotFound, message),
            TalosError::ResourceNotFound(message) => Self::run_not_found(message),
            TalosError::Conflict(message)
            | TalosError::IdempotencyConflict(message)
            | TalosError::ActiveTurnRunning(message)
            | TalosError::RetryActiveTurnRunning(message) => Self::run_conflict(message),
            TalosError::ValidationError(message)
            | TalosError::RetrySourceCompleted(message)
            | TalosError::RetrySourceMismatch(message) => Self::invalid_execution_context(message),
            TalosError::ContextWindowExceeded(message) => Self::new(
                AgentErrorKind::RunFailure(TalosRunErrorCode::ContextWindowExceeded),
                message,
            ),
            TalosError::ContextCompactionFailed(message) => Self::new(
                AgentErrorKind::RunFailure(TalosRunErrorCode::ContextCompactionFailed),
                message,
            ),
            TalosError::AuthorizationDenied => Self::new(
                AgentErrorKind::RunFailure(TalosRunErrorCode::PermissionDenied),
                "authorization denied",
            ),
            TalosError::AuthenticationFailed(message) | TalosError::AuthError(message) => {
                Self::new(
                    AgentErrorKind::RunFailure(TalosRunErrorCode::PermissionDenied),
                    message,
                )
            }
            TalosError::ToolError(message)
            | TalosError::ToolFailure { message, .. }
            | TalosError::ToolNotFound(message) => Self::new(
                AgentErrorKind::RunFailure(TalosRunErrorCode::ToolFailure),
                message,
            ),
            TalosError::Timeout(message) => Self::new(AgentErrorKind::Timeout, message),
            TalosError::UnsupportedModel(message) => Self::new(
                AgentErrorKind::RunFailure(TalosRunErrorCode::ProviderUnavailable),
                message,
            ),
            TalosError::RuntimeUnavailable(message) | TalosError::TransportUnavailable(message) => {
                Self::new(
                    AgentErrorKind::RunFailure(TalosRunErrorCode::ProviderUnavailable),
                    message,
                )
            }
            TalosError::Cancelled(message) => Self::new(AgentErrorKind::Interrupted, message),
            other => Self::runtime_error(other.to_string()).with_source(other.to_string()),
        }
    }
}

#[cfg(test)]
#[path = "error_tests.rs"]
mod tests;
