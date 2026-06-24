use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use talos_auth::UserDto;
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq, Default)]
pub struct AuthLogoutRequest {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct AuthLogoutResponse {
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq, Default)]
pub struct AuthSessionReadRequest {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct AuthSessionReadResponse {
    #[serde(default)]
    pub session: Option<AuthSessionSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq, Default)]
pub struct AuthSessionClearRequest {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct AuthSessionClearResponse {
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct CloudLoginCancelResponse {
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct AuthSessionSnapshot {
    pub schema_version: u32,
    pub cloud_api_base_url: String,
    pub access_token: String,
    pub token_type: String,
    #[serde(default)]
    pub expires_at_ms: Option<String>,
    pub last_used_at_ms: String,
    pub user: AuthSessionUserSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct AuthSessionUserSnapshot {
    pub user_id: String,
    pub username: String,
    #[serde(default)]
    pub email: Option<String>,
    pub role: talos_auth::UserRole,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq, Default)]
pub struct UserCurrentRequest {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct UserCurrentResponse {
    pub user: UserDto,
}
