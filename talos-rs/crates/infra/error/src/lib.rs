use serde::{Deserialize, Serialize};
use thiserror::Error;

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

    #[error("Internal error: {0}")]
    InternalError(String),

    #[error("Validation error: {0}")]
    ValidationError(String),

    #[error("Tool error: {0}")]
    ToolError(String),

    #[error("Tool not found: {0}")]
    ToolNotFound(String),

    #[error("Client error: {0}")]
    ClientError(String),

    #[error("Cancelled: {0}")]
    Cancelled(String),
}

pub type TalosResult<T> = Result<T, TalosError>;
