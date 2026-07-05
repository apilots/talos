use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use cloud_sdk_rs::cloud_types::{
    AuthResponseDto, CurrentUserDto, DeviceClientKindDto, DeviceLoginCancelRequestDto,
    DeviceLoginPollRequestDto, DeviceLoginPollResponseDto, DeviceLoginStartRequestDto,
    IntrospectionRequestDto, IntrospectionResponseDto, LogoutResponseDto, UserRoleDto,
    UserStatusDto, UserTypeDto,
};
use cloud_sdk_rs::{CloudSdkError, TalosCloudClient};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use talos_error::{TalosError, TalosResult};
use ts_rs::TS;

use crate::{
    AuthResponse, AuthService, CloudSessionCache, CloudSessionCacheEntry, CloudSessionCacheManager,
    RequestIdentity, UserDto, UserRole, UserService, UserType,
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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct CloudLoginStartRequest {
    pub client_name: String,
    pub client_kind: CloudLoginClientKind,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CloudLoginClientKind {
    Web,
    Desktop,
    Tui,
    Cli,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloudLoginPersistence {
    None,
    LocalCache,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct CloudLoginStartResponse {
    pub login_id: String,
    pub login_url: String,
    pub user_code: String,
    #[ts(type = "number")]
    pub expires_at_ms: i64,
    #[ts(type = "number")]
    pub interval_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct CloudLoginStatusRequest {
    pub login_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct CloudLoginCancelRequest {
    pub login_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum CloudLoginStatusResponse {
    Pending {
        #[ts(type = "number")]
        interval_ms: u64,
    },
    Authenticated {
        user: UserDto,
        access_token: String,
        token_type: String,
    },
    Denied,
    Expired,
}

#[derive(Debug, Clone)]
struct PendingCloudLogin {
    device_code: String,
    persistence: CloudLoginPersistence,
    expires_at_ms: i64,
}

enum PendingLookup {
    Active(PendingCloudLogin),
    Expired,
    Missing,
}

pub struct CloudLoginService {
    client: TalosCloudClient,
    cloud_api_base_url: String,
    session_cache: CloudSessionCacheManager,
    pending: Mutex<HashMap<String, PendingCloudLogin>>,
}

impl CloudLoginService {
    pub fn new(api_base_url: impl Into<String>, session_cache: CloudSessionCacheManager) -> Self {
        let api_base_url = api_base_url.into();
        Self {
            client: TalosCloudClient::new(api_base_url.clone()),
            cloud_api_base_url: api_base_url,
            session_cache,
            pending: Mutex::new(HashMap::new()),
        }
    }

    pub async fn start(
        &self,
        request: CloudLoginStartRequest,
        persistence: CloudLoginPersistence,
    ) -> TalosResult<CloudLoginStartResponse> {
        self.cleanup_expired_pending(CloudSessionCache::now_ms())?;
        let response = self
            .client
            .device_start(DeviceLoginStartRequestDto {
                client_name: request.client_name,
                client_kind: map_client_kind(request.client_kind),
                scopes: vec!["talos:runtime".to_string()],
                code_challenge: None,
            })
            .await
            .map_err(map_cloud_error)?;
        self.pending
            .lock()
            .map_err(|_| TalosError::InternalError("cloud login pending store poisoned".into()))?
            .insert(
                response.session_id.clone(),
                PendingCloudLogin {
                    device_code: response.device_code,
                    persistence,
                    expires_at_ms: response.expires_at_ms,
                },
            );
        Ok(CloudLoginStartResponse {
            login_id: response.session_id,
            login_url: response.verification_uri_complete,
            user_code: response.user_code,
            expires_at_ms: response.expires_at_ms,
            interval_ms: response.interval_ms,
        })
    }

    pub async fn status(
        &self,
        request: CloudLoginStatusRequest,
    ) -> TalosResult<CloudLoginStatusResponse> {
        let pending = match self.lookup_pending(&request.login_id)? {
            PendingLookup::Active(pending) => pending,
            PendingLookup::Expired => return Ok(CloudLoginStatusResponse::Expired),
            PendingLookup::Missing => {
                return Err(TalosError::AuthError(
                    "cloud login session not found".into(),
                ));
            }
        };
        let response = self
            .client
            .device_poll(DeviceLoginPollRequestDto {
                session_id: request.login_id.clone(),
                device_code: pending.device_code,
            })
            .await
            .map_err(map_cloud_error)?;
        match response {
            DeviceLoginPollResponseDto::Pending { interval_ms } => {
                Ok(CloudLoginStatusResponse::Pending { interval_ms })
            }
            DeviceLoginPollResponseDto::Approved { auth } => {
                self.pending
                    .lock()
                    .map_err(|_| {
                        TalosError::InternalError("cloud login pending store poisoned".into())
                    })?
                    .remove(&request.login_id);
                let auth_response = map_auth_response(auth);
                let user = auth_response.user.clone();
                if should_store_cloud_session(pending.persistence) {
                    let entry = CloudSessionCacheEntry::from_auth_response(
                        self.cloud_api_base_url.clone(),
                        &auth_response,
                        CloudSessionCache::now_ms(),
                    );
                    self.session_cache
                        .store_verified_session(&entry)
                        .map_err(|err| {
                            TalosError::StorageError(format!("store cloud login session: {err}"))
                        })?;
                }
                Ok(CloudLoginStatusResponse::Authenticated {
                    user,
                    access_token: auth_response.access_token,
                    token_type: auth_response.token_type,
                })
            }
            DeviceLoginPollResponseDto::Denied => {
                self.remove_pending(&request.login_id)?;
                Ok(CloudLoginStatusResponse::Denied)
            }
            DeviceLoginPollResponseDto::Expired => {
                self.remove_pending(&request.login_id)?;
                Ok(CloudLoginStatusResponse::Expired)
            }
        }
    }

    pub async fn cancel(&self, request: CloudLoginCancelRequest) -> TalosResult<()> {
        self.cleanup_expired_pending(CloudSessionCache::now_ms())?;
        let pending = self.remove_pending(&request.login_id)?;
        self.client
            .device_cancel(DeviceLoginCancelRequestDto {
                session_id: request.login_id,
                device_code: pending.device_code,
            })
            .await
            .map_err(map_cloud_error)?;
        Ok(())
    }

    fn remove_pending(&self, login_id: &str) -> TalosResult<PendingCloudLogin> {
        self.pending
            .lock()
            .map_err(|_| TalosError::InternalError("cloud login pending store poisoned".into()))?
            .remove(login_id)
            .ok_or_else(|| TalosError::AuthError("cloud login session not found".into()))
    }

    fn lookup_pending(&self, login_id: &str) -> TalosResult<PendingLookup> {
        let now_ms = CloudSessionCache::now_ms();
        let mut pending = self
            .pending
            .lock()
            .map_err(|_| TalosError::InternalError("cloud login pending store poisoned".into()))?;
        if let Some(login) = pending.get(login_id).cloned() {
            if login.expires_at_ms <= now_ms {
                pending.remove(login_id);
                cleanup_expired_pending_locked(&mut pending, now_ms);
                return Ok(PendingLookup::Expired);
            }
            cleanup_expired_pending_locked(&mut pending, now_ms);
            return Ok(PendingLookup::Active(login));
        }
        cleanup_expired_pending_locked(&mut pending, now_ms);
        Ok(PendingLookup::Missing)
    }

    fn cleanup_expired_pending(&self, now_ms: i64) -> TalosResult<()> {
        let mut pending = self
            .pending
            .lock()
            .map_err(|_| TalosError::InternalError("cloud login pending store poisoned".into()))?;
        cleanup_expired_pending_locked(&mut pending, now_ms);
        Ok(())
    }
}

fn cleanup_expired_pending_locked(pending: &mut HashMap<String, PendingCloudLogin>, now_ms: i64) {
    pending.retain(|_, login| login.expires_at_ms > now_ms);
}

fn should_store_cloud_session(persistence: CloudLoginPersistence) -> bool {
    matches!(persistence, CloudLoginPersistence::LocalCache)
}

#[async_trait]
impl AuthService for CloudAuthService {
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

fn map_client_kind(value: CloudLoginClientKind) -> DeviceClientKindDto {
    match value {
        CloudLoginClientKind::Web => DeviceClientKindDto::Web,
        CloudLoginClientKind::Desktop => DeviceClientKindDto::Desktop,
        CloudLoginClientKind::Tui => DeviceClientKindDto::Tui,
        CloudLoginClientKind::Cli => DeviceClientKindDto::Cli,
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
    Ok(RequestIdentity {
        user_id: user.user_id,
        username: user.username,
        role: map_role(user.role),
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
    }
}

fn map_user(user: CurrentUserDto) -> UserDto {
    UserDto {
        user_id: user.user_id,
        username: user.username,
        email: Some(user.email),
        role: map_role(user.role),
        r#type: map_user_type(user.user_type),
        disabled: user.disabled,
        created_at: user.created_at_ms,
        updated_at: user.updated_at_ms,
    }
}

fn map_role(value: UserRoleDto) -> UserRole {
    match value {
        UserRoleDto::User => UserRole::User,
        UserRoleDto::Admin => UserRole::Admin,
    }
}

fn map_user_type(value: UserTypeDto) -> UserType {
    match value {
        UserTypeDto::Free => UserType::Free,
        UserTypeDto::Plus => UserType::Plus,
        UserTypeDto::Pro => UserType::Pro,
        UserTypeDto::Max => UserType::Max,
    }
}

#[cfg(test)]
#[path = "cloud_tests.rs"]
mod tests;

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
