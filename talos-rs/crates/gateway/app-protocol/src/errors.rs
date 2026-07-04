use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use talos_error::{TalosError, TalosFsErrorKind};
use ts_rs::TS;

pub type AppServerErrorCode = i32;

pub const PARSE_ERROR: AppServerErrorCode = -32700;
pub const INVALID_REQUEST: AppServerErrorCode = -32600;
pub const METHOD_NOT_FOUND: AppServerErrorCode = -32601;
pub const INVALID_PARAMS: AppServerErrorCode = -32602;
pub const INTERNAL_ERROR: AppServerErrorCode = -32603;

pub const NOT_INITIALIZED: AppServerErrorCode = -32001;
pub const ALREADY_INITIALIZED: AppServerErrorCode = -32002;
pub const UNAUTHORIZED: AppServerErrorCode = -32003;
pub const FORBIDDEN: AppServerErrorCode = -32004;
pub const CONFLICT: AppServerErrorCode = -32005;
pub const NOT_FOUND: AppServerErrorCode = -32006;
pub const VALIDATION_FAILED: AppServerErrorCode = -32007;
pub const RESOURCE_EXPIRED: AppServerErrorCode = -32008;
pub const RUNTIME_UNAVAILABLE: AppServerErrorCode = -32009;
pub const TRANSPORT_UNAVAILABLE: AppServerErrorCode = -32010;
pub const REQUEST_TIMEOUT: AppServerErrorCode = -32011;
pub const CANCELLED: AppServerErrorCode = -32012;
pub const ACTIVE_TURN_RUNNING: AppServerErrorCode = -32013;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct AppServerErrorBody {
    pub code: AppServerErrorCode,
    pub message: String,
}

impl AppServerErrorBody {
    pub fn new(code: AppServerErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn parse_error(message: impl Into<String>) -> Self {
        Self::new(PARSE_ERROR, message)
    }

    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::new(INVALID_REQUEST, message)
    }

    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self::new(INVALID_PARAMS, message)
    }

    pub fn method_not_found(method: impl Into<String>) -> Self {
        let method = method.into();
        Self::new(METHOD_NOT_FOUND, format!("method not found: {method}"))
    }

    pub fn not_initialized() -> Self {
        Self::new(NOT_INITIALIZED, "app-server is not initialized")
    }

    pub fn from_talos_error(error: TalosError) -> Self {
        match error {
            TalosError::AuthError(message) | TalosError::AuthenticationFailed(message) => {
                Self::new(UNAUTHORIZED, message)
            }
            TalosError::AuthorizationDenied => Self::new(FORBIDDEN, "authorization denied"),
            TalosError::ResourceNotFound(resource)
            | TalosError::AgentNotFound(resource)
            | TalosError::RouteNotFound(resource)
            | TalosError::ToolNotFound(resource) => {
                Self::new(NOT_FOUND, format!("resource not found: {resource}"))
            }
            TalosError::ActiveTurnRunning(message) => Self::new(ACTIVE_TURN_RUNNING, message),
            TalosError::Conflict(message)
            | TalosError::IdempotencyConflict(message)
            | TalosError::RetryActiveTurnRunning(message) => Self::new(CONFLICT, message),
            TalosError::ValidationError(message)
            | TalosError::UnsupportedModel(message)
            | TalosError::InvalidConfiguration(message)
            | TalosError::ConfigError(message) => Self::new(VALIDATION_FAILED, message),
            TalosError::FsError { kind, message } => {
                Self::new(app_server_code_for_fs(kind), message)
            }
            TalosError::ResourceExpired(message) => Self::new(RESOURCE_EXPIRED, message),
            TalosError::Timeout(message) => Self::new(REQUEST_TIMEOUT, message),
            TalosError::RuntimeUnavailable(message) => Self::new(RUNTIME_UNAVAILABLE, message),
            TalosError::NetworkError(message)
            | TalosError::ClientError(message)
            | TalosError::TransportUnavailable(message) => {
                Self::new(TRANSPORT_UNAVAILABLE, message)
            }
            TalosError::Cancelled(message) => Self::new(CANCELLED, message),
            other => Self::new(INTERNAL_ERROR, other.to_string()),
        }
    }
}

fn app_server_code_for_fs(kind: TalosFsErrorKind) -> AppServerErrorCode {
    match kind {
        TalosFsErrorKind::FsDisabled | TalosFsErrorKind::PathOutsideRoot => FORBIDDEN,
        TalosFsErrorKind::PathNotFound => NOT_FOUND,
        TalosFsErrorKind::AlreadyExists => CONFLICT,
        TalosFsErrorKind::InvalidPath
        | TalosFsErrorKind::SymlinkNotSupported
        | TalosFsErrorKind::NotDirectory
        | TalosFsErrorKind::InvalidName => VALIDATION_FAILED,
    }
}

#[cfg(test)]
#[path = "errors_tests.rs"]
mod tests;
