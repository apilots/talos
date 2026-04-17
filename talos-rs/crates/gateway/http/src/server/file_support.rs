use std::path::{Path, PathBuf};

use talos_config::resolve_runtime_path;
use talos_error::{TalosError, TalosResult};
use talos_types::{DocUpdateRequest, ExpectedFileVersion, TalosConfig};
use tokio::fs;

use super::state::HttpApiState;

pub(super) fn workspace_root(config: &TalosConfig) -> PathBuf {
    resolve_runtime_path(&config.agent.kernel.memory.workspace_dir)
}

/// Non-cryptographic hash used only for content change detection and version tokens.
pub(super) fn content_hash(content: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in content.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

pub(super) async fn read_optional(path: &Path) -> TalosResult<Option<String>> {
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

pub(super) async fn modified_at_ms(path: &Path) -> TalosResult<Option<i64>> {
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

pub(super) fn validate_expected_version(
    existing_content: Option<&str>,
    request: &DocUpdateRequest,
) -> TalosResult<()> {
    match (&request.base_version, existing_content) {
        (ExpectedFileVersion::MustNotExist, Some(_)) => Err(TalosError::ValidationError(
            "target file already exists".to_string(),
        )),
        (ExpectedFileVersion::MustNotExist, None) => Ok(()),
        (ExpectedFileVersion::Match { .. }, None) => Err(TalosError::ValidationError(
            "target file does not exist".to_string(),
        )),
        (ExpectedFileVersion::Match { version_token }, Some(content)) => {
            let current = content_hash(content);
            if current == *version_token {
                Ok(())
            } else {
                Err(TalosError::ValidationError(
                    "file version token mismatch".to_string(),
                ))
            }
        }
    }
}

pub(super) async fn write_atomic(path: &Path, content: &str) -> TalosResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await.map_err(|err| {
            TalosError::StorageError(format!("create dir {}: {}", parent.display(), err))
        })?;
    }

    let temp_path = path.with_extension(format!(
        "{}.tmp-{}",
        path.extension()
            .and_then(|value| value.to_str())
            .unwrap_or("tmp"),
        uuid::Uuid::new_v4()
    ));
    fs::write(&temp_path, content).await.map_err(|err| {
        TalosError::StorageError(format!("write temp file {}: {}", temp_path.display(), err))
    })?;
    fs::rename(&temp_path, path).await.map_err(|err| {
        TalosError::StorageError(format!(
            "replace file {} from {}: {}",
            path.display(),
            temp_path.display(),
            err
        ))
    })?;
    Ok(())
}

pub(super) async fn with_file_lock<T, Fut>(
    state: &HttpApiState,
    path: &Path,
    fut: Fut,
) -> TalosResult<T>
where
    Fut: std::future::Future<Output = TalosResult<T>>,
{
    let lock = state.file_locks.lock_for(path.display().to_string()).await;
    let _guard = lock.lock().await;
    fut.await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_expected_version_accepts_create_when_file_missing() {
        let request = DocUpdateRequest {
            content: "new doc".to_string(),
            base_version: ExpectedFileVersion::MustNotExist,
        };

        validate_expected_version(None, &request).expect("missing file should allow create");
    }

    #[test]
    fn test_validate_expected_version_rejects_create_when_file_exists() {
        let request = DocUpdateRequest {
            content: "new doc".to_string(),
            base_version: ExpectedFileVersion::MustNotExist,
        };

        let error = validate_expected_version(Some("existing"), &request)
            .expect_err("existing file should reject create-only request");
        assert!(error.to_string().contains("already exists"));
    }

    #[test]
    fn test_validate_expected_version_rejects_stale_token() {
        let request = DocUpdateRequest {
            content: "updated".to_string(),
            base_version: ExpectedFileVersion::Match {
                version_token: "stale".to_string(),
            },
        };

        let error = validate_expected_version(Some("current"), &request)
            .expect_err("stale token should reject write");
        assert!(error.to_string().contains("mismatch"));
    }

    #[test]
    fn test_validate_expected_version_accepts_matching_token() {
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
