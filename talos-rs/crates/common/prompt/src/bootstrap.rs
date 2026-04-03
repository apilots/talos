use std::path::{Path, PathBuf};
use talos_config::resolve_runtime_path;
use talos_error::{TalosError, TalosResult};
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;

pub(crate) fn expand_workspace_dir(workspace_dir: &str) -> PathBuf {
    resolve_runtime_path(workspace_dir)
}

pub(crate) async fn ensure_bootstrap_file(path: &Path, template: &str) -> TalosResult<()> {
    if fs::try_exists(path).await.map_err(|err| {
        TalosError::StorageError(format!("check bootstrap file {}: {}", path.display(), err))
    })? {
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await.map_err(|err| {
            TalosError::StorageError(format!(
                "create bootstrap dir {}: {}",
                parent.display(),
                err
            ))
        })?;
    }

    match OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .await
    {
        Ok(mut file) => {
            file.write_all(template.as_bytes()).await.map_err(|err| {
                TalosError::StorageError(format!(
                    "write bootstrap template {}: {}",
                    path.display(),
                    err
                ))
            })?;
            Ok(())
        }
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(err) => Err(TalosError::StorageError(format!(
            "open bootstrap file {}: {}",
            path.display(),
            err
        ))),
    }
}

pub(crate) fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }

    const SUFFIX: &str = "...[truncated]";
    if max_chars <= SUFFIX.len() {
        return SUFFIX.chars().take(max_chars).collect();
    }

    let keep_chars = max_chars - SUFFIX.len();
    let mut out = text.chars().take(keep_chars).collect::<String>();
    out.push_str(SUFFIX);
    out
}
