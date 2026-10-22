use super::*;

#[test]
fn cloud_failures_do_not_turn_server_errors_into_logout_or_expose_remote_bodies() {
    for (status, expected) in [
        (401, talos_error::ErrorCode::Unauthenticated),
        (403, talos_error::ErrorCode::PermissionDenied),
        (429, talos_error::ErrorCode::UsageLimitExceeded),
        (500, talos_error::ErrorCode::ServiceUnavailable),
        (501, talos_error::ErrorCode::InternalError),
        (503, talos_error::ErrorCode::ServiceUnavailable),
    ] {
        let error = map_cloud_error(CloudSdkError::Api {
            status: reqwest::StatusCode::from_u16(status).unwrap(),
            code: "remote-secret-code".to_string(),
            message: "remote-secret-body".to_string(),
        });
        assert_eq!(error.error_code(), expected);
        assert!(!error.to_string().contains("remote-secret"));
        assert!(std::error::Error::source(&error)
            .unwrap()
            .is::<CloudSdkError>());
    }
}

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
        Arc::new(PendingCloudLogin {
            device_code: "device-1".to_string(),
            persistence: CloudLoginPersistence::None,
            expires_at_ms: now_ms - 1,
            interval_ms: 1000,
            approved: Default::default(),
        }),
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
            Arc::new(PendingCloudLogin {
                device_code: "device-active".to_string(),
                persistence: CloudLoginPersistence::None,
                expires_at_ms: now_ms + 60_000,
                interval_ms: 1000,
                approved: Default::default(),
            }),
        ),
        (
            "expired".to_string(),
            Arc::new(PendingCloudLogin {
                device_code: "device-expired".to_string(),
                persistence: CloudLoginPersistence::None,
                expires_at_ms: now_ms - 1,
                interval_ms: 1000,
                approved: Default::default(),
            }),
        ),
    ]);

    let lookup = service.lookup_pending("active").expect("lookup pending");

    assert!(matches!(lookup, PendingLookup::Active(_)));
    let pending = service.pending.lock().expect("lock pending");
    assert!(pending.contains_key("active"));
    assert!(!pending.contains_key("expired"));
}
