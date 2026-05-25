use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use talos_error::{TalosError, TalosResult};

use crate::{
    AdminCreateUserRequest, AdminPatchUserRequest, AuthResponse, AuthScope, AuthService,
    Capabilities, CreateUserTokenResult, LoginRequest, RegisterRequest, RequestIdentity, UserDto,
    UserRole, UserService, UserTokenMeta,
};

#[derive(Debug, Clone)]
pub struct CloudAuthService {
    api_base_url: String,
    http: reqwest::Client,
}

impl CloudAuthService {
    pub fn new(api_base_url: impl Into<String>) -> Self {
        Self {
            api_base_url: api_base_url.into(),
            http: reqwest::Client::new(),
        }
    }

    async fn get<T>(&self, path: &str, token: Option<&str>) -> TalosResult<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        self.send(self.http.get(self.url(path)), token).await
    }

    async fn post<T, B>(&self, path: &str, body: &B, token: Option<&str>) -> TalosResult<T>
    where
        T: for<'de> Deserialize<'de>,
        B: Serialize + ?Sized,
    {
        self.send(self.http.post(self.url(path)).json(body), token)
            .await
    }

    async fn patch<T, B>(&self, path: &str, body: &B, token: Option<&str>) -> TalosResult<T>
    where
        T: for<'de> Deserialize<'de>,
        B: Serialize + ?Sized,
    {
        self.send(self.http.patch(self.url(path)).json(body), token)
            .await
    }

    async fn delete<T>(&self, path: &str, token: Option<&str>) -> TalosResult<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        self.send(self.http.delete(self.url(path)), token).await
    }

    async fn send<T>(&self, request: reqwest::RequestBuilder, token: Option<&str>) -> TalosResult<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        let request = if let Some(token) = token {
            request.bearer_auth(token)
        } else {
            request
        }
        .header(reqwest::header::ACCEPT, "application/json");
        let response = request
            .send()
            .await
            .map_err(|err| TalosError::NetworkError(err.to_string()))?;
        let status = response.status();
        if status.is_success() {
            return response
                .json::<T>()
                .await
                .map_err(|err| TalosError::SerializationError(err.to_string()));
        }
        let error = response
            .json::<CloudErrorResponse>()
            .await
            .unwrap_or_else(|_| CloudErrorResponse {
                code: "request_failed".to_string(),
                message: status.to_string(),
            });
        Err(TalosError::AuthError(format!(
            "cloud auth request failed: {}: {}",
            error.code, error.message
        )))
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.api_base_url.trim_end_matches('/'), path)
    }
}

#[async_trait]
impl AuthService for CloudAuthService {
    async fn register(&self, input: RegisterRequest) -> TalosResult<AuthResponse> {
        let email = input.email.ok_or_else(|| {
            TalosError::ValidationError("email is required for cloud registration".to_string())
        })?;
        let response: CloudRegisterResponse = self
            .post(
                "/v1/auth/register",
                &CloudRegisterRequest {
                    email,
                    username: input.username,
                    password: input.password,
                },
                None,
            )
            .await?;
        Err(TalosError::AuthError(format!(
            "email verification required for user {}",
            response.user_id
        )))
    }

    async fn login(&self, input: LoginRequest) -> TalosResult<AuthResponse> {
        self.post(
            "/v1/auth/login",
            &CloudLoginRequest {
                identifier: input.username,
                password: input.password,
            },
            None,
        )
        .await
        .map(map_auth_response)
    }

    async fn logout(&self, actor: &RequestIdentity) -> TalosResult<()> {
        let response = self
            .post::<CloudLogoutResponse, _>(
                "/v1/auth/logout",
                &serde_json::json!({}),
                bearer_from_actor(actor).as_deref(),
            )
            .await?;
        expect_success(response)?;
        Ok(())
    }

    async fn create_token(
        &self,
        actor: &RequestIdentity,
        name: &str,
    ) -> TalosResult<CreateUserTokenResult> {
        let response: CloudCreateUserTokenResult = self
            .post(
                "/v1/me/tokens",
                &CloudCreateUserTokenRequest {
                    name: name.to_string(),
                },
                bearer_from_actor(actor).as_deref(),
            )
            .await?;
        Ok(CreateUserTokenResult {
            token_id: response.token_id,
            raw_token: response.raw_token,
            prefix: response.prefix,
        })
    }

    async fn list_tokens(&self, actor: &RequestIdentity) -> TalosResult<Vec<UserTokenMeta>> {
        let response: CloudListResponse<CloudUserTokenMeta> = self
            .get("/v1/me/tokens", bearer_from_actor(actor).as_deref())
            .await?;
        Ok(response.data.into_iter().map(map_token_meta).collect())
    }

    async fn revoke_token(&self, actor: &RequestIdentity, token_id: &str) -> TalosResult<()> {
        let response = self
            .delete::<CloudLogoutResponse>(
                &format!("/v1/me/tokens/{token_id}"),
                bearer_from_actor(actor).as_deref(),
            )
            .await?;
        expect_success(response)?;
        Ok(())
    }

    async fn verify_access_token(&self, token: &str) -> TalosResult<RequestIdentity> {
        let response: CloudIntrospectionResponse = self
            .post(
                "/v1/auth/introspect",
                &CloudIntrospectionRequest {
                    token: token.to_string(),
                },
                None,
            )
            .await?;
        map_identity(response, token)
    }
}

#[async_trait]
impl UserService for CloudAuthService {
    async fn get_current_user(&self, actor: &RequestIdentity) -> TalosResult<UserDto> {
        self.get("/v1/me", bearer_from_actor(actor).as_deref())
            .await
            .map(map_user)
    }

    async fn patch_current_user(
        &self,
        actor: &RequestIdentity,
        patch: crate::UpdateCurrentUserRequest,
    ) -> TalosResult<UserDto> {
        self.patch("/v1/me", &patch, bearer_from_actor(actor).as_deref())
            .await
            .map(map_user)
    }

    async fn list_users(&self, actor: &RequestIdentity) -> TalosResult<Vec<UserDto>> {
        let response: CloudListResponse<CloudUserDto> = self
            .get("/v1/admin/users", bearer_from_actor(actor).as_deref())
            .await?;
        Ok(response.data.into_iter().map(map_user).collect())
    }

    async fn create_user(
        &self,
        actor: &RequestIdentity,
        request: AdminCreateUserRequest,
    ) -> TalosResult<UserDto> {
        let email = request.email.ok_or_else(|| {
            TalosError::ValidationError("email is required for cloud admin user".to_string())
        })?;
        self.post(
            "/v1/admin/users",
            &CloudAdminCreateUserRequest {
                username: request.username,
                password: request.password,
                email,
                role: format_role(request.role).to_string(),
                disabled: request.disabled,
            },
            bearer_from_actor(actor).as_deref(),
        )
        .await
        .map(map_user)
    }

    async fn patch_user(
        &self,
        actor: &RequestIdentity,
        user_id: &str,
        patch: AdminPatchUserRequest,
    ) -> TalosResult<UserDto> {
        self.patch(
            &format!("/v1/admin/users/{user_id}"),
            &CloudAdminPatchUserRequest {
                username: patch.username,
                email: patch.email,
                password: patch.password,
                role: patch.role.map(format_role).map(str::to_string),
                disabled: patch.disabled,
            },
            bearer_from_actor(actor).as_deref(),
        )
        .await
        .map(map_user)
    }

    async fn delete_user(&self, actor: &RequestIdentity, user_id: &str) -> TalosResult<()> {
        let response = self
            .delete::<CloudLogoutResponse>(
                &format!("/v1/admin/users/{user_id}"),
                bearer_from_actor(actor).as_deref(),
            )
            .await?;
        expect_success(response)?;
        Ok(())
    }
}

fn bearer_from_actor(actor: &RequestIdentity) -> Option<String> {
    actor.access_token.clone()
}

fn map_identity(
    response: CloudIntrospectionResponse,
    raw_token: &str,
) -> TalosResult<RequestIdentity> {
    if !response.active {
        return Err(TalosError::AuthError("cloud token is inactive".to_string()));
    }
    let user = response
        .user
        .ok_or_else(|| TalosError::AuthError("cloud introspection missing user".to_string()))?;
    if user.status != "active" {
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
        role: parse_role(&user.role)?,
        scopes: capabilities
            .scopes
            .iter()
            .map(|scope| AuthScope::from_str(scope))
            .collect::<TalosResult<Vec<_>>>()?,
        auth_disabled: false,
        access_token: Some(raw_token.to_string()),
    })
}

fn map_auth_response(response: CloudAuthResponse) -> AuthResponse {
    AuthResponse {
        access_token: response.access_token,
        token_type: response.token_type,
        expires_at: response.expires_at_ms.max(0) as u64,
        user: map_user(response.user),
        capabilities: map_capabilities(response.capabilities),
    }
}

fn map_user(user: CloudUserDto) -> UserDto {
    UserDto {
        user_id: user.user_id,
        username: user.username,
        email: Some(user.email),
        role: parse_role(&user.role).unwrap_or(UserRole::User),
        disabled: user.disabled,
        created_at: user.created_at_ms,
        updated_at: user.updated_at_ms,
    }
}

fn map_capabilities(value: CloudCapabilitiesDto) -> Capabilities {
    Capabilities {
        is_admin: value.is_admin,
        role: parse_role(&value.role).unwrap_or(UserRole::User),
        scopes: value
            .scopes
            .into_iter()
            .filter(|scope| AuthScope::from_str(scope).is_ok())
            .collect(),
    }
}

fn map_token_meta(value: CloudUserTokenMeta) -> UserTokenMeta {
    UserTokenMeta {
        token_id: value.token_id,
        name: value.name,
        prefix: value.prefix,
        created_at: value.created_at_ms,
        last_used_at: value.last_used_at_ms,
        expires_at: value.expires_at_ms,
    }
}

fn parse_role(value: &str) -> TalosResult<UserRole> {
    match value {
        "user" => Ok(UserRole::User),
        "admin" => Ok(UserRole::Admin),
        other => Err(TalosError::AuthError(format!(
            "unknown cloud role: {other}"
        ))),
    }
}

fn format_role(value: UserRole) -> &'static str {
    match value {
        UserRole::User => "user",
        UserRole::Admin => "admin",
    }
}

fn expect_success(response: CloudLogoutResponse) -> TalosResult<()> {
    if response.success {
        Ok(())
    } else {
        Err(TalosError::AuthError(
            "cloud auth request did not complete".to_string(),
        ))
    }
}

#[derive(Debug, Deserialize)]
struct CloudErrorResponse {
    code: String,
    message: String,
}

#[derive(Debug, Serialize)]
struct CloudRegisterRequest {
    email: String,
    username: String,
    password: String,
}

#[derive(Debug, Serialize)]
struct CloudLoginRequest {
    identifier: String,
    password: String,
}

#[derive(Debug, Serialize)]
struct CloudIntrospectionRequest {
    token: String,
}

#[derive(Debug, Serialize)]
struct CloudCreateUserTokenRequest {
    name: String,
}

#[derive(Debug, Serialize)]
struct CloudAdminCreateUserRequest {
    username: String,
    password: String,
    email: String,
    role: String,
    disabled: Option<bool>,
}

#[derive(Debug, Serialize)]
struct CloudAdminPatchUserRequest {
    username: Option<String>,
    email: Option<String>,
    password: Option<String>,
    role: Option<String>,
    disabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct CloudListResponse<T> {
    data: Vec<T>,
}

#[derive(Debug, Deserialize)]
struct CloudAuthResponse {
    access_token: String,
    token_type: String,
    expires_at_ms: i64,
    user: CloudUserDto,
    capabilities: CloudCapabilitiesDto,
}

#[derive(Debug, Deserialize)]
struct CloudRegisterResponse {
    user_id: String,
}

#[derive(Debug, Deserialize)]
struct CloudIntrospectionResponse {
    active: bool,
    user: Option<CloudIntrospectionUser>,
    capabilities: Option<CloudCapabilitiesDto>,
    entitlement: Option<CloudEntitlementDto>,
}

#[derive(Debug, Deserialize)]
struct CloudIntrospectionUser {
    user_id: String,
    username: Option<String>,
    role: String,
    status: String,
}

#[derive(Debug, Deserialize)]
struct CloudEntitlementDto {
    can_use: bool,
}

#[derive(Debug, Deserialize)]
struct CloudCapabilitiesDto {
    is_admin: bool,
    role: String,
    scopes: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct CloudUserDto {
    user_id: String,
    email: String,
    username: String,
    role: String,
    disabled: bool,
    created_at_ms: i64,
    updated_at_ms: i64,
}

#[derive(Debug, Deserialize)]
struct CloudUserTokenMeta {
    token_id: String,
    name: String,
    prefix: String,
    created_at_ms: i64,
    last_used_at_ms: Option<i64>,
    expires_at_ms: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct CloudCreateUserTokenResult {
    token_id: String,
    raw_token: String,
    prefix: String,
}

#[derive(Debug, Deserialize)]
struct CloudLogoutResponse {
    success: bool,
}
