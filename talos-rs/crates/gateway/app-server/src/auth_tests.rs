use super::*;
use talos_auth::{CloudSessionCacheEntry, UserDto, UserRole, UserService, UserType};
use talos_error::{TalosError, TalosResult};

#[derive(Default)]
struct FakeAuthService {
    remote_calls: std::sync::atomic::AtomicUsize,
}

#[async_trait]
impl AuthService for FakeAuthService {
    async fn logout(&self, _actor: &RequestIdentity) -> TalosResult<()> {
        Ok(())
    }

    async fn verify_access_token(&self, token: &str) -> TalosResult<RequestIdentity> {
        self.remote_calls
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.local_identity(token)
    }

    fn local_identity(&self, token: &str) -> TalosResult<RequestIdentity> {
        if token == "reject" {
            return Err(TalosError::new(
                talos_error::ErrorCode::Unauthenticated,
                "invalid token".to_string(),
            ));
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
            r#type: UserType::Free,
            disabled: false,
            created_at: 0,
            updated_at: 0,
        })
    }
}

#[tokio::test]
async fn verified_local_token_touches_stale_cache_without_remote_admission() {
    let now_ms = CloudSessionCache::now_ms();
    let old_last_used = now_ms - 2_000;
    let dir = tempfile::tempdir().expect("tempdir should be created");
    let cache = CloudSessionCache::for_talos_home(dir.path());
    cache
        .store(&test_entry("token", old_last_used))
        .expect("cache should store");
    let auth_service = Arc::new(FakeAuthService::default());
    let account = AccountService::new(
        auth_service.clone(),
        auth_service.clone(),
        "http://127.0.0.1:8080",
    );
    let validator = AuthServiceCredentialValidator::new(auth_service.clone(), account)
        .with_cloud_session_cache(cache.clone())
        .with_cloud_session_touch_interval_ms(1_000);

    let actor = validator
        .validate_local(&ActorCredentials {
            credential: ActorCredential::ExistingSessionToken {
                token: "token".to_string(),
            },
        })
        .expect("token should validate");
    assert_eq!(
        auth_service
            .remote_calls
            .load(std::sync::atomic::Ordering::SeqCst),
        0
    );

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
    let auth_service = Arc::new(FakeAuthService::default());
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
    let user = UserDto {
        user_id: "user-1".to_string(),
        username: "user-1".to_string(),
        email: Some("user@example.com".to_string()),
        role: UserRole::User,
        r#type: UserType::Free,
        disabled: false,
        created_at: 0,
        updated_at: 0,
    };
    let mut entry = CloudSessionCacheEntry::from_verified_user(
        "http://127.0.0.1:8080",
        token,
        "Bearer",
        Some(CloudSessionCache::now_ms() + talos_auth::DEFAULT_CLOUD_SESSION_IDLE_TIMEOUT_MS),
        &user,
        CloudSessionCache::now_ms(),
    );
    entry.last_used_at_ms = last_used_at_ms;
    entry
}
