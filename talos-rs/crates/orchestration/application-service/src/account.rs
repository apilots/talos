use std::sync::Arc;

use talos_auth::{
    AdminCreateUserRequest, AdminPatchUserRequest, AuthResponse, AuthService,
    CreateUserTokenResult, LoginRequest, RegisterRequest, RequestIdentity,
    UpdateCurrentUserRequest, UserDto, UserService, UserTokenMeta,
};
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

    pub async fn register(&self, request: RegisterRequest) -> TalosResult<AuthResponse> {
        self.auth_service.register(request).await
    }

    pub async fn login(&self, request: LoginRequest) -> TalosResult<AuthResponse> {
        self.auth_service.login(request).await
    }

    pub async fn logout(&self, actor: &RequestIdentity) -> TalosResult<()> {
        self.auth_service.logout(actor).await
    }

    pub async fn current_user(&self, actor: &RequestIdentity) -> TalosResult<UserDto> {
        self.user_service.get_current_user(actor).await
    }

    pub async fn patch_current_user(
        &self,
        actor: &RequestIdentity,
        request: UpdateCurrentUserRequest,
    ) -> TalosResult<UserDto> {
        self.user_service.patch_current_user(actor, request).await
    }

    pub async fn list_tokens(&self, actor: &RequestIdentity) -> TalosResult<Vec<UserTokenMeta>> {
        self.auth_service.list_tokens(actor).await
    }

    pub async fn create_token(
        &self,
        actor: &RequestIdentity,
        name: &str,
    ) -> TalosResult<CreateUserTokenResult> {
        self.auth_service.create_token(actor, name).await
    }

    pub async fn revoke_token(&self, actor: &RequestIdentity, token_id: &str) -> TalosResult<()> {
        self.auth_service.revoke_token(actor, token_id).await
    }

    pub async fn list_users(&self, actor: &RequestIdentity) -> TalosResult<Vec<UserDto>> {
        self.user_service.list_users(actor).await
    }

    pub async fn create_user(
        &self,
        actor: &RequestIdentity,
        request: AdminCreateUserRequest,
    ) -> TalosResult<UserDto> {
        self.user_service.create_user(actor, request).await
    }

    pub async fn patch_user(
        &self,
        actor: &RequestIdentity,
        user_id: &str,
        request: AdminPatchUserRequest,
    ) -> TalosResult<UserDto> {
        self.user_service.patch_user(actor, user_id, request).await
    }

    pub async fn delete_user(&self, actor: &RequestIdentity, user_id: &str) -> TalosResult<()> {
        self.user_service.delete_user(actor, user_id).await
    }
}
