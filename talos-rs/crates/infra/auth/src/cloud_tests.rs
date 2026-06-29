use super::*;

fn test_service() -> CloudLoginService {
    let dir = tempfile::tempdir().expect("create temp dir");
    CloudLoginService::new(
        "http://127.0.0.1:8787",
        CloudSessionCacheManager::for_talos_home(dir.path()),
    )
}

#[test]
fn lookup_pending_returns_expired_and_removes_stale_login() {
    let service = test_service();
    let now_ms = CloudSessionCache::now_ms();
    service.pending.lock().expect("lock pending").insert(
        "login-1".to_string(),
        PendingCloudLogin {
            device_code: "device-1".to_string(),
            persistence: CloudLoginPersistence::None,
            expires_at_ms: now_ms - 1,
        },
    );

    let lookup = service.lookup_pending("login-1").expect("lookup pending");

    assert!(matches!(lookup, PendingLookup::Expired));
    assert!(!service
        .pending
        .lock()
        .expect("lock pending")
        .contains_key("login-1"));
}

#[test]
fn lookup_pending_cleans_other_expired_logins() {
    let service = test_service();
    let now_ms = CloudSessionCache::now_ms();
    service.pending.lock().expect("lock pending").extend([
        (
            "active".to_string(),
            PendingCloudLogin {
                device_code: "device-active".to_string(),
                persistence: CloudLoginPersistence::None,
                expires_at_ms: now_ms + 60_000,
            },
        ),
        (
            "expired".to_string(),
            PendingCloudLogin {
                device_code: "device-expired".to_string(),
                persistence: CloudLoginPersistence::None,
                expires_at_ms: now_ms - 1,
            },
        ),
    ]);

    let lookup = service.lookup_pending("active").expect("lookup pending");

    assert!(matches!(lookup, PendingLookup::Active(_)));
    let pending = service.pending.lock().expect("lock pending");
    assert!(pending.contains_key("active"));
    assert!(!pending.contains_key("expired"));
}

#[test]
fn local_persistence_stores_cloud_session_cache() {
    assert!(!should_store_cloud_session(CloudLoginPersistence::None));
    assert!(should_store_cloud_session(
        CloudLoginPersistence::LocalCache
    ));
}
