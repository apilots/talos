use async_trait::async_trait;
use talos_error::TalosResult;

use crate::{RequestIdentity, UserDto};

#[async_trait]
pub trait AuthService: Send + Sync {
    async fn logout(&self, actor: &RequestIdentity) -> TalosResult<()>;
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
            disabled: false,
            created_at: 0,
            updated_at: 0,
        })
    }
}
