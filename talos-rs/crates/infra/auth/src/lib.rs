mod cloud;
mod pairing;
mod service;
mod session_cache;

pub use cloud::{
    CloudAuthService, CloudLoginCancelRequest, CloudLoginClientKind, CloudLoginService,
    CloudLoginStartRequest, CloudLoginStartResponse, CloudLoginStatusRequest,
    CloudLoginStatusResponse,
};
pub use pairing::{DevicePairing, PairingCode, PairingResult};
pub use service::{AuthCacheConfig, AuthService, OpenModeUserService, UserService};
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
    pub fn is_admin(self) -> bool {
        matches!(self, Self::Admin)
    }

    pub fn as_storage_value(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Admin => "admin",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AuthScope {
    SessionsRead,
    SessionsWrite,
    ProjectsRead,
    ProjectsWrite,
    MessagesWrite,
    ModelsRead,
    MemoryRead,
    MemoryWrite,
    WorkspaceDocsRead,
    WorkspaceDocsWrite,
    UsersRead,
    UsersWrite,
}

impl AuthScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SessionsRead => "sessions:read",
            Self::SessionsWrite => "sessions:write",
            Self::ProjectsRead => "projects:read",
            Self::ProjectsWrite => "projects:write",
            Self::MessagesWrite => "messages:write",
            Self::ModelsRead => "models:read",
            Self::MemoryRead => "memory:read",
            Self::MemoryWrite => "memory:write",
            Self::WorkspaceDocsRead => "workspace_docs:read",
            Self::WorkspaceDocsWrite => "workspace_docs:write",
            Self::UsersRead => "users:read",
            Self::UsersWrite => "users:write",
        }
    }

    pub fn from_str(value: &str) -> talos_error::TalosResult<Self> {
        match value {
            "sessions:read" => Ok(Self::SessionsRead),
            "sessions:write" => Ok(Self::SessionsWrite),
            "projects:read" => Ok(Self::ProjectsRead),
            "projects:write" => Ok(Self::ProjectsWrite),
            "messages:write" => Ok(Self::MessagesWrite),
            "models:read" => Ok(Self::ModelsRead),
            "memory:read" => Ok(Self::MemoryRead),
            "memory:write" => Ok(Self::MemoryWrite),
            "workspace_docs:read" => Ok(Self::WorkspaceDocsRead),
            "workspace_docs:write" => Ok(Self::WorkspaceDocsWrite),
            "users:read" => Ok(Self::UsersRead),
            "users:write" => Ok(Self::UsersWrite),
            other => Err(talos_error::TalosError::AuthError(format!(
                "unknown auth scope: {}",
                other
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestIdentity {
    pub user_id: String,
    pub username: Option<String>,
    pub role: UserRole,
    pub scopes: Vec<AuthScope>,
    pub auth_disabled: bool,
    pub access_token: Option<String>,
}

impl RequestIdentity {
    pub fn is_admin(&self) -> bool {
        self.role.is_admin()
    }

    pub fn bypasses_auth(&self) -> bool {
        self.auth_disabled
    }

    pub fn has_full_access(&self) -> bool {
        self.auth_disabled || self.is_admin()
    }

    pub fn with_access_token(mut self, token: impl Into<String>) -> Self {
        self.access_token = Some(token.into());
        self
    }

    pub fn mock_admin(user_id: impl Into<String>) -> Self {
        Self {
            user_id: user_id.into(),
            username: None,
            role: UserRole::Admin,
            scopes: default_scopes_for_role(UserRole::Admin),
            auth_disabled: false,
            access_token: None,
        }
    }

    pub fn auth_disabled() -> Self {
        Self {
            user_id: "anonymous".to_string(),
            username: Some("anonymous".to_string()),
            role: UserRole::User,
            scopes: default_scopes_for_role(UserRole::User),
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
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct Capabilities {
    pub is_admin: bool,
    pub role: UserRole,
    pub scopes: Vec<String>,
}

impl Capabilities {
    pub fn from_role(role: UserRole, scopes: &[AuthScope]) -> Self {
        Self {
            is_admin: role.is_admin(),
            role,
            scopes: scopes
                .iter()
                .map(AuthScope::as_str)
                .map(str::to_string)
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct AuthResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_at: u64,
    pub user: UserDto,
    pub capabilities: Capabilities,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct RegisterRequest {
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub email: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

pub fn default_scopes_for_role(role: UserRole) -> Vec<AuthScope> {
    let mut scopes = vec![
        AuthScope::SessionsRead,
        AuthScope::SessionsWrite,
        AuthScope::ProjectsRead,
        AuthScope::ProjectsWrite,
        AuthScope::MessagesWrite,
        AuthScope::ModelsRead,
        AuthScope::MemoryRead,
        AuthScope::MemoryWrite,
        AuthScope::WorkspaceDocsRead,
        AuthScope::WorkspaceDocsWrite,
    ];
    if role.is_admin() {
        scopes.push(AuthScope::UsersRead);
        scopes.push(AuthScope::UsersWrite);
    }
    scopes
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
