use std::sync::Arc;

use async_trait::async_trait;
use talos_error::{TalosError, TalosResult};

use crate::password::PasswordService;
use crate::store::UserStore;
use crate::{
    default_scopes_for_role, AdminCreateUserRequest, AdminPatchUserRequest, AuthResponse,
    Capabilities, CreateUserRecord, JwtAuth, LoginRequest, RegisterRequest, RequestIdentity,
    UpdateCurrentUserRequest, UserDto, UserPatch, UserRecord, UserRole,
};

#[async_trait]
pub trait AuthService: Send + Sync {
    async fn register(&self, input: RegisterRequest) -> TalosResult<AuthResponse>;
    async fn login(&self, input: LoginRequest) -> TalosResult<AuthResponse>;
    async fn logout(&self, actor: &RequestIdentity) -> TalosResult<()>;
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
    jwt_auth: Arc<JwtAuth>,
    password_service: PasswordService,
    allow_register: bool,
}

impl AuthManager {
    pub fn new(
        user_store: Arc<dyn UserStore>,
        jwt_auth: Arc<JwtAuth>,
        allow_register: bool,
    ) -> Self {
        Self {
            user_store,
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
        self.ensure_username_available(&normalized_username, None).await?;
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
        if actor.role.is_admin() {
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
            .create_user_internal(
                &input.username,
                &input.password,
                input.email,
                role,
                false,
            )
            .await?;
        self.issue_auth_response(&user)
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
        self.issue_auth_response(&user)
    }

    async fn logout(&self, actor: &RequestIdentity) -> TalosResult<()> {
        self.user_store
            .increment_token_version(&actor.user_id)
            .await?;
        Ok(())
    }

    async fn verify_access_token(&self, token: &str) -> TalosResult<RequestIdentity> {
        let claims = self.jwt_auth.decode_access_token(token)?;
        let user = self
            .user_store
            .get_user_by_id(&claims.sub)
            .await?
            .ok_or_else(|| TalosError::AuthError("user not found".to_string()))?;
        if user.disabled {
            return Err(TalosError::AuthError(
                "user account is disabled".to_string(),
            ));
        }
        if user.token_version != claims.token_version {
            return Err(TalosError::AuthError("token has been revoked".to_string()));
        }
        let scopes = claims
            .scopes
            .iter()
            .map(|scope| crate::AuthScope::from_str(scope))
            .collect::<TalosResult<Vec<_>>>()?;
        Ok(RequestIdentity {
            user_id: user.user_id.clone(),
            username: Some(user.username.clone()),
            role: user.role,
            scopes,
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
        let _user = self
            .user_store
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
        self.user_store.delete_user(user_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::InMemoryUserStore;

    fn service(allow_register: bool) -> AuthManager {
        AuthManager::new(
            Arc::new(InMemoryUserStore::default()),
            Arc::new(JwtAuth::new("test-secret")),
            allow_register,
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
            .unwrap();
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
            .unwrap();
        let actor = service
            .verify_access_token(&response.access_token)
            .await
            .unwrap();
        service.logout(&actor).await.unwrap();
        assert!(service
            .verify_access_token(&response.access_token)
            .await
            .is_err());
    }
}
