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
    if let Some(project_root) = context.project_root() {
        return Ok(project_root);
    }

    if let Some(config) = context.kernel_config() {
        let workspace_dir = config.memory.workspace_dir.trim();
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use talos_config::AgentKernelConfig;

    #[test]
    fn resolve_base_dir_prefers_explicit_project_root() {
        let mut config = AgentKernelConfig::default();
        config.memory.workspace_dir = "/tmp/talos-workspace".to_string();
        let context = crate::ToolContext::new_with_kernel_config(
            "session-1",
            "user-1",
            Some(Arc::new(config)),
        )
        .with_project_root("/tmp/project-root");

        let base_dir = resolve_base_dir(&context).expect("base dir should resolve");

        assert_eq!(base_dir, PathBuf::from("/tmp/project-root"));
    }

    #[test]
    fn resolve_base_dir_falls_back_to_workspace_dir_without_project_root() {
        let mut config = AgentKernelConfig::default();
        config.memory.workspace_dir = "/tmp/talos-workspace".to_string();
        let context = crate::ToolContext::new_with_kernel_config(
            "session-1",
            "user-1",
            Some(Arc::new(config)),
        );

        let base_dir = resolve_base_dir(&context).expect("base dir should resolve");

        assert_eq!(base_dir, PathBuf::from("/tmp/talos-workspace"));
    }

    #[test]
    fn resolve_base_dir_errors_without_project_root_or_workspace_dir() {
        let context = crate::ToolContext::new("session-1", "user-1");

        let error = resolve_base_dir(&context).expect_err("resolution should fail");

        assert!(error.to_string().contains("no project root is configured"));
    }
}
