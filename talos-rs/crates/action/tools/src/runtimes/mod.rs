pub mod bash;
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
use std::{collections::BTreeMap, env};

use talos_config::resolve_runtime_path;
use talos_error::{TalosError, TalosResult};
use talos_tools_sandbox::{filter_inherited_tool_environment, TalosSandboxError};

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

    Err(TalosError::new(
        talos_error::ErrorCode::ToolExecutionFailed,
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

pub(crate) fn resolve_executable(program: &str) -> TalosResult<PathBuf> {
    let path = PathBuf::from(program);
    if path.components().count() > 1 {
        return std::fs::canonicalize(&path).map_err(|error| {
            TalosError::new(
                talos_error::ErrorCode::ToolExecutionFailed,
                format!("resolve executable {}: {error}", path.display()),
            )
        });
    }
    let paths = env::var_os("PATH").ok_or_else(|| TalosError::internal("PATH is unavailable"))?;
    env::split_paths(&paths)
        .map(|directory| directory.join(&path))
        .find(|candidate| candidate.is_file())
        .and_then(|candidate| std::fs::canonicalize(candidate).ok())
        .ok_or_else(|| {
            TalosError::new(
                talos_error::ErrorCode::ToolExecutionFailed,
                format!("executable not found on PATH: {program}"),
            )
        })
}

pub(crate) fn process_environment() -> BTreeMap<String, String> {
    filter_inherited_tool_environment(env::vars())
}

/// Project sandbox failures once for filesystem and process tools. Preserve
/// policy metadata, but keep provider reasons and resolved paths in the source.
pub(crate) fn map_sandbox_error(error: TalosSandboxError) -> TalosError {
    let public = match &error {
        TalosSandboxError::FileSystemAccessDenied { .. }
        | TalosSandboxError::SandboxBypassDenied
        | TalosSandboxError::Permission(_) => TalosError::tool_failure(
            talos_error::TalosToolErrorKind::PermissionRejection,
            "The sandbox denied this tool operation. Check the granted permissions.",
            Some("sandbox_policy".to_string()),
        ),
        TalosSandboxError::UnsupportedPlatform
        | TalosSandboxError::ProviderUnavailable { .. }
        | TalosSandboxError::UnsupportedCapability { .. } => TalosError::new(
            talos_error::ErrorCode::ServiceUnavailable,
            "The configured sandbox cannot support this tool operation.",
        ),
        TalosSandboxError::SandboxSetupFailed { .. } => TalosError::internal(
            "The sandbox could not be initialized; technical details were withheld.",
        ),
        TalosSandboxError::SandboxExecutionFailed { .. } => TalosError::internal(
            "The sandbox could not execute this tool; technical details were withheld.",
        ),
        TalosSandboxError::FileSystemOperationFailed { .. } => TalosError::internal(
            "The sandbox filesystem operation failed; technical details were withheld.",
        ),
        TalosSandboxError::InvalidLocalExecutionContext => TalosError::internal(
            "The tool execution context is invalid; technical details were withheld.",
        ),
    };
    public.with_source(error)
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
