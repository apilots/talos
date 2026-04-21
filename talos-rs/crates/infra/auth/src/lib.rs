mod jwt;
mod pairing;
mod password;
mod service;
mod store;

pub use jwt::{AccessTokenClaims, IssuedAccessToken, JwtAuth, JwtConfig};
pub use pairing::{DevicePairing, PairingCode, PairingResult};
pub use password::{validate_password, PasswordService};
pub use service::{AuthCacheConfig, AuthManager, AuthService, UserService};
pub use store::{
    DatabaseUserStore, DatabaseUserTokenStore, InMemoryUserStore, InMemoryUserTokenStore,
    UserStore, UserTokenStore,
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
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

    pub fn mock_admin(user_id: impl Into<String>) -> Self {
        Self {
            user_id: user_id.into(),
            username: None,
            role: UserRole::Admin,
            scopes: default_scopes_for_role(UserRole::Admin),
            auth_disabled: false,
        }
    }

    pub fn auth_disabled() -> Self {
        Self {
            user_id: "auth-disabled".to_string(),
            username: None,
            role: UserRole::User,
            scopes: default_scopes_for_role(UserRole::Admin),
            auth_disabled: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserRecord {
    pub user_id: String,
    pub username: String,
    pub email: Option<String>,
    #[serde(skip_serializing)]
    pub password_hash: String,
    pub role: UserRole,
    pub token_version: u64,
    pub disabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct CreateUserRecord {
    pub user_id: String,
    pub username: String,
    pub email: Option<String>,
    pub password_hash: String,
    pub role: UserRole,
    pub token_version: u64,
    pub disabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

impl From<CreateUserRecord> for UserRecord {
    fn from(value: CreateUserRecord) -> Self {
        Self {
            user_id: value.user_id,
            username: value.username,
            email: value.email,
            password_hash: value.password_hash,
            role: value.role,
            token_version: value.token_version,
            disabled: value.disabled,
            created_at: value.created_at,
            updated_at: value.updated_at,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct UserPatch {
    pub username: Option<String>,
    pub email: Option<String>,
    pub role: Option<UserRole>,
    pub disabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserDto {
    pub user_id: String,
    pub username: String,
    pub email: Option<String>,
    pub role: UserRole,
    pub disabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserTokenMeta {
    pub token_id: String,
    pub name: String,
    pub prefix: String,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
    pub expires_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateUserTokenResult {
    pub token_id: String,
    pub raw_token: String,
    pub prefix: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateUserTokenRequest {
    pub name: String,
}

impl From<UserRecord> for UserDto {
    fn from(value: UserRecord) -> Self {
        Self {
            user_id: value.user_id,
            username: value.username,
            email: value.email,
            role: value.role,
            disabled: value.disabled,
            created_at: value.created_at,
            updated_at: value.updated_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_at: u64,
    pub user: UserDto,
    pub capabilities: Capabilities,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub email: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateCurrentUserRequest {
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminCreateUserRequest {
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub email: Option<String>,
    pub role: UserRole,
    #[serde(default)]
    pub disabled: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AdminPatchUserRequest {
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub role: Option<UserRole>,
    #[serde(default)]
    pub disabled: Option<bool>,
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
