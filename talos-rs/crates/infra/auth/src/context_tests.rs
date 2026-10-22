use super::*;

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[derive(Clone, Default)]
struct Issuer {
    calls: Arc<AtomicUsize>,
    blocked: Arc<AtomicBool>,
    revoked: Arc<AtomicBool>,
    entered: Arc<tokio::sync::Notify>,
    resume: Arc<tokio::sync::Notify>,
}

async fn issuer() -> (Arc<AuthContexts>, TalosCloudClient, Issuer, JoinHandle<()>) {
    use axum::{extract::State, routing::post, Json, Router};
    let issuer = Issuer::default();
    let app = Router::new().route("/v1/auth/introspect", post(|State(issuer): State<Issuer>| async move {
        issuer.calls.fetch_add(1, Ordering::SeqCst);
        let resumed = issuer.resume.notified();
        if issuer.blocked.load(Ordering::SeqCst) {
            issuer.entered.notify_one();
            resumed.await;
        }
        Json(serde_json::json!({"active": !issuer.revoked.load(Ordering::SeqCst),
            "user":{"user_id":"account-a","username":"test","email":"test@example.invalid","role":"user","type":"free","status":"active"},
            "capabilities":{"is_admin":false,"role":"user","scopes":[]},
            "entitlement":{"can_use":true,"plan":"free","reason":null},"expires_at_ms":null}))
    })).with_state(issuer.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = TalosCloudClient::new(format!("http://{}", listener.local_addr().unwrap()));
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (Arc::new(AuthContexts::default()), client, issuer, task)
}

#[tokio::test]
async fn local_context_is_network_free_during_shared_refresh_and_rejects_revocation() {
    let (service, client, issuer, server) = issuer().await;
    assert_eq!(
        service.identity("unknown").unwrap_err().error_code(),
        ErrorCode::Unauthenticated
    );
    let (first, concurrent) = tokio::join!(
        service.verify(&client, "token"),
        service.verify(&client, "token")
    );
    assert_eq!(first.unwrap(), concurrent.unwrap());
    assert_eq!(issuer.calls.load(Ordering::SeqCst), 1);
    issuer.blocked.store(true, Ordering::SeqCst);
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(70)).await;
    // Resume the clock before waiting for actual network I/O.
    tokio::time::resume();
    tokio::time::timeout(Duration::from_secs(1), issuer.entered.notified())
        .await
        .unwrap();
    for _ in 0..100 {
        assert_eq!(service.identity("token").unwrap().user_id, "account-a");
    }
    assert_eq!(issuer.calls.load(Ordering::SeqCst), 2);
    issuer.revoked.store(true, Ordering::SeqCst);
    issuer.blocked.store(false, Ordering::SeqCst);
    let context = service
        .sessions
        .lock()
        .unwrap()
        .get(&fingerprint("token"))
        .unwrap()
        .clone();
    let mut changes = context.state.subscribe();
    issuer.resume.notify_waiters();
    tokio::time::timeout(Duration::from_secs(1), changes.changed())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        service.identity("token").unwrap_err().error_code(),
        ErrorCode::Unauthenticated
    );
    server.abort();
}

#[tokio::test]
async fn logout_invalidates_inflight_admission_and_cancellation_allows_retry() {
    let (service, client, issuer, server) = issuer().await;
    issuer.blocked.store(true, Ordering::SeqCst);
    let admission = {
        let service = service.clone();
        let client = client.clone();
        tokio::spawn(async move { service.verify(&client, "token").await })
    };
    tokio::time::timeout(Duration::from_secs(1), issuer.entered.notified())
        .await
        .unwrap();
    service.invalidate("token");
    issuer.blocked.store(false, Ordering::SeqCst);
    issuer.resume.notify_waiters();
    assert_eq!(
        admission.await.unwrap().unwrap_err().error_code(),
        ErrorCode::Unauthenticated
    );
    assert!(service.identity("token").is_err());
    issuer.blocked.store(true, Ordering::SeqCst);
    let cancelled = {
        let service = service.clone();
        let client = client.clone();
        tokio::spawn(async move { service.verify(&client, "token").await })
    };
    tokio::time::timeout(Duration::from_secs(1), issuer.entered.notified())
        .await
        .unwrap();
    cancelled.abort();
    let _ = cancelled.await;
    issuer.blocked.store(false, Ordering::SeqCst);
    issuer.resume.notify_waiters();
    assert_eq!(
        service.verify(&client, "token").await.unwrap().user_id,
        "account-a"
    );
    server.abort();
}

#[tokio::test]
async fn hard_deadline_and_token_expiry_fail_locally_without_network_fallback() {
    let (service, client, issuer, server) = issuer().await;
    service.verify(&client, "token").await.unwrap();
    let context = service
        .sessions
        .lock()
        .unwrap()
        .get(&fingerprint("token"))
        .unwrap()
        .clone();
    let mut snapshot = context.state.borrow().clone().unwrap();
    snapshot.verified_at = Instant::now() - VALIDITY;
    let _ = context.state.send_replace(Ok(snapshot.clone()));
    assert_eq!(
        service.identity("token").unwrap_err().error_code(),
        ErrorCode::ServiceUnavailable
    );
    snapshot.verified_at = Instant::now();
    snapshot.expires_at_ms = Some(CloudSessionCache::now_ms() - 1);
    let _ = context.state.send_replace(Ok(snapshot));
    assert_eq!(
        service.identity("token").unwrap_err().error_code(),
        ErrorCode::Unauthenticated
    );
    assert_eq!(issuer.calls.load(Ordering::SeqCst), 1);
    service.invalidate("token");
    assert!(context.task.lock().unwrap().is_none());
    server.abort();
}

#[tokio::test]
async fn shutdown_joins_held_refresh_and_prevents_restart() {
    let (service, client, issuer, server) = issuer().await;
    service.verify(&client, "token").await.unwrap();
    issuer.blocked.store(true, Ordering::SeqCst);
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(70)).await;
    tokio::time::resume();
    tokio::time::timeout(Duration::from_secs(1), issuer.entered.notified())
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(1), service.shutdown())
        .await
        .unwrap();
    assert!(service.tasks.is_empty());
    assert!(service.identity("token").is_err());
    assert!(service.verify(&client, "new-token").await.is_err());
    assert_eq!(issuer.calls.load(Ordering::SeqCst), 2);
    issuer.blocked.store(false, Ordering::SeqCst);
    issuer.resume.notify_waiters();
    server.abort();
}
