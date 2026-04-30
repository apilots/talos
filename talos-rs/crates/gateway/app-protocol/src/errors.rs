use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use talos_error::TalosError;
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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct AppServerErrorBody {
    pub code: AppServerErrorCode,
    pub message: String,
    #[serde(default)]
    pub data: Option<AppServerErrorData>,
}

impl AppServerErrorBody {
    pub fn new(
        code: AppServerErrorCode,
        message: impl Into<String>,
        data: Option<AppServerErrorData>,
    ) -> Self {
        Self {
            code,
            message: message.into(),
            data,
        }
    }

    pub fn parse_error(message: impl Into<String>) -> Self {
        Self::new(PARSE_ERROR, message, None)
    }

    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::new(INVALID_REQUEST, message, None)
    }

    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self::new(INVALID_PARAMS, message, None)
    }

    pub fn method_not_found(method: impl Into<String>) -> Self {
        let method = method.into();
        Self::new(
            METHOD_NOT_FOUND,
            format!("method not found: {method}"),
            None,
        )
    }

    pub fn not_initialized() -> Self {
        Self::new(NOT_INITIALIZED, "app-server is not initialized", None)
    }

    pub fn from_talos_error(error: TalosError) -> Self {
        match error {
            TalosError::AuthError(message) | TalosError::AuthenticationFailed(message) => {
                Self::new(
                    UNAUTHORIZED,
                    message,
                    Some(AppServerErrorData::Auth(AuthErrorData::Unauthorized)),
                )
            }
            TalosError::AuthorizationDenied => Self::new(
                FORBIDDEN,
                "authorization denied",
                Some(AppServerErrorData::Auth(AuthErrorData::Forbidden)),
            ),
            TalosError::ResourceNotFound(resource) => Self::new(
                NOT_FOUND,
                format!("resource not found: {resource}"),
                Some(AppServerErrorData::Resource(ResourceErrorData::NotFound {
                    resource,
                })),
            ),
            TalosError::Conflict(message)
            | TalosError::IdempotencyConflict(message)
            | TalosError::RetryActiveTurnRunning(message) => Self::new(
                CONFLICT,
                message.clone(),
                Some(AppServerErrorData::Turn(TurnErrorData::Conflict {
                    reason: message,
                })),
            ),
            TalosError::ValidationError(message) | TalosError::UnsupportedModel(message) => {
                Self::new(
                    VALIDATION_FAILED,
                    message.clone(),
                    Some(AppServerErrorData::Validation(ValidationErrorData {
                        reason: message,
                    })),
                )
            }
            TalosError::ResourceExpired(message) => Self::new(
                RESOURCE_EXPIRED,
                message.clone(),
                Some(AppServerErrorData::Resource(ResourceErrorData::Expired {
                    reason: message,
                })),
            ),
            other => Self::new(INTERNAL_ERROR, other.to_string(), None),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AppServerErrorData {
    Auth(AuthErrorData),
    Resource(ResourceErrorData),
    Validation(ValidationErrorData),
    Turn(TurnErrorData),
    Tool(ToolErrorData),
    Config(ConfigErrorData),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuthErrorData {
    Unauthorized,
    Forbidden,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResourceErrorData {
    NotFound { resource: String },
    Expired { reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct ValidationErrorData {
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TurnErrorData {
    Conflict { reason: String },
    NotInitialized,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct ToolErrorData {
    pub tool_name: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct ConfigErrorData {
    pub path: String,
    pub reason: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_common_not_found_error() {
        let body = AppServerErrorBody::from_talos_error(TalosError::ResourceNotFound(
            "session s1".to_string(),
        ));
        assert_eq!(body.code, NOT_FOUND);
        assert!(matches!(
            body.data,
            Some(AppServerErrorData::Resource(
                ResourceErrorData::NotFound { .. }
            ))
        ));
    }
}
