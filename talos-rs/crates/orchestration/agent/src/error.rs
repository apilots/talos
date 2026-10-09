use talos_error::TalosError;

/// Operation-owned delivery policy for a terminal Agent failure.
///
/// Product classification, diagnostics, and public projection remain owned by
/// `TalosError`; this wrapper exists only while publishing a terminal event.
#[derive(Debug, Clone)]
pub struct AgentRunFailure {
    retryable: bool,
    error: TalosError,
}

impl AgentRunFailure {
    /// Mark whether the failed operation can be retried safely.
    pub fn retryable(mut self) -> Self {
        self.retryable = true;
        self
    }

    /// Return whether retrying the failed operation is safe.
    pub const fn is_retryable(&self) -> bool {
        self.retryable
    }

    /// Borrow the product failure for logging and projection.
    pub fn error(&self) -> &TalosError {
        &self.error
    }

    /// Return the product failure after terminal metadata is projected.
    pub fn into_error(self) -> TalosError {
        self.error
    }
}

impl From<TalosError> for AgentRunFailure {
    fn from(error: TalosError) -> Self {
        Self {
            retryable: false,
            error,
        }
    }
}
