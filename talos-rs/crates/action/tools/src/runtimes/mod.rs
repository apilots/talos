pub mod bash;
pub mod fs_apply_patch;
pub mod fs_diff;
pub mod fs_edit;
pub mod fs_glob;
pub mod fs_read;
pub mod fs_search;
pub mod fs_write;
pub mod task_state_runtime;
pub mod task_state_types;
pub mod web_common;
pub mod web_fetch;
pub mod web_search;

use std::path::{Path, PathBuf};

use talos_config::resolve_runtime_path;
use talos_error::{TalosError, TalosResult};

use crate::ToolContext;

pub(crate) fn expand_workspace_dir(workspace_dir: &str) -> PathBuf {
    resolve_runtime_path(workspace_dir)
}

pub(crate) fn resolve_base_dir(context: &ToolContext) -> TalosResult<PathBuf> {
    if let Some(project_root) = context.project_root() {
        return Ok(project_root);
    }

    if let Some(config) = context.kernel_config() {
        let workspace_dir = config.workspace_dir.trim();
        if !workspace_dir.is_empty() {
            return Ok(expand_workspace_dir(workspace_dir));
        }
    }

    Err(TalosError::ToolError(
        "no project root is configured for this tool invocation".to_string(),
    ))
}

pub(crate) fn resolve_path(base_dir: &Path, raw_path: &str) -> PathBuf {
    let path = PathBuf::from(raw_path);
    if path.is_absolute() {
        path
    } else {
        base_dir.join(path)
    }
}

pub(crate) fn truncate_prefix(text: &str, max_bytes: usize) -> (String, usize, bool) {
    if text.len() <= max_bytes {
        return (text.to_string(), text.len(), false);
    }

    let mut cutoff = max_bytes;
    while cutoff > 0 && !text.is_char_boundary(cutoff) {
        cutoff -= 1;
    }
    (text[..cutoff].to_string(), cutoff, true)
}

#[cfg(test)]
#[path = "runtimes_tests.rs"]
mod tests;
