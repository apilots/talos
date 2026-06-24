use std::sync::Arc;

use async_trait::async_trait;
use talos_app_protocol::requests::{ActorCredential, ActorCredentials};
use talos_auth::{AuthService, CloudSessionCache, RequestIdentity};
use talos_service::{AccountService, Actor};

use crate::application::ProtocolResult;

#[async_trait]
pub trait ActorCredentialValidator: Send + Sync {
    async fn validate(&self, credentials: &ActorCredentials) -> ProtocolResult<Actor>;
}

#[derive(Clone)]
pub struct AuthServiceCredentialValidator {
    auth_service: Arc<dyn AuthService>,
    account: AccountService,
}

impl AuthServiceCredentialValidator {
    pub fn new(auth_service: Arc<dyn AuthService>, account: AccountService) -> Self {
        Self {
            auth_service,
            account,
        }
    }

    pub fn with_cloud_session_cache(self, cache: CloudSessionCache) -> Self {
        self.account
            .set_cloud_session_cache(cache)
            .expect("cloud session cache should configure");
        self
    }

    pub fn with_cloud_session_touch_interval_ms(self, interval_ms: i64) -> Self {
        self.account
            .set_cloud_session_touch_interval_ms(interval_ms)
            .expect("cloud session touch interval should configure");
        self
    }
}

#[async_trait]
impl ActorCredentialValidator for AuthServiceCredentialValidator {
    async fn validate(&self, credentials: &ActorCredentials) -> ProtocolResult<Actor> {
        match &credentials.credential {
            ActorCredential::ExistingSessionToken { token } => {
                let identity = match self.auth_service.verify_access_token(token).await {
                    Ok(identity) => identity,
                    Err(error) => {
                        let _ = self
                            .account
                            .clear_rejected_access_token(token, CloudSessionCache::now_ms());
                        return Err(
                            talos_app_protocol::errors::AppServerErrorBody::from_talos_error(error),
                        );
                    }
                };
                let _ = self
                    .account
                    .record_verified_access_token_use(token, CloudSessionCache::now_ms());
                Ok(actor_from_identity(&identity))
            }
        }
    }
}

fn actor_from_identity(identity: &RequestIdentity) -> Actor {
    let actor = Actor::regular(identity.user_id.clone())
        .with_username(identity.username.clone())
        .with_role(identity.role);
    match identity.access_token.clone() {
        Some(token) => actor.with_access_token(token),
        None => actor,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use talos_auth::{
        CloudSessionCacheEntry, CloudSessionUserSnapshot, UserDto, UserRole, UserService,
    };
    use talos_error::{TalosError, TalosResult};

    #[derive(Default)]
    struct FakeAuthService;

    #[async_trait]
    impl AuthService for FakeAuthService {
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
                role: UserRole::User,
                auth_disabled: false,
                access_token: Some(token.to_string()),
            })
        }
    }

    #[async_trait]
    impl UserService for FakeAuthService {
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

    #[tokio::test]
    async fn validates_existing_token_and_touches_stale_cache() {
        let now_ms = CloudSessionCache::now_ms();
        let old_last_used = now_ms - 2_000;
        let dir = tempfile::tempdir().expect("tempdir should be created");
        let cache = CloudSessionCache::for_talos_home(dir.path());
        cache
            .store(&test_entry("token", old_last_used))
            .expect("cache should store");
        let auth_service = Arc::new(FakeAuthService);
        let account = AccountService::new(
            auth_service.clone(),
            auth_service.clone(),
            "http://127.0.0.1:8080",
        );
        let validator = AuthServiceCredentialValidator::new(auth_service, account)
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
        let auth_service = Arc::new(FakeAuthService);
        let account = AccountService::new(
            auth_service.clone(),
            auth_service.clone(),
            "http://127.0.0.1:8080",
        );
        let validator = AuthServiceCredentialValidator::new(auth_service, account)
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
                role: UserRole::User,
            },
        }
    }
}
