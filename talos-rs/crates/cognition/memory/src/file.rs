use std::path::Path;

use talos_error::{TalosError, TalosResult};

pub(crate) async fn append_string(path: &Path, content: String) -> TalosResult<()> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || talos_fs::append_string(&path, &content))
        .await
        .map_err(|err| TalosError::StorageError(format!("join file append task: {err}")))?
}

pub(crate) async fn write_string(path: &Path, content: String) -> TalosResult<()> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || talos_fs::write_string(&path, &content))
        .await
        .map_err(|err| TalosError::StorageError(format!("join file write task: {err}")))?
}

pub(crate) async fn remove_dir_all(path: &Path) -> TalosResult<()> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || talos_fs::remove_dir_all(&path))
        .await
        .map_err(|err| TalosError::StorageError(format!("join directory remove task: {err}")))?
}
