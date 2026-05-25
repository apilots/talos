use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use moka::sync::Cache;
use rand::RngCore;
use sha2::{Digest, Sha256};
use talos_error::{TalosError, TalosResult};

use crate::password::PasswordService;
use crate::store::{CreateUserTokenRecord, UserStore, UserTokenRecord, UserTokenStore};
use crate::{
    default_scopes_for_role, AdminCreateUserRequest, AdminPatchUserRequest, AuthResponse,
    Capabilities, CreateUserRecord, CreateUserTokenResult, JwtAuth, LoginRequest, RegisterRequest,
    RequestIdentity, UpdateCurrentUserRequest, UserDto, UserPatch, UserRecord, UserRole,
    UserTokenMeta,
};

/// Prefix used by long-lived user token secrets exposed in Authorization: Bearer.
const USER_TOKEN_PREFIX: &str = "tls_";
/// Random byte length for user token secret generation before base64 encoding.
const USER_TOKEN_RANDOM_BYTES: usize = 24;
/// Character count shown to users as token fingerprint.
const USER_TOKEN_PREFIX_CHARS: usize = 12;
const DEFAULT_AUTH_CACHE_TTL_SECONDS: u64 = 300;

#[derive(Debug, Clone)]
pub struct AuthCacheConfig {
    pub enabled: bool,
    pub ttl_seconds: u64,
}

impl Default for AuthCacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            ttl_seconds: DEFAULT_AUTH_CACHE_TTL_SECONDS,
        }
    }
}

#[derive(Clone)]
struct CachedUserAuthState {
    user_id: String,
    username: String,
    role: UserRole,
    token_version: u64,
    disabled: bool,
}

impl From<UserRecord> for CachedUserAuthState {
    fn from(value: UserRecord) -> Self {
        Self {
            user_id: value.user_id,
            username: value.username,
            role: value.role,
            token_version: value.token_version,
            disabled: value.disabled,
        }
    }
}

#[derive(Clone)]
struct CachedUserTokenBinding {
    token_id: String,
    user_id: String,
    expires_at: Option<i64>,
}

impl From<UserTokenRecord> for CachedUserTokenBinding {
    fn from(value: UserTokenRecord) -> Self {
        Self {
            token_id: value.token_id,
            user_id: value.user_id,
            expires_at: value.expires_at,
        }
    }
}

struct UserAuthStateService {
    user_store: Arc<dyn UserStore>,
    cache: Option<Cache<String, CachedUserAuthState>>,
}

impl UserAuthStateService {
    fn new(user_store: Arc<dyn UserStore>, cache_config: &AuthCacheConfig) -> Self {
        Self {
            user_store,
            cache: cache_config.enabled.then(|| {
                Cache::builder()
                    .time_to_live(Duration::from_secs(cache_config.ttl_seconds))
                    .build()
            }),
        }
    }

    async fn get_auth_state(&self, user_id: &str) -> TalosResult<Option<CachedUserAuthState>> {
        if let Some(cache) = &self.cache {
            if let Some(cached) = cache.get(user_id) {
                return Ok(Some(cached));
            }
        }

        let Some(user) = self.user_store.get_user_by_id(user_id).await? else {
            return Ok(None);
        };
        let state = CachedUserAuthState::from(user);
        if let Some(cache) = &self.cache {
            cache.insert(user_id.to_string(), state.clone());
        }
        Ok(Some(state))
    }

    fn invalidate(&self, user_id: &str) {
        if let Some(cache) = &self.cache {
            cache.invalidate(user_id);
        }
    }
}

struct UserTokenService {
    token_store: Arc<dyn UserTokenStore>,
    cache: Option<Cache<String, CachedUserTokenBinding>>,
}

impl UserTokenService {
    fn new(token_store: Arc<dyn UserTokenStore>, cache_config: &AuthCacheConfig) -> Self {
        Self {
            token_store,
            cache: cache_config.enabled.then(|| {
                Cache::builder()
                    .time_to_live(Duration::from_secs(cache_config.ttl_seconds))
                    .build()
            }),
        }
    }

    async fn create_token(&self, input: CreateUserTokenRecord) -> TalosResult<UserTokenRecord> {
        let record = self.token_store.create_token(input).await?;
        self.cache_binding(
            &record.token_hash,
            CachedUserTokenBinding::from(record.clone()),
        );
        Ok(record)
    }

    async fn get_token_binding_by_hash(
        &self,
        token_hash: &str,
    ) -> TalosResult<Option<CachedUserTokenBinding>> {
        if let Some(cache) = &self.cache {
            if let Some(cached) = cache.get(token_hash) {
                return Ok(Some(cached));
            }
        }

        let Some(record) = self.token_store.get_token_by_hash(token_hash).await? else {
            return Ok(None);
        };
        let binding = CachedUserTokenBinding::from(record);
        self.cache_binding(token_hash, binding.clone());
        Ok(Some(binding))
    }

    async fn get_token_by_id(&self, token_id: &str) -> TalosResult<Option<UserTokenRecord>> {
        self.token_store.get_token_by_id(token_id).await
    }

    async fn list_tokens_for_user(&self, user_id: &str) -> TalosResult<Vec<UserTokenRecord>> {
        self.token_store.list_tokens_for_user(user_id).await
    }

    async fn revoke_token(&self, user_id: &str, token_id: &str) -> TalosResult<()> {
        let token = self
            .token_store
            .get_token_by_id(token_id)
            .await?
            .ok_or_else(|| TalosError::ResourceNotFound(format!("token {}", token_id)))?;
        self.token_store.delete_token(user_id, token_id).await?;
        self.invalidate_hash(&token.token_hash);
        Ok(())
    }

    async fn touch_token_last_used(&self, token_id: &str, last_used_at: i64) {
        let _ = self
            .token_store
            .touch_token_last_used(token_id, last_used_at)
            .await;
    }

    fn invalidate_hash(&self, token_hash: &str) {
        if let Some(cache) = &self.cache {
            cache.invalidate(token_hash);
        }
    }

    fn cache_binding(&self, token_hash: &str, binding: CachedUserTokenBinding) {
        if let Some(cache) = &self.cache {
            cache.insert(token_hash.to_string(), binding);
        }
    }
}

#[async_trait]
pub trait AuthService: Send + Sync {
    async fn register(&self, input: RegisterRequest) -> TalosResult<AuthResponse>;
    async fn login(&self, input: LoginRequest) -> TalosResult<AuthResponse>;
    async fn logout(&self, actor: &RequestIdentity) -> TalosResult<()>;
    async fn create_token(
        &self,
        actor: &RequestIdentity,
        name: &str,
    ) -> TalosResult<CreateUserTokenResult>;
    async fn list_tokens(&self, actor: &RequestIdentity) -> TalosResult<Vec<UserTokenMeta>>;
    async fn revoke_token(&self, actor: &RequestIdentity, token_id: &str) -> TalosResult<()>;
    async fn verify_access_token(&self, token: &str) -> TalosResult<RequestIdentity>;
}

#[async_trait]
pub trait UserService: Send + Sync {
    async fn get_current_user(&self, actor: &RequestIdentity) -> TalosResult<UserDto>;
    async fn patch_current_user(
        &self,
        actor: &RequestIdentity,
        patch: UpdateCurrentUserRequest,
    ) -> TalosResult<UserDto>;
    async fn list_users(&self, actor: &RequestIdentity) -> TalosResult<Vec<UserDto>>;
    async fn create_user(
        &self,
        actor: &RequestIdentity,
        request: AdminCreateUserRequest,
    ) -> TalosResult<UserDto>;
    async fn patch_user(
        &self,
        actor: &RequestIdentity,
        user_id: &str,
        patch: AdminPatchUserRequest,
    ) -> TalosResult<UserDto>;
    async fn delete_user(&self, actor: &RequestIdentity, user_id: &str) -> TalosResult<()>;
}

pub struct AuthManager {
    user_store: Arc<dyn UserStore>,
    user_auth_state_service: UserAuthStateService,
    user_token_service: UserTokenService,
    jwt_auth: Arc<JwtAuth>,
    password_service: PasswordService,
    allow_register: bool,
}

impl AuthManager {
    pub fn new(
        user_store: Arc<dyn UserStore>,
        token_store: Arc<dyn UserTokenStore>,
        jwt_auth: Arc<JwtAuth>,
        allow_register: bool,
        cache_config: AuthCacheConfig,
    ) -> Self {
        Self {
            user_store: user_store.clone(),
            user_auth_state_service: UserAuthStateService::new(user_store, &cache_config),
            user_token_service: UserTokenService::new(token_store, &cache_config),
            jwt_auth,
            password_service: PasswordService,
            allow_register,
        }
    }

    fn issue_auth_response(&self, user: &UserRecord) -> TalosResult<AuthResponse> {
        let scopes = default_scopes_for_role(user.role);
        let issued = self.jwt_auth.issue_access_token(user, &scopes)?;
        Ok(AuthResponse {
            access_token: issued.token,
            token_type: "Bearer".to_string(),
            expires_at: issued.expires_at,
            user: UserDto::from(user.clone()),
            capabilities: Capabilities::from_role(user.role, &scopes),
        })
    }

    async fn ensure_username_available(
        &self,
        username: &str,
        except_user_id: Option<&str>,
    ) -> TalosResult<()> {
        if let Some(existing) = self.user_store.get_user_by_identifier(username).await? {
            if except_user_id.is_none() || except_user_id != Some(existing.user_id.as_str()) {
                return Err(TalosError::ValidationError(format!(
                    "username already exists: {}",
                    username
                )));
            }
        }
        Ok(())
    }

    async fn ensure_email_available(
        &self,
        email: Option<&str>,
        except_user_id: Option<&str>,
    ) -> TalosResult<()> {
        let Some(email) = email else {
            return Ok(());
        };
        if let Some(existing) = self.user_store.get_user_by_identifier(email).await? {
            if except_user_id.is_none() || except_user_id != Some(existing.user_id.as_str()) {
                return Err(TalosError::ValidationError(format!(
                    "email already exists: {}",
                    email
                )));
            }
        }
        Ok(())
    }

    fn normalize_optional_email(email: Option<String>) -> Option<String> {
        email.and_then(|value| {
            let normalized = value.trim().to_string();
            if normalized.is_empty() {
                None
            } else {
                Some(normalized)
            }
        })
    }

    fn normalize_required_text(field: &str, value: &str) -> TalosResult<String> {
        let normalized = value.trim().to_string();
        if normalized.is_empty() {
            return Err(TalosError::ValidationError(format!(
                "{} is required",
                field
            )));
        }
        Ok(normalized)
    }

    fn normalize_token_name(name: &str) -> TalosResult<String> {
        let normalized = Self::normalize_required_text("name", name)?;
        if normalized.len() > 255 {
            return Err(TalosError::ValidationError(
                "token name must be no longer than 255 characters".to_string(),
            ));
        }
        Ok(normalized)
    }

    fn normalize_required_email(email: Option<String>) -> TalosResult<String> {
        match Self::normalize_optional_email(email) {
            Some(value) => Ok(value),
            None => Err(TalosError::ValidationError("email is required".to_string())),
        }
    }

    fn normalize_patch_email(email: Option<String>) -> TalosResult<Option<String>> {
        match email {
            Some(value) => {
                let normalized = value.trim().to_string();
                if normalized.is_empty() {
                    return Err(TalosError::ValidationError("email is required".to_string()));
                }
                Ok(Some(normalized))
            }
            None => Ok(None),
        }
    }

    fn normalize_patch_username(username: Option<String>) -> TalosResult<Option<String>> {
        match username {
            Some(value) => Ok(Some(Self::normalize_required_text("username", &value)?)),
            None => Ok(None),
        }
    }

    fn hash_token(raw_token: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(raw_token.as_bytes());
        URL_SAFE_NO_PAD.encode(hasher.finalize())
    }

    fn token_prefix(raw_token: &str) -> String {
        raw_token.chars().take(USER_TOKEN_PREFIX_CHARS).collect()
    }

    fn generate_token_value() -> String {
        let mut bytes = [0u8; USER_TOKEN_RANDOM_BYTES];
        rand::thread_rng().fill_bytes(&mut bytes);
        let encoded = URL_SAFE_NO_PAD.encode(bytes);
        format!("{}{}", USER_TOKEN_PREFIX, encoded)
    }

    fn is_token_expired(expires_at: Option<i64>, now_ms: i64) -> bool {
        matches!(expires_at, Some(ts) if ts <= now_ms)
    }

    fn request_identity_from_state(state: &CachedUserAuthState) -> RequestIdentity {
        RequestIdentity {
            user_id: state.user_id.clone(),
            username: Some(state.username.clone()),
            role: state.role,
            scopes: default_scopes_for_role(state.role),
            auth_disabled: false,
            access_token: None,
        }
    }

    async fn get_auth_state_for_token(&self, user_id: &str) -> TalosResult<CachedUserAuthState> {
        self.user_auth_state_service
            .get_auth_state(user_id)
            .await?
            .ok_or_else(|| TalosError::AuthError("user not found".to_string()))
    }

    async fn verify_user_token(&self, token: &str) -> TalosResult<Option<RequestIdentity>> {
        // PATs are identified by prefix and validated before JWT parsing.
        if !token.starts_with(USER_TOKEN_PREFIX) {
            return Ok(None);
        }

        let token_hash = Self::hash_token(token);
        let now_ms = chrono::Utc::now().timestamp_millis();
        let Some(binding) = self
            .user_token_service
            .get_token_binding_by_hash(&token_hash)
            .await?
        else {
            return Ok(None);
        };

        if Self::is_token_expired(binding.expires_at, now_ms) {
            self.user_token_service.invalidate_hash(&token_hash);
            return Err(TalosError::AuthError("token has expired".to_string()));
        }

        let state = self.get_auth_state_for_token(&binding.user_id).await?;
        if state.disabled {
            return Err(TalosError::AuthError(
                "user account is disabled".to_string(),
            ));
        }

        self.user_token_service
            .touch_token_last_used(&binding.token_id, now_ms)
            .await;
        Ok(Some(Self::request_identity_from_state(&state)))
    }

    async fn create_user_internal(
        &self,
        username: &str,
        password: &str,
        email: Option<String>,
        role: UserRole,
        disabled: bool,
    ) -> TalosResult<UserRecord> {
        let normalized_username = Self::normalize_required_text("username", username)?;
        let normalized_email = Self::normalize_required_email(email)?;
        self.ensure_username_available(&normalized_username, None)
            .await?;
        self.ensure_email_available(Some(normalized_email.as_str()), None)
            .await?;
        let password_hash = self.password_service.hash_password(password)?;
        let now = chrono::Utc::now().timestamp_millis();
        self.user_store
            .create_user(CreateUserRecord {
                user_id: uuid::Uuid::new_v4().to_string(),
                username: normalized_username,
                email: Some(normalized_email),
                password_hash,
                role,
                token_version: 1,
                disabled,
                created_at: now,
                updated_at: now,
            })
            .await
    }

    async fn update_password_if_needed(
        &self,
        user_id: &str,
        password: Option<String>,
    ) -> TalosResult<()> {
        if let Some(password) = password {
            let password_hash = self.password_service.hash_password(&password)?;
            self.user_store
                .update_password_hash(user_id, &password_hash)
                .await?;
        }
        Ok(())
    }

    fn require_admin(actor: &RequestIdentity) -> TalosResult<()> {
        if actor.has_full_access() {
            return Ok(());
        }
        Err(TalosError::AuthError("admin access required".to_string()))
    }
}

#[async_trait]
impl AuthService for AuthManager {
    async fn register(&self, input: RegisterRequest) -> TalosResult<AuthResponse> {
        if !self.allow_register {
            return Err(TalosError::AuthError(
                "public registration is disabled".to_string(),
            ));
        }
        let role = if self.user_store.count_users().await? == 0 {
            UserRole::Admin
        } else {
            UserRole::User
        };
        let user = self
            .create_user_internal(&input.username, &input.password, input.email, role, false)
            .await?;
        Ok(self.issue_auth_response(&user)?)
    }

    async fn login(&self, input: LoginRequest) -> TalosResult<AuthResponse> {
        let user = self
            .user_store
            .get_user_by_identifier(input.username.trim())
            .await?
            .ok_or_else(|| TalosError::AuthError("invalid username or password".to_string()))?;
        if user.disabled {
            return Err(TalosError::AuthError(
                "user account is disabled".to_string(),
            ));
        }
        self.password_service
            .verify_password(&input.password, &user.password_hash)?;
        Ok(self.issue_auth_response(&user)?)
    }

    async fn logout(&self, actor: &RequestIdentity) -> TalosResult<()> {
        self.user_store
            .increment_token_version(&actor.user_id)
            .await?;
        self.user_auth_state_service.invalidate(&actor.user_id);
        Ok(())
    }

    async fn create_token(
        &self,
        actor: &RequestIdentity,
        name: &str,
    ) -> TalosResult<CreateUserTokenResult> {
        // Generate raw token only once; persist hash for validation.
        let name = Self::normalize_token_name(name)?;
        let raw_token = Self::generate_token_value();
        let token_hash = Self::hash_token(&raw_token);
        let token_id = uuid::Uuid::new_v4().to_string();
        let prefix = Self::token_prefix(&raw_token);
        let now = chrono::Utc::now().timestamp_millis();

        self.user_token_service
            .create_token(CreateUserTokenRecord {
                token_id: token_id.clone(),
                user_id: actor.user_id.clone(),
                name,
                token_hash,
                prefix: prefix.clone(),
                created_at: now,
                expires_at: None,
            })
            .await?;

        Ok(CreateUserTokenResult {
            token_id,
            raw_token,
            prefix,
        })
    }

    async fn list_tokens(&self, actor: &RequestIdentity) -> TalosResult<Vec<UserTokenMeta>> {
        let mut tokens = self
            .user_token_service
            .list_tokens_for_user(&actor.user_id)
            .await?
            .into_iter()
            .map(UserTokenMeta::from)
            .collect::<Vec<_>>();
        tokens.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(tokens)
    }

    async fn revoke_token(&self, actor: &RequestIdentity, token_id: &str) -> TalosResult<()> {
        let token_record = self
            .user_token_service
            .get_token_by_id(token_id)
            .await?
            .ok_or_else(|| TalosError::ResourceNotFound(format!("token {}", token_id)))?;
        if token_record.user_id != actor.user_id {
            return Err(TalosError::AuthError(
                "token belongs to another user".to_string(),
            ));
        }
        self.user_token_service
            .revoke_token(&actor.user_id, token_id)
            .await
    }

    async fn verify_access_token(&self, token: &str) -> TalosResult<RequestIdentity> {
        // PAT path runs first, then falls back to JWT decode/validation.
        if let Some(identity) = self.verify_user_token(token).await? {
            return Ok(identity);
        }

        let claims = self.jwt_auth.decode_access_token(token)?;
        let state = self
            .user_auth_state_service
            .get_auth_state(&claims.sub)
            .await?
            .ok_or_else(|| TalosError::AuthError("user not found".to_string()))?;
        if state.disabled {
            return Err(TalosError::AuthError(
                "user account is disabled".to_string(),
            ));
        }
        if state.token_version != claims.token_version {
            return Err(TalosError::AuthError("token has been revoked".to_string()));
        }
        let scopes = claims
            .scopes
            .iter()
            .map(|scope| crate::AuthScope::from_str(scope))
            .collect::<TalosResult<Vec<_>>>()?;
        Ok(RequestIdentity {
            user_id: state.user_id,
            username: Some(state.username),
            role: state.role,
            scopes,
            auth_disabled: false,
            access_token: Some(token.to_string()),
        })
    }
}

#[async_trait]
impl UserService for AuthManager {
    async fn get_current_user(&self, actor: &RequestIdentity) -> TalosResult<UserDto> {
        let user = self
            .user_store
            .get_user_by_id(&actor.user_id)
            .await?
            .ok_or_else(|| TalosError::ResourceNotFound(format!("user {}", actor.user_id)))?;
        Ok(UserDto::from(user))
    }

    async fn patch_current_user(
        &self,
        actor: &RequestIdentity,
        patch: UpdateCurrentUserRequest,
    ) -> TalosResult<UserDto> {
        let normalized_username = Self::normalize_patch_username(patch.username)?;
        let normalized_email = Self::normalize_patch_email(patch.email)?;
        if let Some(username) = normalized_username.as_deref() {
            self.ensure_username_available(username, Some(&actor.user_id))
                .await?;
        }
        self.ensure_email_available(normalized_email.as_deref(), Some(&actor.user_id))
            .await?;
        let user = self
            .user_store
            .update_user(
                &actor.user_id,
                UserPatch {
                    username: normalized_username,
                    email: normalized_email,
                    role: None,
                    disabled: None,
                },
            )
            .await?;
        self.user_auth_state_service.invalidate(&actor.user_id);
        Ok(UserDto::from(user))
    }

    async fn list_users(&self, actor: &RequestIdentity) -> TalosResult<Vec<UserDto>> {
        Self::require_admin(actor)?;
        Ok(self
            .user_store
            .list_users()
            .await?
            .into_iter()
            .map(UserDto::from)
            .collect())
    }

    async fn create_user(
        &self,
        actor: &RequestIdentity,
        request: AdminCreateUserRequest,
    ) -> TalosResult<UserDto> {
        Self::require_admin(actor)?;
        let user = self
            .create_user_internal(
                &request.username,
                &request.password,
                request.email,
                request.role,
                request.disabled.unwrap_or(false),
            )
            .await?;
        Ok(UserDto::from(user))
    }

    async fn patch_user(
        &self,
        actor: &RequestIdentity,
        user_id: &str,
        patch: AdminPatchUserRequest,
    ) -> TalosResult<UserDto> {
        Self::require_admin(actor)?;
        let normalized_username = Self::normalize_patch_username(patch.username)?;
        let normalized_email = Self::normalize_patch_email(patch.email)?;
        if let Some(username) = normalized_username.as_deref() {
            self.ensure_username_available(username, Some(user_id))
                .await?;
        }
        self.ensure_email_available(normalized_email.as_deref(), Some(user_id))
            .await?;
        self.user_store
            .update_user(
                user_id,
                UserPatch {
                    username: normalized_username,
                    email: normalized_email,
                    role: patch.role,
                    disabled: patch.disabled,
                },
            )
            .await?;
        self.update_password_if_needed(user_id, patch.password)
            .await?;
        self.user_auth_state_service.invalidate(user_id);
        let latest = self
            .user_store
            .get_user_by_id(user_id)
            .await?
            .ok_or_else(|| TalosError::ResourceNotFound(format!("user {}", user_id)))?;
        Ok(UserDto::from(latest))
    }

    async fn delete_user(&self, actor: &RequestIdentity, user_id: &str) -> TalosResult<()> {
        Self::require_admin(actor)?;
        if actor.user_id == user_id {
            return Err(TalosError::ValidationError(
                "admin cannot delete current user".to_string(),
            ));
        }
        self.user_store.delete_user(user_id).await?;
        self.user_auth_state_service.invalidate(user_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{InMemoryUserStore, InMemoryUserTokenStore};

    fn service(allow_register: bool) -> AuthManager {
        AuthManager::new(
            Arc::new(InMemoryUserStore::default()),
            Arc::new(InMemoryUserTokenStore::default()),
            Arc::new(JwtAuth::new("test-secret")),
            allow_register,
            AuthCacheConfig::default(),
        )
    }

    #[tokio::test]
    async fn test_first_registered_user_becomes_admin() {
        let service = service(true);
        let response = service
            .register(RegisterRequest {
                username: "alice".to_string(),
                password: "password123".to_string(),
                email: Some("alice@example.com".to_string()),
            })
            .await
            .expect("register should succeed");
        assert_eq!(response.user.role, UserRole::Admin);
    }

    #[tokio::test]
    async fn test_logout_invalidates_existing_token() {
        let service = service(true);
        let response = service
            .register(RegisterRequest {
                username: "alice".to_string(),
                password: "password123".to_string(),
                email: Some("alice@example.com".to_string()),
            })
            .await
            .expect("register should succeed");
        let actor = service
            .verify_access_token(&response.access_token)
            .await
            .expect("jwt should verify before logout");
        service.logout(&actor).await.expect("logout should succeed");
        assert!(service
            .verify_access_token(&response.access_token)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn test_create_and_verify_user_token() {
        let service = service(true);
        let response = service
            .register(RegisterRequest {
                username: "alice".to_string(),
                password: "password123".to_string(),
                email: Some("alice@example.com".to_string()),
            })
            .await
            .expect("register should succeed");
        let actor = service
            .verify_access_token(&response.access_token)
            .await
            .expect("jwt should verify");
        let token = service
            .create_token(&actor, "cli")
            .await
            .expect("token should be created");

        let pat_actor = service
            .verify_access_token(&token.raw_token)
            .await
            .expect("pat should verify");
        assert_eq!(pat_actor.user_id, actor.user_id);
        assert_eq!(pat_actor.role, actor.role);
    }

    #[tokio::test]
    async fn test_revoke_user_token_invalidates_pat() {
        let service = service(true);
        let response = service
            .register(RegisterRequest {
                username: "alice".to_string(),
                password: "password123".to_string(),
                email: Some("alice@example.com".to_string()),
            })
            .await
            .expect("register should succeed");
        let actor = service
            .verify_access_token(&response.access_token)
            .await
            .expect("jwt should verify");
        let token = service
            .create_token(&actor, "cli")
            .await
            .expect("token should be created");

        service
            .revoke_token(&actor, &token.token_id)
            .await
            .expect("revoke should succeed");
        assert!(service.verify_access_token(&token.raw_token).await.is_err());
    }

    #[tokio::test]
    async fn test_list_users_allows_auth_disabled_actor() {
        let service = service(true);
        service
            .register(RegisterRequest {
                username: "alice".to_string(),
                password: "password123".to_string(),
                email: Some("alice@example.com".to_string()),
            })
            .await
            .expect("register should succeed");

        let users = service
            .list_users(&RequestIdentity::auth_disabled())
            .await
            .expect("auth disabled actor should list users");

        assert_eq!(users.len(), 1);
        assert_eq!(users[0].username, "alice");
    }
}
