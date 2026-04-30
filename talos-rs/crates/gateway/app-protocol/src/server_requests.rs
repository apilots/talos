use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum ServerRequest {
    #[serde(rename = "client.user_input.request")]
    ClientUserInputRequest(RequestUserInputParams),
    #[serde(rename = "client.auth.refresh")]
    ClientAuthRefresh(AuthRefreshParams),
    #[serde(rename = "client.approval.request")]
    ClientApprovalRequest(ClientApprovalRequestParams),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct RequestUserInputParams {
    pub prompt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct RequestUserInputResponse {
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct AuthRefreshParams {
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct AuthRefreshResponse {
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct ClientApprovalRequestParams {
    pub prompt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct ClientApprovalRequestResponse {
    pub approved: bool,
    #[serde(default)]
    pub reason: Option<String>,
}
