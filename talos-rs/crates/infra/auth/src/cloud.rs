use async_trait::async_trait;
use cloud_sdk_rs::cloud_types::{
    AuthResponseDto, CapabilitiesDto, CurrentUserDto, IntrospectionRequestDto,
    IntrospectionResponseDto, LoginRequestDto, LogoutResponseDto, RegisterRequestDto, UserRoleDto,
    UserStatusDto,
};
use cloud_sdk_rs::{CloudSdkError, TalosCloudClient};
use talos_error::{TalosError, TalosResult};

use crate::{
    AuthResponse, AuthScope, AuthService, Capabilities, LoginRequest, RegisterRequest,
    RequestIdentity, UserDto, UserRole, UserService,
};

#[derive(Debug, Clone)]
pub struct CloudAuthService {
    client: TalosCloudClient,
}

impl CloudAuthService {
    pub fn new(api_base_url: impl Into<String>) -> Self {
        Self {
            client: TalosCloudClient::new(api_base_url),
        }
    }

    fn with_token(&self, token: &str) -> TalosCloudClient {
        self.client.clone().with_access_token(token.to_owned())
    }
}

#[async_trait]
impl AuthService for CloudAuthService {
    async fn register(&self, input: RegisterRequest) -> TalosResult<AuthResponse> {
        let email = input.email.ok_or_else(|| {
            TalosError::ValidationError("email is required for cloud registration".to_string())
        })?;
        let username = input.username;
        let password = input.password;
        self.client
            .register(RegisterRequestDto {
                email,
                username: username.clone(),
                password: password.clone(),
            })
            .await
            .map_err(map_cloud_error)?;
        self.client
            .login(LoginRequestDto {
                identifier: username,
                password,
            })
            .await
            .map(map_auth_response)
            .map_err(map_cloud_error)
    }

    async fn login(&self, input: LoginRequest) -> TalosResult<AuthResponse> {
        self.client
            .login(LoginRequestDto {
                identifier: input.username,
                password: input.password,
            })
            .await
            .map(map_auth_response)
            .map_err(map_cloud_error)
    }

    async fn logout(&self, actor: &RequestIdentity) -> TalosResult<()> {
        let Some(token) = bearer_from_actor(actor) else {
            return Ok(());
        };
        let response = self
            .with_token(&token)
            .logout()
            .await
            .map_err(map_cloud_error)?;
        expect_success(response)
    }

    async fn verify_access_token(&self, token: &str) -> TalosResult<RequestIdentity> {
        let response = self
            .client
            .introspect(IntrospectionRequestDto {
                token: token.to_string(),
            })
            .await
            .map_err(map_cloud_error)?;
        map_identity(response, token)
    }
}

#[async_trait]
impl UserService for CloudAuthService {
    async fn get_current_user(&self, actor: &RequestIdentity) -> TalosResult<UserDto> {
        let token = bearer_from_actor(actor)
            .ok_or_else(|| TalosError::AuthError("cloud access token is missing".to_string()))?;
        self.with_token(&token)
            .me()
            .await
            .map(map_user)
            .map_err(map_cloud_error)
    }
}

fn bearer_from_actor(actor: &RequestIdentity) -> Option<String> {
    actor.access_token.clone()
}

fn map_identity(
    response: IntrospectionResponseDto,
    raw_token: &str,
) -> TalosResult<RequestIdentity> {
    if !response.active {
        return Err(TalosError::AuthError("cloud token is inactive".to_string()));
    }
    let user = response
        .user
        .ok_or_else(|| TalosError::AuthError("cloud introspection missing user".to_string()))?;
    if user.status != UserStatusDto::Active {
        return Err(TalosError::AuthError(
            "cloud user is not active".to_string(),
        ));
    }
    let entitlement = response.entitlement.ok_or_else(|| {
        TalosError::AuthError("cloud introspection missing entitlement".to_string())
    })?;
    if !entitlement.can_use {
        return Err(TalosError::AuthorizationDenied);
    }
    let capabilities = response.capabilities.ok_or_else(|| {
        TalosError::AuthError("cloud introspection missing capabilities".to_string())
    })?;
    Ok(RequestIdentity {
        user_id: user.user_id,
        username: user.username,
        role: map_role(user.role),
        scopes: capabilities
            .scopes
            .iter()
            .map(|scope| AuthScope::from_str(scope))
            .collect::<TalosResult<Vec<_>>>()?,
        auth_disabled: false,
        access_token: Some(raw_token.to_string()),
    })
}

fn map_auth_response(response: AuthResponseDto) -> AuthResponse {
    AuthResponse {
        access_token: response.access_token,
        token_type: response.token_type,
        expires_at: response.expires_at_ms.max(0) as u64,
        user: map_user(response.user),
        capabilities: map_capabilities(response.capabilities),
    }
}

fn map_user(user: CurrentUserDto) -> UserDto {
    UserDto {
        user_id: user.user_id,
        username: user.username,
        email: Some(user.email),
        role: map_role(user.role),
        disabled: user.disabled,
        created_at: user.created_at_ms,
        updated_at: user.updated_at_ms,
    }
}

fn map_capabilities(value: CapabilitiesDto) -> Capabilities {
    Capabilities {
        is_admin: value.is_admin,
        role: map_role(value.role),
        scopes: value
            .scopes
            .into_iter()
            .filter(|scope| AuthScope::from_str(scope).is_ok())
            .collect(),
    }
}

fn map_role(value: UserRoleDto) -> UserRole {
    match value {
        UserRoleDto::User => UserRole::User,
        UserRoleDto::Admin => UserRole::Admin,
    }
}

fn expect_success(response: LogoutResponseDto) -> TalosResult<()> {
    if response.success {
        Ok(())
    } else {
        Err(TalosError::AuthError(
            "cloud auth request did not complete".to_string(),
        ))
    }
}

fn map_cloud_error(error: CloudSdkError) -> TalosError {
    match error {
        CloudSdkError::Transport(err) => TalosError::NetworkError(err.to_string()),
        CloudSdkError::Api { code, message, .. } => {
            TalosError::AuthError(format!("cloud auth request failed: {code}: {message}"))
        }
    }
}
