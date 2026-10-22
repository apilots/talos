use async_trait::async_trait;
use talos_error::TalosResult;

use crate::{RequestIdentity, UserDto, UserType};

#[async_trait]
pub trait AuthService: Send + Sync {
    /// Cancel and join Auth-owned maintenance during application shutdown.
    async fn shutdown(&self) {}

    async fn logout(&self, actor: &RequestIdentity) -> TalosResult<()>;
    /// Read an already verified context without network fallback or side effects.
    fn local_identity(&self, _token: &str) -> TalosResult<RequestIdentity> {
        Err(talos_error::TalosError::new(
            talos_error::ErrorCode::Unauthenticated,
            "No verified local authentication context.",
        ))
    }

    /// Invalidate before logout/cache removal; pending verification cannot restore it.
    fn invalidate_access_token(&self, _token: &str) {}

    /// Explicit account admission; reuses an existing context or verifies remotely.
    async fn verify_access_token(&self, token: &str) -> TalosResult<RequestIdentity>;
}

#[async_trait]
pub trait UserService: Send + Sync {
    async fn get_current_user(&self, actor: &RequestIdentity) -> TalosResult<UserDto>;
}

#[derive(Debug, Clone, Default)]
pub struct OpenModeUserService;

#[async_trait]
impl UserService for OpenModeUserService {
    async fn get_current_user(&self, actor: &RequestIdentity) -> TalosResult<UserDto> {
        Ok(UserDto {
            user_id: actor.user_id.clone(),
            username: actor
                .username
                .clone()
                .unwrap_or_else(|| actor.user_id.clone()),
            email: None,
            role: actor.role,
            r#type: UserType::Free,
            disabled: false,
            created_at: 0,
            updated_at: 0,
        })
    }
}
