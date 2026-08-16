use std::path::PathBuf;

use talos_error::{TalosError, TalosResult};

use crate::{resolve_runtime_path, AgentKernelConfig};

#[derive(Debug, Clone)]
pub struct EffectiveKernelWorkspace {
    pub user_id: String,
    pub base_workspace_dir: PathBuf,
    pub user_workspace_dir: PathBuf,
    pub kernel_config: AgentKernelConfig,
}

pub fn validate_user_workspace_segment(user_id: &str) -> TalosResult<&str> {
    let trimmed = user_id.trim();
    if trimmed.is_empty() {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "user_id cannot be empty for workspace resolution".to_string(),
        ));
    }
    if trimmed != user_id {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "user_id must not contain leading or trailing whitespace".to_string(),
        ));
    }
    if !trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            format!("user_id is not path-safe: {}", user_id),
        ));
    }
    Ok(trimmed)
}

pub fn validate_session_workspace_user<'a>(
    session_user_id: &'a str,
    context_user_id: &str,
) -> TalosResult<&'a str> {
    if session_user_id != context_user_id {
        return Err(TalosError::internal(
            "session user_id does not match session context user_id".to_string(),
        ));
    }
    validate_user_workspace_segment(session_user_id)
}

pub fn resolve_effective_kernel_workspace(
    base_kernel_config: &AgentKernelConfig,
    user_id: &str,
) -> TalosResult<EffectiveKernelWorkspace> {
    let safe_user_id = validate_user_workspace_segment(user_id)?;
    let base_workspace_dir = resolve_runtime_path(base_kernel_config.workspace_dir.trim());
    let user_workspace_dir = base_workspace_dir.join("users").join(safe_user_id);

    let mut kernel_config = base_kernel_config.clone();
    kernel_config.workspace_dir = user_workspace_dir.to_string_lossy().to_string();

    Ok(EffectiveKernelWorkspace {
        user_id: user_id.to_string(),
        base_workspace_dir,
        user_workspace_dir,
        kernel_config,
    })
}

#[cfg(test)]
#[path = "workspace_tests.rs"]
mod tests;
