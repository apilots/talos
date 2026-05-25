use std::sync::Arc;

use talos_auth::{AuthService, RequestIdentity, UserDto, UserService};
use talos_error::TalosResult;

#[derive(Clone)]
pub struct AccountService {
    auth_service: Arc<dyn AuthService>,
    user_service: Arc<dyn UserService>,
}

impl AccountService {
    pub fn new(auth_service: Arc<dyn AuthService>, user_service: Arc<dyn UserService>) -> Self {
        Self {
            auth_service,
            user_service,
        }
    }

    pub async fn logout(&self, actor: &RequestIdentity) -> TalosResult<()> {
        self.auth_service.logout(actor).await
    }

    pub async fn current_user(&self, actor: &RequestIdentity) -> TalosResult<UserDto> {
        self.user_service.get_current_user(actor).await
    }
}
