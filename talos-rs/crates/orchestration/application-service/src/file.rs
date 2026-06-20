use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use talos_config::resolve_runtime_path;
use talos_error::{TalosError, TalosResult};
use talos_types::{DocUpdateRequest, ExpectedFileVersion, TalosConfig};
use tokio::fs;
use tokio::sync::Mutex;

#[derive(Clone, Default)]
pub struct FileLockRegistry {
    locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
}

impl FileLockRegistry {
    pub async fn lock_for(&self, key: impl Into<String>) -> Arc<Mutex<()>> {
        let key = key.into();
        let mut locks = self.locks.lock().await;
        locks
            .entry(key)
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }
}

pub async fn with_file_lock<T, Fut>(
    registry: &FileLockRegistry,
    path: &Path,
    fut: Fut,
) -> TalosResult<T>
where
    Fut: std::future::Future<Output = TalosResult<T>>,
{
    let lock = registry.lock_for(path.display().to_string()).await;
    let _guard = lock.lock().await;
    let lock_path = lock_path_for(path);
    let file_lock = tokio::task::spawn_blocking(move || talos_fs::lock_exclusive(&lock_path))
        .await
        .map_err(|err| TalosError::StorageError(format!("join file lock task: {err}")))??;
    let result = fut.await;
    let unlock_result = file_lock.unlock();
    match (result, unlock_result) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(err), _) => Err(err),
        (Ok(_), Err(err)) => Err(err),
    }
}

pub fn workspace_root(config: &TalosConfig) -> PathBuf {
    let workspace_dir = config.agent.kernel.workspace_dir.trim();
    resolve_runtime_path(workspace_dir)
}

pub fn content_hash(content: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in content.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

pub async fn read_optional(path: &Path) -> TalosResult<Option<String>> {
    match fs::read_to_string(path).await {
        Ok(content) => Ok(Some(content)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(TalosError::StorageError(format!(
            "read file {}: {}",
            path.display(),
            err
        ))),
    }
}

pub async fn modified_at_ms(path: &Path) -> TalosResult<Option<i64>> {
    match fs::metadata(path).await {
        Ok(metadata) => match metadata.modified() {
            Ok(value) => Ok(value
                .duration_since(std::time::UNIX_EPOCH)
                .ok()
                .and_then(|duration| i64::try_from(duration.as_millis()).ok())),
            Err(_) => Ok(None),
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(TalosError::StorageError(format!(
            "read file metadata {}: {}",
            path.display(),
            err
        ))),
    }
}

pub fn validate_expected_version(
    existing_content: Option<&str>,
    request: &DocUpdateRequest,
) -> TalosResult<()> {
    match (&request.base_version, existing_content) {
        (ExpectedFileVersion::MustNotExist, Some(_)) => Err(TalosError::Conflict(
            "target file already exists".to_string(),
        )),
        (ExpectedFileVersion::MustNotExist, None) => Ok(()),
        (ExpectedFileVersion::Match { .. }, None) => Err(TalosError::ResourceNotFound(
            "target file does not exist".to_string(),
        )),
        (ExpectedFileVersion::Match { version_token }, Some(content)) => {
            let current = content_hash(content);
            if current == *version_token {
                Ok(())
            } else {
                Err(TalosError::Conflict(
                    "file version token mismatch".to_string(),
                ))
            }
        }
    }
}

pub async fn write_atomic(path: &Path, content: &str) -> TalosResult<()> {
    let path = path.to_path_buf();
    let content = content.to_string();
    tokio::task::spawn_blocking(move || talos_fs::write_string(&path, &content))
        .await
        .map_err(|err| TalosError::StorageError(format!("join file write task: {err}")))?
}

fn lock_path_for(path: &Path) -> PathBuf {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| format!("{value}.lock"))
        .unwrap_or_else(|| "lock".to_string());
    path.with_extension(extension)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_expected_version_accepts_create_when_file_missing() {
        let request = DocUpdateRequest {
            content: "new doc".to_string(),
            base_version: ExpectedFileVersion::MustNotExist,
        };

        validate_expected_version(None, &request).expect("missing file should allow create");
    }

    #[test]
    fn validate_expected_version_rejects_create_when_file_exists() {
        let request = DocUpdateRequest {
            content: "new doc".to_string(),
            base_version: ExpectedFileVersion::MustNotExist,
        };

        let error = validate_expected_version(Some("existing"), &request)
            .expect_err("existing file should reject create-only request");
        assert!(matches!(error, TalosError::Conflict(_)));
    }

    #[test]
    fn validate_expected_version_rejects_stale_token() {
        let request = DocUpdateRequest {
            content: "updated".to_string(),
            base_version: ExpectedFileVersion::Match {
                version_token: "stale".to_string(),
            },
        };

        let error = validate_expected_version(Some("current"), &request)
            .expect_err("stale token should reject write");
        assert!(matches!(error, TalosError::Conflict(_)));
    }

    #[test]
    fn validate_expected_version_accepts_matching_token() {
        let request = DocUpdateRequest {
            content: "updated".to_string(),
            base_version: ExpectedFileVersion::Match {
                version_token: content_hash("current"),
            },
        };

        validate_expected_version(Some("current"), &request)
            .expect("matching token should allow write");
    }
}
