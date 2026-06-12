pub mod bash;
pub mod fs_edit;
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
mod tests {
    use super::*;
    use std::sync::Arc;
    use talos_config::AgentKernelConfig;

    #[test]
    fn resolve_base_dir_prefers_explicit_project_root() {
        let mut config = AgentKernelConfig::default();
        config.workspace_dir = "/tmp/talos-workspace".to_string();
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
        config.workspace_dir = "/tmp/talos-workspace".to_string();
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
