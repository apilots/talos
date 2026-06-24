mod cloud;
mod service;
mod session_cache;

pub use cloud::{
    CloudAuthService, CloudLoginCancelRequest, CloudLoginClientKind, CloudLoginPersistence,
    CloudLoginService, CloudLoginStartRequest, CloudLoginStartResponse, CloudLoginStatusRequest,
    CloudLoginStatusResponse,
};
pub use service::{AuthService, OpenModeUserService, UserService};
pub use session_cache::{
    CloudSessionCache, CloudSessionCacheEntry, CloudSessionCacheManager, CloudSessionUserSnapshot,
    DEFAULT_CLOUD_SESSION_IDLE_TIMEOUT_MS, DEFAULT_CLOUD_SESSION_TOUCH_INTERVAL_MS,
};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum UserRole {
    #[default]
    User,
    Admin,
}

impl UserRole {
    pub fn as_storage_value(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Admin => "admin",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestIdentity {
    pub user_id: String,
    pub username: Option<String>,
    pub role: UserRole,
    pub auth_disabled: bool,
    pub access_token: Option<String>,
}

impl RequestIdentity {
    pub fn bypasses_auth(&self) -> bool {
        self.auth_disabled
    }

    pub fn with_access_token(mut self, token: impl Into<String>) -> Self {
        self.access_token = Some(token.into());
        self
    }

    pub fn auth_disabled() -> Self {
        Self {
            user_id: "anonymous".to_string(),
            username: Some("anonymous".to_string()),
            role: UserRole::User,
            auth_disabled: true,
            access_token: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct UserDto {
    pub user_id: String,
    pub username: String,
    pub email: Option<String>,
    pub role: UserRole,
    pub disabled: bool,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number")]
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct AuthResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_at: u64,
    pub user: UserDto,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_disabled_identity_uses_anonymous_user_snapshot() {
        let identity = RequestIdentity::auth_disabled();

        assert_eq!(identity.user_id, "anonymous");
        assert_eq!(identity.username.as_deref(), Some("anonymous"));
        assert!(identity.auth_disabled);
    }
}
