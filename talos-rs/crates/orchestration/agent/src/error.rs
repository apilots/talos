use talos_error::{ErrorCode, TalosError};
use thiserror::Error;

/// Agent-domain result used inside Provider, Harness, and Coordinator adapters.
pub type AgentResult<T> = Result<T, AgentError>;

/// Agent lifecycle metadata composed with the original product failure.
///
/// Only operation-owned retry safety belongs here. Product classification,
/// projection, diagnostic sources, and capture identity belong to TalosError.
#[derive(Debug, Clone, Error)]
#[error("{error}")]
pub struct AgentError {
    pub retryable: bool,
    #[source]
    error: TalosError,
}

impl AgentError {
    /// Build a framework failure with its product meaning established once.
    #[track_caller]
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        TalosError::new(code, message).into()
    }

    /// Retain a dependency source when constructing a framework failure.
    #[track_caller]
    pub fn from_source(
        code: ErrorCode,
        message: impl Into<String>,
        source: impl Into<Box<dyn std::error::Error + Send + Sync>>,
    ) -> Self {
        TalosError::from_source(code, message, source).into()
    }

    /// Mark operation-owned retry safety without changing the product error.
    pub fn retryable(mut self, retryable: bool) -> Self {
        self.retryable = retryable;
        self
    }

    /// Add diagnostic context without discarding sources or capturing again.
    pub fn context(mut self, context: impl std::fmt::Display) -> Self {
        self.error = self.error.context(context);
        self
    }

    /// Borrow the original failure for projection or boundary reporting.
    pub fn error(&self) -> &TalosError {
        &self.error
    }

    /// Return the original product failure without remapping its code.
    pub fn into_talos_error(self) -> TalosError {
        self.error
    }

    /// Error for unsupported standard Agent capabilities.
    #[track_caller]
    pub fn capability_unsupported(agent_id: &str, capability: &str) -> Self {
        Self::new(
            talos_error::ErrorCode::InvalidRequest,
            format!("agent `{agent_id}` does not support capability `{capability}`"),
        )
    }

    /// Error for invalid cross-layer Agent execution context.
    #[track_caller]
    pub fn invalid_execution_context(message: impl Into<String>) -> Self {
        Self::new(talos_error::ErrorCode::InvalidRequest, message)
    }

    /// Error for an Agent run that cannot be found or controlled.
    #[track_caller]
    pub fn run_not_found(message: impl Into<String>) -> Self {
        Self::new(talos_error::ErrorCode::ResourceNotFound, message)
    }

    /// Error for conflicting Agent run lifecycle state.
    #[track_caller]
    pub fn run_conflict(message: impl Into<String>) -> Self {
        Self::new(talos_error::ErrorCode::Conflict, message)
    }

    /// Error for rejected structured input to an Agent run.
    #[track_caller]
    pub fn input_rejected(message: impl Into<String>) -> Self {
        Self::new(talos_error::ErrorCode::InvalidRequest, message)
    }

    /// Error for unexpected Provider or backend failures.
    #[track_caller]
    pub fn provider_error(message: impl Into<String>) -> Self {
        Self::new(talos_error::ErrorCode::InternalError, message)
    }
}

impl From<TalosError> for AgentError {
    fn from(error: TalosError) -> Self {
        Self {
            retryable: false,
            error,
        }
    }
}

#[cfg(test)]
#[path = "error_tests.rs"]
mod tests;
