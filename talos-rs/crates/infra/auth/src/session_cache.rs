use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use talos_error::{TalosError, TalosResult};
use talos_fs::{FileLock, FilePermissions, WriteOptions};
use ts_rs::TS;

use crate::{AuthResponse, UserDto, UserRole};

const SCHEMA_VERSION: u32 = 1;
const CACHE_RELATIVE_PATH: &[&str] = &["auth", "cloud-session.json"];
const LOCK_EXTENSION: &str = "lock";
const FILE_PERMISSION_MODE: u32 = 0o600;
const FILE_PERMISSIONS: FilePermissions = FilePermissions::unix(FILE_PERMISSION_MODE);

pub const DEFAULT_CLOUD_SESSION_IDLE_TIMEOUT_MS: i64 = 15 * 24 * 60 * 60 * 1000;
pub const DEFAULT_CLOUD_SESSION_TOUCH_INTERVAL_MS: i64 = 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, TS, PartialEq, Eq)]
pub struct CloudSessionCacheEntry {
    pub schema_version: u32,
    pub cloud_api_base_url: String,
    pub access_token: String,
    pub token_type: String,
    #[serde(default)]
    pub expires_at_ms: Option<i64>,
    pub last_used_at_ms: i64,
    pub user: CloudSessionUserSnapshot,
}

impl CloudSessionCacheEntry {
    pub fn from_auth_response(
        cloud_api_base_url: impl Into<String>,
        auth_response: &AuthResponse,
        now_ms: i64,
    ) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            cloud_api_base_url: cloud_api_base_url.into(),
            access_token: auth_response.access_token.clone(),
            token_type: auth_response.token_type.clone(),
            expires_at_ms: Some(auth_response.expires_at as i64),
            last_used_at_ms: now_ms,
            user: CloudSessionUserSnapshot::from_user(&auth_response.user),
        }
    }

    pub fn from_verified_user(
        cloud_api_base_url: impl Into<String>,
        access_token: impl Into<String>,
        token_type: impl Into<String>,
        expires_at_ms: Option<i64>,
        user: &UserDto,
        now_ms: i64,
    ) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            cloud_api_base_url: cloud_api_base_url.into(),
            access_token: access_token.into(),
            token_type: token_type.into(),
            expires_at_ms,
            last_used_at_ms: now_ms,
            user: CloudSessionUserSnapshot::from_user(user),
        }
    }

    pub fn is_candidate_usable(&self, now_ms: i64, idle_timeout_ms: i64) -> bool {
        if self.schema_version != SCHEMA_VERSION || self.access_token.trim().is_empty() {
            return false;
        }
        if self.last_used_at_ms + idle_timeout_ms <= now_ms {
            return false;
        }
        if matches!(self.expires_at_ms, Some(expires_at_ms) if expires_at_ms <= now_ms) {
            return false;
        }
        true
    }

    pub fn mark_used(&mut self, now_ms: i64) {
        self.last_used_at_ms = now_ms;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, TS, PartialEq, Eq)]
pub struct CloudSessionUserSnapshot {
    pub user_id: String,
    pub username: String,
    #[serde(default)]
    pub email: Option<String>,
    pub role: UserRole,
}

impl CloudSessionUserSnapshot {
    pub fn from_user(user: &UserDto) -> Self {
        Self {
            user_id: user.user_id.clone(),
            username: user.username.clone(),
            email: user.email.clone(),
            role: user.role,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CloudSessionCache {
    path: PathBuf,
    idle_timeout_ms: i64,
}

#[derive(Debug, Clone)]
pub struct CloudSessionCacheManager {
    cache: CloudSessionCache,
    touch_interval_ms: i64,
}

impl CloudSessionCacheManager {
    pub fn for_talos_home(talos_home: impl Into<PathBuf>) -> Self {
        Self::new(CloudSessionCache::for_talos_home(talos_home))
    }

    pub fn new(cache: CloudSessionCache) -> Self {
        Self {
            cache,
            touch_interval_ms: DEFAULT_CLOUD_SESSION_TOUCH_INTERVAL_MS,
        }
    }

    pub fn with_touch_interval_ms(mut self, touch_interval_ms: i64) -> Self {
        self.touch_interval_ms = touch_interval_ms;
        self
    }

    pub fn cache(&self) -> &CloudSessionCache {
        &self.cache
    }

    pub fn read_candidate(&self, now_ms: i64) -> TalosResult<Option<CloudSessionCacheEntry>> {
        self.cache.load_candidate(now_ms)
    }

    pub fn store_verified_session(&self, entry: &CloudSessionCacheEntry) -> TalosResult<()> {
        self.cache.store(entry)
    }

    pub fn record_verified_token_use(
        &self,
        token: &str,
        now_ms: i64,
    ) -> TalosResult<Option<CloudSessionCacheEntry>> {
        let Some(mut entry) = self.cache.load_candidate(now_ms)? else {
            return Ok(None);
        };
        if entry.access_token != token {
            return Ok(Some(entry));
        }
        if now_ms - entry.last_used_at_ms < self.touch_interval_ms {
            return Ok(Some(entry));
        }
        entry.mark_used(now_ms);
        self.cache.store(&entry)?;
        Ok(Some(entry))
    }

    pub fn clear_rejected_token(&self, token: &str, now_ms: i64) -> TalosResult<()> {
        if self
            .cache
            .load_candidate(now_ms)?
            .is_some_and(|entry| entry.access_token == token)
        {
            self.cache.clear()?;
        }
        Ok(())
    }

    pub fn clear(&self) -> TalosResult<()> {
        self.cache.clear()
    }
}

impl CloudSessionCache {
    pub fn for_talos_home(talos_home: impl Into<PathBuf>) -> Self {
        let mut path = talos_home.into();
        for segment in CACHE_RELATIVE_PATH {
            path.push(segment);
        }
        Self::new(path)
    }

    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            idle_timeout_ms: DEFAULT_CLOUD_SESSION_IDLE_TIMEOUT_MS,
        }
    }

    pub fn with_idle_timeout_ms(mut self, idle_timeout_ms: i64) -> Self {
        self.idle_timeout_ms = idle_timeout_ms;
        self
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load_candidate(&self, now_ms: i64) -> TalosResult<Option<CloudSessionCacheEntry>> {
        let Some(content) = talos_fs::read_optional_to_string(&self.path)? else {
            return Ok(None);
        };
        let entry: CloudSessionCacheEntry = serde_json::from_str(&content).map_err(|err| {
            TalosError::SerializationError(format!(
                "parse cloud session cache {}: {}",
                self.path.display(),
                err
            ))
        })?;
        if entry.is_candidate_usable(now_ms, self.idle_timeout_ms) {
            Ok(Some(entry))
        } else {
            Ok(None)
        }
    }

    pub fn store(&self, entry: &CloudSessionCacheEntry) -> TalosResult<()> {
        if entry.schema_version != SCHEMA_VERSION {
            return Err(TalosError::ValidationError(
                "cloud session cache schema version is unsupported".to_string(),
            ));
        }
        if entry.access_token.trim().is_empty() {
            return Err(TalosError::ValidationError(
                "cloud session cache access token is empty".to_string(),
            ));
        }
        let _lock = acquire_cache_lock(&lock_path(&self.path))?;
        talos_fs::write_json_pretty_with(
            &self.path,
            entry,
            WriteOptions::default().with_permissions(FILE_PERMISSIONS),
        )
    }

    pub fn touch(&self, now_ms: i64) -> TalosResult<Option<CloudSessionCacheEntry>> {
        let Some(mut entry) = self.load_candidate(now_ms)? else {
            return Ok(None);
        };
        entry.mark_used(now_ms);
        self.store(&entry)?;
        Ok(Some(entry))
    }

    pub fn touch_if_stale(
        &self,
        now_ms: i64,
        touch_interval_ms: i64,
    ) -> TalosResult<Option<CloudSessionCacheEntry>> {
        let Some(mut entry) = self.load_candidate(now_ms)? else {
            return Ok(None);
        };
        if now_ms - entry.last_used_at_ms < touch_interval_ms {
            return Ok(Some(entry));
        }
        entry.mark_used(now_ms);
        self.store(&entry)?;
        Ok(Some(entry))
    }

    pub fn clear(&self) -> TalosResult<()> {
        let _lock = acquire_cache_lock(&lock_path(&self.path))?;
        talos_fs::remove_file(&self.path)
    }

    pub fn now_ms() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .min(i64::MAX as u128) as i64
    }
}

fn acquire_cache_lock(path: &Path) -> TalosResult<FileLock> {
    talos_fs::try_lock_exclusive(path)?.ok_or_else(|| {
        TalosError::Conflict(format!(
            "cloud session cache lock already exists: {}",
            path.display()
        ))
    })
}

fn lock_path(path: &Path) -> PathBuf {
    let mut lock_path = path.to_path_buf();
    lock_path.set_extension(LOCK_EXTENSION);
    lock_path
}

#[cfg(test)]
mod tests {
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
                role: UserRole::Admin,
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
}
