pub mod apply_patch;
pub mod exec;
pub mod fs_list;
pub mod fs_read;
pub mod fs_search;

use std::path::{Path, PathBuf};

use talos_config::resolve_runtime_path;
use talos_error::{TalosError, TalosResult};

use crate::ToolContext;

const TRUNCATED_SUFFIX: &str = "...[truncated]";

pub(crate) fn expand_workspace_dir(workspace_dir: &str) -> PathBuf {
    resolve_runtime_path(workspace_dir)
}

pub(crate) fn resolve_base_dir(context: &ToolContext) -> TalosResult<PathBuf> {
    if let Some(config) = context.kernel_config() {
        let workspace_dir = config.memory.workspace_dir.trim();
        if !workspace_dir.is_empty() {
            return Ok(expand_workspace_dir(workspace_dir));
        }
    }

    std::env::current_dir()
        .map_err(|err| TalosError::ToolError(format!("resolve current directory: {}", err)))
}

pub(crate) fn resolve_path(base_dir: &Path, raw_path: &str) -> PathBuf {
    let path = PathBuf::from(raw_path);
    if path.is_absolute() {
        path
    } else {
        base_dir.join(path)
    }
}

pub(crate) fn truncate_text(text: &str, max_bytes: usize) -> (String, bool) {
    if text.len() <= max_bytes {
        return (text.to_string(), false);
    }

    if max_bytes == 0 {
        return (String::new(), true);
    }

    if max_bytes <= TRUNCATED_SUFFIX.len() {
        return (TRUNCATED_SUFFIX[..max_bytes].to_string(), true);
    }

    let mut cutoff = max_bytes - TRUNCATED_SUFFIX.len();
    while cutoff > 0 && !text.is_char_boundary(cutoff) {
        cutoff -= 1;
    }

    let mut output = String::with_capacity(max_bytes);
    output.push_str(&text[..cutoff]);
    output.push_str(TRUNCATED_SUFFIX);
    (output, true)
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
