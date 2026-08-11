use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use talos_error::{TalosError, TalosResult};
use talos_fs::{FilePermissions, FileSystem, WriteOptions};
use ts_rs::TS;

use crate::{AuthResponse, UserDto, UserRole, UserType};

const SCHEMA_VERSION: u32 = 2;
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
    pub r#type: UserType,
}

impl CloudSessionUserSnapshot {
    pub fn from_user(user: &UserDto) -> Self {
        Self {
            user_id: user.user_id.clone(),
            username: user.username.clone(),
            email: user.email.clone(),
            role: user.role,
            r#type: user.r#type,
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
        let content = match fs::read_to_string(&self.path) {
            Ok(content) => content,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => {
                return Err(TalosError::internal(format!(
                    "read cloud session cache {}: {}",
                    self.path.display(),
                    err
                )));
            }
        };
        let entry: CloudSessionCacheEntry = serde_json::from_str(&content).map_err(|err| {
            TalosError::internal(format!(
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
            return Err(TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                "cloud session cache schema version is unsupported".to_string(),
            ));
        }
        if entry.access_token.trim().is_empty() {
            return Err(TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
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

fn acquire_cache_lock(path: &Path) -> TalosResult<talos_fs::FileLock> {
    talos_fs::std_fs().try_file_lock(path)?.ok_or_else(|| {
        TalosError::public(
            talos_error::ErrorCode::Conflict,
            format!(
                "cloud session cache lock already exists: {}",
                path.display()
            ),
        )
    })
}

fn lock_path(path: &Path) -> PathBuf {
    let mut lock_path = path.to_path_buf();
    lock_path.set_extension(LOCK_EXTENSION);
    lock_path
}

#[cfg(test)]
#[path = "session_cache_tests.rs"]
mod tests;
