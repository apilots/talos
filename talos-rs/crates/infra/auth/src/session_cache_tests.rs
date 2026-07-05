use super::*;
use std::fs;

fn test_entry(now_ms: i64) -> CloudSessionCacheEntry {
    CloudSessionCacheEntry {
        schema_version: SCHEMA_VERSION,
        cloud_api_base_url: "http://127.0.0.1:8787".to_string(),
        access_token: "token-1".to_string(),
        token_type: "Bearer".to_string(),
        expires_at_ms: Some(now_ms + 60_000),
        last_used_at_ms: now_ms,
        user: CloudSessionUserSnapshot {
            user_id: "user-1".to_string(),
            username: "ada".to_string(),
            email: Some("ada@example.com".to_string()),
            role: UserRole::User,
            r#type: UserType::Free,
        },
    }
}

fn long_lived_test_entry(now_ms: i64) -> CloudSessionCacheEntry {
    let mut entry = test_entry(now_ms);
    entry.expires_at_ms = Some(now_ms + DEFAULT_CLOUD_SESSION_IDLE_TIMEOUT_MS);
    entry
}

#[test]
fn cloud_session_cache_uses_talos_home_auth_path() {
    let cache = CloudSessionCache::for_talos_home("/tmp/talos-home");
    assert_eq!(
        cache.path(),
        Path::new("/tmp/talos-home/auth/cloud-session.json")
    );
}

#[test]
fn cloud_session_cache_round_trips_valid_candidate() {
    let dir = tempfile::tempdir().unwrap();
    let cache = CloudSessionCache::for_talos_home(dir.path());
    let now_ms = 1_000_000;
    let entry = test_entry(now_ms);

    cache.store(&entry).unwrap();

    let loaded = cache.load_candidate(now_ms + 1_000).unwrap().unwrap();
    assert_eq!(loaded, entry);
}

#[test]
fn cloud_session_cache_rejects_idle_expired_candidate() {
    let dir = tempfile::tempdir().unwrap();
    let cache = CloudSessionCache::for_talos_home(dir.path()).with_idle_timeout_ms(5_000);
    let now_ms = 1_000_000;
    cache.store(&long_lived_test_entry(now_ms)).unwrap();

    assert_eq!(cache.load_candidate(now_ms + 5_000).unwrap(), None);
}

#[test]
fn cloud_session_cache_rejects_cloud_expired_candidate() {
    let dir = tempfile::tempdir().unwrap();
    let cache = CloudSessionCache::for_talos_home(dir.path());
    let now_ms = 1_000_000;
    let mut entry = test_entry(now_ms);
    entry.expires_at_ms = Some(now_ms + 1_000);
    cache.store(&entry).unwrap();

    assert_eq!(cache.load_candidate(now_ms + 1_000).unwrap(), None);
}

#[test]
fn cloud_session_cache_touch_updates_last_used_at() {
    let dir = tempfile::tempdir().unwrap();
    let cache = CloudSessionCache::for_talos_home(dir.path());
    let now_ms = 1_000_000;
    cache.store(&long_lived_test_entry(now_ms)).unwrap();

    let touched = cache.touch(now_ms + 2_000).unwrap().unwrap();
    assert_eq!(touched.last_used_at_ms, now_ms + 2_000);
    assert_eq!(
        cache
            .load_candidate(now_ms + 2_001)
            .unwrap()
            .unwrap()
            .last_used_at_ms,
        now_ms + 2_000
    );
}

#[test]
fn cloud_session_cache_touch_if_stale_skips_recent_use() {
    let dir = tempfile::tempdir().unwrap();
    let cache = CloudSessionCache::for_talos_home(dir.path());
    let now_ms = 1_000_000;
    cache.store(&long_lived_test_entry(now_ms)).unwrap();

    let touched = cache
        .touch_if_stale(
            now_ms + DEFAULT_CLOUD_SESSION_TOUCH_INTERVAL_MS - 1,
            DEFAULT_CLOUD_SESSION_TOUCH_INTERVAL_MS,
        )
        .unwrap()
        .unwrap();

    assert_eq!(touched.last_used_at_ms, now_ms);
    assert_eq!(
        cache
            .load_candidate(now_ms + DEFAULT_CLOUD_SESSION_TOUCH_INTERVAL_MS - 1)
            .unwrap()
            .unwrap()
            .last_used_at_ms,
        now_ms
    );
}

#[test]
fn cloud_session_cache_touch_if_stale_refreshes_after_interval() {
    let dir = tempfile::tempdir().unwrap();
    let cache = CloudSessionCache::for_talos_home(dir.path());
    let now_ms = 1_000_000;
    cache.store(&long_lived_test_entry(now_ms)).unwrap();

    let touched = cache
        .touch_if_stale(
            now_ms + DEFAULT_CLOUD_SESSION_TOUCH_INTERVAL_MS,
            DEFAULT_CLOUD_SESSION_TOUCH_INTERVAL_MS,
        )
        .unwrap()
        .unwrap();

    assert_eq!(
        touched.last_used_at_ms,
        now_ms + DEFAULT_CLOUD_SESSION_TOUCH_INTERVAL_MS
    );
}

#[test]
fn cloud_session_manager_touches_only_matching_verified_token() {
    let dir = tempfile::tempdir().unwrap();
    let cache = CloudSessionCache::for_talos_home(dir.path());
    let manager = CloudSessionCacheManager::new(cache.clone()).with_touch_interval_ms(1_000);
    let now_ms = 1_000_000;
    cache.store(&long_lived_test_entry(now_ms)).unwrap();

    manager
        .record_verified_token_use("different-token", now_ms + 2_000)
        .unwrap();
    assert_eq!(
        cache
            .load_candidate(now_ms + 2_000)
            .unwrap()
            .unwrap()
            .last_used_at_ms,
        now_ms
    );

    manager
        .record_verified_token_use("token-1", now_ms + 2_000)
        .unwrap();
    assert_eq!(
        cache
            .load_candidate(now_ms + 2_001)
            .unwrap()
            .unwrap()
            .last_used_at_ms,
        now_ms + 2_000
    );
}

#[test]
fn cloud_session_manager_clears_only_matching_rejected_token() {
    let dir = tempfile::tempdir().unwrap();
    let cache = CloudSessionCache::for_talos_home(dir.path());
    let manager = CloudSessionCacheManager::new(cache.clone());
    let now_ms = 1_000_000;
    cache.store(&long_lived_test_entry(now_ms)).unwrap();

    manager
        .clear_rejected_token("different-token", now_ms + 1_000)
        .unwrap();
    assert!(cache.load_candidate(now_ms + 1_000).unwrap().is_some());

    manager
        .clear_rejected_token("token-1", now_ms + 1_000)
        .unwrap();
    assert_eq!(cache.load_candidate(now_ms + 1_000).unwrap(), None);
}

#[test]
fn cloud_session_cache_clear_removes_file() {
    let dir = tempfile::tempdir().unwrap();
    let cache = CloudSessionCache::for_talos_home(dir.path());
    cache.store(&test_entry(1_000_000)).unwrap();

    cache.clear().unwrap();

    assert_eq!(cache.load_candidate(1_000_001).unwrap(), None);
}

#[cfg(unix)]
#[test]
fn cloud_session_cache_file_is_private() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let cache = CloudSessionCache::for_talos_home(dir.path());
    cache.store(&test_entry(1_000_000)).unwrap();

    let mode = fs::metadata(cache.path()).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, FILE_PERMISSION_MODE);
}
