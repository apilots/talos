use schemars::schema::{InstanceType, Schema, SchemaObject, SingleOrVec};
use schemars::{gen::SchemaGenerator, JsonSchema};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum AppServerErrorCode {
    ParseError = -32700,
    InvalidRequest = -32600,
    MethodNotFound = -32601,
    InvalidParams = -32602,
    InternalError = -32603,
    NotInitialized = -32001,
    AlreadyInitialized = -32002,
}

impl AppServerErrorCode {
    pub const ALL: [Self; 7] = [
        Self::ParseError,
        Self::InvalidRequest,
        Self::MethodNotFound,
        Self::InvalidParams,
        Self::InternalError,
        Self::NotInitialized,
        Self::AlreadyInitialized,
    ];

    pub const fn as_i32(self) -> i32 {
        self as i32
    }
}

impl TryFrom<i32> for AppServerErrorCode {
    type Error = &'static str;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            -32700 => Ok(Self::ParseError),
            -32600 => Ok(Self::InvalidRequest),
            -32601 => Ok(Self::MethodNotFound),
            -32602 => Ok(Self::InvalidParams),
            -32603 => Ok(Self::InternalError),
            -32001 => Ok(Self::NotInitialized),
            -32002 => Ok(Self::AlreadyInitialized),
            _ => Err("unknown JSON-RPC protocol error code"),
        }
    }
}

impl Serialize for AppServerErrorCode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_i32(self.as_i32())
    }
}

impl<'de> Deserialize<'de> for AppServerErrorCode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = i32::deserialize(deserializer)?;
        Self::try_from(value).map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for AppServerErrorCode {
    fn schema_name() -> String {
        "AppServerErrorCode".to_string()
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        Schema::Object(SchemaObject {
            instance_type: Some(SingleOrVec::Single(Box::new(InstanceType::Integer))),
            enum_values: Some(
                Self::ALL
                    .into_iter()
                    .map(|code| serde_json::json!(code.as_i32()))
                    .collect(),
            ),
            ..SchemaObject::default()
        })
    }
}

pub const PARSE_ERROR: AppServerErrorCode = AppServerErrorCode::ParseError;
pub const INVALID_REQUEST: AppServerErrorCode = AppServerErrorCode::InvalidRequest;
pub const METHOD_NOT_FOUND: AppServerErrorCode = AppServerErrorCode::MethodNotFound;
pub const INVALID_PARAMS: AppServerErrorCode = AppServerErrorCode::InvalidParams;
pub const INTERNAL_ERROR: AppServerErrorCode = AppServerErrorCode::InternalError;
pub const NOT_INITIALIZED: AppServerErrorCode = AppServerErrorCode::NotInitialized;
pub const ALREADY_INITIALIZED: AppServerErrorCode = AppServerErrorCode::AlreadyInitialized;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct AppServerErrorBody {
    #[ts(type = "AppServerErrorCode")]
    pub code: AppServerErrorCode,
    pub message: String,
}

impl std::fmt::Display for AppServerErrorBody {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{} ({})", self.message, self.code.as_i32())
    }
}

impl std::error::Error for AppServerErrorBody {}

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
}

#[cfg(test)]
#[path = "errors_tests.rs"]
mod tests;
