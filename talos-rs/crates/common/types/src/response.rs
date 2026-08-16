use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use talos_error::ApiError;
use ts_rs::TS;

/// Public application response envelope for request-response protocols.
///
/// Gateway adapters construct this envelope at public boundaries. Service,
/// store, runtime, and provider layers return domain responses or
/// `TalosError`; they must not construct protocol envelopes.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum ApiResponse<T> {
    Success { data: T },
    Error { error: ApiError },
}

impl<T> ApiResponse<T> {
    pub fn success(data: T) -> Self {
        Self::Success { data }
    }

    pub fn error(error: ApiError) -> Self {
        Self::Error { error }
    }
}

#[cfg(test)]
#[path = "response_tests.rs"]
mod tests;
