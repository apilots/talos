use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum ServerRequest {
    #[serde(rename = "client.user_input.request")]
    ClientUserInputRequest(RequestUserInputParams),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct RequestUserInputParams {
    pub prompt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct RequestUserInputResponse {
    pub text: String,
}
