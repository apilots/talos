use std::path::{Path, PathBuf};
use talos_config::resolve_runtime_path;
use talos_error::{TalosError, TalosResult};
use talos_fs::{FileSystem, WriteOptions};

pub(crate) fn expand_workspace_dir(workspace_dir: &str) -> PathBuf {
    resolve_runtime_path(workspace_dir)
}

pub(crate) async fn ensure_bootstrap_file(path: &Path, template: &str) -> TalosResult<()> {
    let path = path.to_path_buf();
    let template = template.to_string();
    match tokio::task::spawn_blocking(move || {
        talos_fs::std_fs().write_file_with(&path, &template, WriteOptions::create_new())
    })
    .await
    .map_err(|err| TalosError::StorageError(format!("join bootstrap write task: {err}")))?
    {
        Ok(()) => Ok(()),
        Err(TalosError::Conflict(_)) => Ok(()),
        Err(err) => Err(err),
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
