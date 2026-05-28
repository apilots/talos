use std::sync::Arc;

use async_trait::async_trait;
use talos_app_protocol::requests::{ActorCredential, ActorCredentials};
use talos_application_service::Actor;
use talos_auth::{AuthService, CloudSessionCache, CloudSessionCacheManager, RequestIdentity};
use talos_config::TalosPaths;
use talos_error::TalosError;

use crate::application::ProtocolResult;

#[async_trait]
pub trait ActorCredentialValidator: Send + Sync {
    async fn validate(&self, credentials: &ActorCredentials) -> ProtocolResult<Actor>;
}

#[derive(Debug, Clone)]
pub struct EnvLaunchTokenValidator {
    token: String,
    user_id: String,
}

impl EnvLaunchTokenValidator {
    pub fn from_env() -> ProtocolResult<Self> {
        let token = std::env::var("TALOS_LOCAL_LAUNCH_TOKEN").map_err(|_| {
            talos_app_protocol::errors::AppServerErrorBody::from_talos_error(TalosError::AuthError(
                "TALOS_LOCAL_LAUNCH_TOKEN is not configured".to_string(),
            ))
        })?;
        Ok(Self::new(token, "local-desktop"))
    }

    pub fn new(token: impl Into<String>, user_id: impl Into<String>) -> Self {
        Self {
            token: token.into(),
            user_id: user_id.into(),
        }
    }
}

#[async_trait]
impl ActorCredentialValidator for EnvLaunchTokenValidator {
    async fn validate(&self, credentials: &ActorCredentials) -> ProtocolResult<Actor> {
        match &credentials.credential {
            ActorCredential::LocalLaunchToken { token } if token == &self.token => {
                Ok(Actor::regular(self.user_id.clone()))
            }
            ActorCredential::LocalLaunchToken { .. } => {
                Err(unauthorized("invalid local launch token"))
            }
            ActorCredential::ExistingSessionToken { .. } => Err(unauthorized(
                "existing session token is not supported by this validator",
            )),
        }
    }
}

#[derive(Clone)]
pub struct AuthServiceCredentialValidator {
    auth_service: Arc<dyn AuthService>,
    local_launch_token: Option<String>,
    local_launch_access: LocalLaunchAccess,
    cloud_session_cache: CloudSessionCacheManager,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalLaunchAccess {
    Regular,
    Full,
}

impl AuthServiceCredentialValidator {
    pub fn new(auth_service: Arc<dyn AuthService>, local_launch_token: Option<String>) -> Self {
        Self {
            auth_service,
            local_launch_token,
            local_launch_access: LocalLaunchAccess::Regular,
            cloud_session_cache: CloudSessionCacheManager::for_talos_home(
                TalosPaths::resolve().talos_home,
            ),
        }
    }

    pub fn with_local_launch_access(mut self, access: LocalLaunchAccess) -> Self {
        self.local_launch_access = access;
        self
    }

    pub fn with_cloud_session_cache(mut self, cache: CloudSessionCache) -> Self {
        self.cloud_session_cache = CloudSessionCacheManager::new(cache);
        self
    }

    pub fn with_cloud_session_touch_interval_ms(mut self, interval_ms: i64) -> Self {
        self.cloud_session_cache = self
            .cloud_session_cache
            .clone()
            .with_touch_interval_ms(interval_ms);
        self
    }

    fn local_launch_actor(&self) -> Actor {
        match self.local_launch_access {
            LocalLaunchAccess::Regular => Actor::regular("local-desktop"),
            LocalLaunchAccess::Full => Actor::full("local-desktop"),
        }
    }
}

#[async_trait]
impl ActorCredentialValidator for AuthServiceCredentialValidator {
    async fn validate(&self, credentials: &ActorCredentials) -> ProtocolResult<Actor> {
        match &credentials.credential {
            ActorCredential::LocalLaunchToken { token } => {
                let expected = self
                    .local_launch_token
                    .as_deref()
                    .ok_or_else(|| unauthorized("TALOS_LOCAL_LAUNCH_TOKEN is not configured"))?;
                if token != expected {
                    return Err(unauthorized("invalid local launch token"));
                }
                Ok(self.local_launch_actor())
            }
            ActorCredential::ExistingSessionToken { token } => {
                let identity = match self.auth_service.verify_access_token(token).await {
                    Ok(identity) => identity,
                    Err(error) => {
                        let _ = self
                            .cloud_session_cache
                            .clear_rejected_token(token, CloudSessionCache::now_ms());
                        return Err(
                            talos_app_protocol::errors::AppServerErrorBody::from_talos_error(error),
                        );
                    }
                };
                let _ = self
                    .cloud_session_cache
                    .record_verified_token_use(token, CloudSessionCache::now_ms());
                Ok(actor_from_identity(&identity))
            }
        }
    }
}

fn actor_from_identity(identity: &RequestIdentity) -> Actor {
    let actor = if identity.has_full_access() {
        Actor::full(identity.user_id.clone())
    } else {
        Actor::regular(identity.user_id.clone())
    };
    match identity.access_token.clone() {
        Some(token) => actor.with_access_token(token),
        None => actor,
    }
}

fn unauthorized(message: impl Into<String>) -> talos_app_protocol::errors::AppServerErrorBody {
    talos_app_protocol::errors::AppServerErrorBody::from_talos_error(TalosError::AuthError(
        message.into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use talos_auth::{
        AuthResponse, CloudSessionCacheEntry, CloudSessionUserSnapshot, LoginRequest,
        RegisterRequest, UserRole,
    };
    use talos_error::TalosResult;

    #[derive(Default)]
    struct FakeAuthService;

    #[async_trait]
    impl AuthService for FakeAuthService {
        async fn register(&self, _input: RegisterRequest) -> TalosResult<AuthResponse> {
            Err(TalosError::AuthError("not implemented".to_string()))
        }

        async fn login(&self, _input: LoginRequest) -> TalosResult<AuthResponse> {
            Err(TalosError::AuthError("not implemented".to_string()))
        }

        async fn logout(&self, _actor: &RequestIdentity) -> TalosResult<()> {
            Ok(())
        }

        async fn verify_access_token(&self, token: &str) -> TalosResult<RequestIdentity> {
            if token == "reject" {
                return Err(TalosError::AuthError("invalid token".to_string()));
            }
            Ok(RequestIdentity {
                user_id: "user-1".to_string(),
                username: Some("user-1".to_string()),
                role: UserRole::Admin,
                scopes: Vec::new(),
                auth_disabled: false,
                access_token: Some(token.to_string()),
            })
        }
    }

    #[tokio::test]
    async fn validates_existing_token_and_touches_stale_cache() {
        let now_ms = CloudSessionCache::now_ms();
        let old_last_used = now_ms - 2_000;
        let dir = tempfile::tempdir().expect("tempdir should be created");
        let cache = CloudSessionCache::for_talos_home(dir.path());
        cache
            .store(&test_entry("token", old_last_used))
            .expect("cache should store");
        let validator = AuthServiceCredentialValidator::new(
            Arc::new(FakeAuthService),
            Some("launch".to_string()),
        )
        .with_cloud_session_cache(cache.clone())
        .with_cloud_session_touch_interval_ms(1_000);

        let actor = validator
            .validate(&ActorCredentials {
                credential: ActorCredential::ExistingSessionToken {
                    token: "token".to_string(),
                },
            })
            .await
            .expect("token should validate");

        assert_eq!(actor.access_token.as_deref(), Some("token"));
        assert!(
            cache
                .load_candidate(CloudSessionCache::now_ms())
                .expect("cache should load")
                .expect("cache should exist")
                .last_used_at_ms
                > old_last_used
        );
    }

    #[tokio::test]
    async fn rejected_existing_token_clears_cache() {
        let now_ms = CloudSessionCache::now_ms();
        let dir = tempfile::tempdir().expect("tempdir should be created");
        let cache = CloudSessionCache::for_talos_home(dir.path());
        cache
            .store(&test_entry("reject", now_ms))
            .expect("cache should store");
        let validator = AuthServiceCredentialValidator::new(
            Arc::new(FakeAuthService),
            Some("launch".to_string()),
        )
        .with_cloud_session_cache(cache.clone());

        let result = validator
            .validate(&ActorCredentials {
                credential: ActorCredential::ExistingSessionToken {
                    token: "reject".to_string(),
                },
            })
            .await;

        assert!(result.is_err());
        assert_eq!(
            cache
                .load_candidate(CloudSessionCache::now_ms())
                .expect("cache should load"),
            None
        );
    }

    fn test_entry(token: &str, last_used_at_ms: i64) -> CloudSessionCacheEntry {
        CloudSessionCacheEntry {
            schema_version: 1,
            cloud_api_base_url: "http://127.0.0.1:8080".to_string(),
            access_token: token.to_string(),
            token_type: "Bearer".to_string(),
            expires_at_ms: Some(
                CloudSessionCache::now_ms() + talos_auth::DEFAULT_CLOUD_SESSION_IDLE_TIMEOUT_MS,
            ),
            last_used_at_ms,
            user: CloudSessionUserSnapshot {
                user_id: "user-1".to_string(),
                username: "user-1".to_string(),
                email: Some("user@example.com".to_string()),
                role: UserRole::Admin,
            },
        }
    }
}
