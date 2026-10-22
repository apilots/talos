use super::*;

use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;

use axum::{extract::State, response::IntoResponse, routing::post, Json, Router};

const LOGIN_ID: &str = "login-a";
const ACCESS_TOKEN: &str = "device-token-a";
const POLL_INTERVAL_MS: u64 = 1700;

#[derive(Clone, Default)]
struct LoginIssuer {
    polls: Arc<AtomicUsize>,
    fail_verification: Arc<AtomicBool>,
    hold_verification: Arc<AtomicBool>,
    verification_entered: Arc<tokio::sync::Notify>,
    verification_resume: Arc<tokio::sync::Notify>,
}

struct LoginFixture {
    issuer: LoginIssuer,
    login: Arc<CloudLoginService>,
    auth: Arc<CloudAuthService>,
    cache: CloudSessionCacheManager,
    server: tokio::task::JoinHandle<()>,
    _home: tempfile::TempDir,
}

impl LoginFixture {
    async fn new() -> Self {
        let issuer = LoginIssuer::default();
        let app = Router::new()
            .route(
                "/v1/auth/device/start",
                post(|| async {
                    Json(serde_json::json!({
                        "session_id": LOGIN_ID,
                        "device_code": "device-a",
                        "user_code": "ABCD",
                        "verification_uri": "https://example.invalid/device",
                        "verification_uri_complete": "https://example.invalid/device/ABCD",
                        "expires_at_ms": CloudSessionCache::now_ms() + 60_000,
                        "interval_ms": POLL_INTERVAL_MS
                    }))
                }),
            )
            .route(
                "/v1/auth/device/poll",
                post(|State(issuer): State<LoginIssuer>| async move {
                    // The real device grant is one-shot: polling again must not
                    // rescue a client that discarded its Approved response.
                    if issuer.polls.fetch_add(1, Ordering::SeqCst) != 0 {
                        return Json(serde_json::json!({"status": "expired"}));
                    }
                    Json(serde_json::json!({"status": "approved", "auth": {
                        "access_token": ACCESS_TOKEN,
                        "token_type": "Bearer",
                        "expires_at_ms": CloudSessionCache::now_ms() + 60_000,
                        "user": {
                            "user_id": "account-a", "username": "test",
                            "email": "test@example.invalid", "role": "user",
                            "type": "free", "status": "active", "email_verified": true,
                            "set_password_required": false, "disabled": false,
                            "created_at_ms": 0, "updated_at_ms": 0
                        },
                        "capabilities": {"is_admin": false, "role": "user", "scopes": []}
                    }}))
                }),
            )
            .route(
                "/v1/auth/device/cancel",
                post(|| async { Json(serde_json::json!({"status": "denied"})) }),
            )
            .route(
                "/v1/auth/introspect",
                post(|State(issuer): State<LoginIssuer>| async move {
                    let resumed = issuer.verification_resume.notified();
                    if issuer.hold_verification.load(Ordering::SeqCst) {
                        issuer.verification_entered.notify_one();
                        resumed.await;
                    }
                    if issuer.fail_verification.load(Ordering::SeqCst) {
                        return axum::http::StatusCode::SERVICE_UNAVAILABLE.into_response();
                    }
                    Json(serde_json::json!({
                        "active": true,
                        "user": {
                            "user_id": "account-a", "username": "test",
                            "email": "test@example.invalid", "role": "user",
                            "type": "free", "status": "active"
                        },
                        "capabilities": {"is_admin": false, "role": "user", "scopes": []},
                        "entitlement": {"can_use": true, "plan": "free", "reason": null},
                        "expires_at_ms": null
                    }))
                    .into_response()
                }),
            )
            .with_state(issuer.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let home = tempfile::tempdir().unwrap();
        let cache = CloudSessionCacheManager::for_talos_home(home.path());
        Self {
            issuer,
            login: Arc::new(CloudLoginService::new(&base_url, cache.clone())),
            auth: Arc::new(CloudAuthService::new(&base_url)),
            cache,
            server,
            _home: home,
        }
    }

    async fn start(&self, persistence: CloudLoginPersistence) {
        let started = self
            .login
            .start(
                CloudLoginStartRequest {
                    client_name: "login-regression".to_owned(),
                    client_kind: CloudLoginClientKind::Desktop,
                },
                persistence,
            )
            .await
            .unwrap();
        assert_eq!(started.login_id, LOGIN_ID);
        assert_eq!(started.interval_ms, POLL_INTERVAL_MS);
    }

    fn poll(&self) -> tokio::task::JoinHandle<TalosResult<CloudLoginStatusResponse>> {
        let login = self.login.clone();
        let auth = self.auth.clone();
        tokio::spawn(async move {
            tokio::time::timeout(
                Duration::from_secs(2),
                login.status(
                    CloudLoginStatusRequest {
                        login_id: LOGIN_ID.to_owned(),
                    },
                    auth.as_ref(),
                ),
            )
            .await
            .expect("login polling must not stall behind admission state")
        })
    }

    async fn wait_for_verification(&self) {
        tokio::time::timeout(
            Duration::from_secs(2),
            self.issuer.verification_entered.notified(),
        )
        .await
        .unwrap();
    }

    fn release_verification(&self) {
        self.issuer.hold_verification.store(false, Ordering::SeqCst);
        self.issuer.verification_resume.notify_waiters();
    }
}

impl Drop for LoginFixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}

#[tokio::test]
async fn approved_login_survives_admission_outage_and_concurrent_poll_without_reconsuming_grant() {
    for persistence in [
        CloudLoginPersistence::None,
        CloudLoginPersistence::LocalCache,
    ] {
        let fixture = LoginFixture::new().await;
        fixture.start(persistence).await;
        fixture
            .issuer
            .fail_verification
            .store(true, Ordering::SeqCst);
        fixture
            .issuer
            .hold_verification
            .store(true, Ordering::SeqCst);
        let first = fixture.poll();
        fixture.wait_for_verification().await;
        let concurrent = fixture.poll();
        assert!(!fixture.cache.cache().path().exists());
        assert!(fixture.auth.local_identity(ACCESS_TOKEN).is_err());
        fixture.release_verification();
        for poll in [first, concurrent] {
            assert_eq!(
                poll.await.unwrap().unwrap(),
                CloudLoginStatusResponse::Pending {
                    interval_ms: POLL_INTERVAL_MS
                }
            );
        }
        assert_eq!(fixture.issuer.polls.load(Ordering::SeqCst), 1);
        assert!(!fixture.cache.cache().path().exists());
        assert!(fixture.auth.local_identity(ACCESS_TOKEN).is_err());

        fixture
            .issuer
            .fail_verification
            .store(false, Ordering::SeqCst);
        let authenticated = fixture.poll().await.unwrap().unwrap();
        let CloudLoginStatusResponse::Authenticated {
            user,
            access_token,
            token_type,
        } = authenticated
        else {
            panic!("the same approved login must recover after admission becomes available");
        };
        assert_eq!(user.user_id, "account-a");
        assert_eq!(access_token, ACCESS_TOKEN);
        assert_eq!(token_type, "Bearer");
        assert_eq!(
            fixture.auth.local_identity(ACCESS_TOKEN).unwrap().user_id,
            user.user_id
        );
        let cached = fixture
            .cache
            .read_candidate(CloudSessionCache::now_ms())
            .unwrap();
        match persistence {
            CloudLoginPersistence::None => assert!(cached.is_none()),
            CloudLoginPersistence::LocalCache => {
                assert_eq!(cached.unwrap().access_token, ACCESS_TOKEN);
            }
        }
        assert_eq!(fixture.issuer.polls.load(Ordering::SeqCst), 1);
        fixture.auth.shutdown().await;
    }
}

#[tokio::test]
async fn cancel_during_login_admission_prevents_late_authentication_and_cache_write() {
    let fixture = LoginFixture::new().await;
    fixture.start(CloudLoginPersistence::LocalCache).await;
    fixture
        .issuer
        .hold_verification
        .store(true, Ordering::SeqCst);
    let pending = fixture.poll();
    fixture.wait_for_verification().await;
    fixture
        .login
        .cancel(CloudLoginCancelRequest {
            login_id: LOGIN_ID.to_owned(),
        })
        .await
        .unwrap();
    fixture.release_verification();
    assert!(!matches!(
        pending.await.unwrap(),
        Ok(CloudLoginStatusResponse::Authenticated { .. })
    ));
    assert!(!fixture.cache.cache().path().exists());
    assert!(fixture.auth.local_identity(ACCESS_TOKEN).is_err());
    assert_eq!(
        fixture.poll().await.unwrap().unwrap_err().error_code(),
        talos_error::ErrorCode::Unauthenticated
    );
    assert_eq!(fixture.issuer.polls.load(Ordering::SeqCst), 1);
    fixture.auth.shutdown().await;
}

#[tokio::test]
async fn cache_commit_failure_invalidates_admission_and_preserves_approved_login_for_retry() {
    let fixture = LoginFixture::new().await;
    fixture.start(CloudLoginPersistence::LocalCache).await;
    let cache_path = fixture.cache.cache().path();
    // Admission succeeds, but replacing a directory with the cache file fails.
    std::fs::create_dir_all(cache_path).unwrap();
    assert_eq!(
        fixture.poll().await.unwrap().unwrap_err().error_code(),
        talos_error::ErrorCode::InternalError
    );
    assert!(cache_path.is_dir());
    assert!(fixture.auth.local_identity(ACCESS_TOKEN).is_err());
    assert_eq!(fixture.issuer.polls.load(Ordering::SeqCst), 1);

    std::fs::remove_dir(cache_path).unwrap();
    let authenticated = fixture.poll().await.unwrap().unwrap();
    let CloudLoginStatusResponse::Authenticated { access_token, .. } = authenticated else {
        panic!("the approved login must remain retryable after its cache commit fails");
    };
    assert_eq!(access_token, ACCESS_TOKEN);
    assert!(cache_path.is_file());
    assert_eq!(
        fixture
            .cache
            .read_candidate(CloudSessionCache::now_ms())
            .unwrap()
            .unwrap()
            .access_token,
        ACCESS_TOKEN
    );
    assert_eq!(
        fixture.auth.local_identity(ACCESS_TOKEN).unwrap().user_id,
        "account-a"
    );
    assert_eq!(fixture.issuer.polls.load(Ordering::SeqCst), 1);
    fixture.auth.shutdown().await;
}
