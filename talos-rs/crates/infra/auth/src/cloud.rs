use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use talos_error::{TalosError, TalosResult};

use crate::{
    AuthResponse, AuthScope, AuthService, Capabilities, LoginRequest, RegisterRequest,
    RequestIdentity, UserDto, UserRole, UserService,
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

fn parse_role(value: &str) -> TalosResult<UserRole> {
    match value {
        "user" => Ok(UserRole::User),
        "admin" => Ok(UserRole::Admin),
        other => Err(TalosError::AuthError(format!(
            "unknown cloud role: {other}"
        ))),
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
struct CloudLogoutResponse {
    success: bool,
}
