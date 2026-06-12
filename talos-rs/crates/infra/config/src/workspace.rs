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
        return Err(TalosError::ValidationError(
            "user_id cannot be empty for workspace resolution".to_string(),
        ));
    }
    if trimmed != user_id {
        return Err(TalosError::ValidationError(
            "user_id must not contain leading or trailing whitespace".to_string(),
        ));
    }
    if !trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        return Err(TalosError::ValidationError(format!(
            "user_id is not path-safe: {}",
            user_id
        )));
    }
    Ok(trimmed)
}

pub fn validate_session_workspace_user<'a>(
    session_user_id: &'a str,
    context_user_id: &str,
) -> TalosResult<&'a str> {
    if session_user_id != context_user_id {
        return Err(TalosError::InternalError(
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
mod tests {
    use super::*;

    fn test_kernel_config() -> AgentKernelConfig {
        let mut config = AgentKernelConfig::default();
        config.workspace_dir = "/tmp/talos-workspace".to_string();
        config
    }

    #[test]
    fn resolves_user_workspace_from_base_prefix() {
        let workspace = resolve_effective_kernel_workspace(
            &test_kernel_config(),
            "user_0123456789abcdef0123456789abcdef",
        )
        .expect("cloud user id should resolve");

        assert_eq!(
            workspace.user_workspace_dir,
            PathBuf::from("/tmp/talos-workspace")
                .join("users")
                .join("user_0123456789abcdef0123456789abcdef")
        );
        assert_eq!(
            workspace.kernel_config.workspace_dir,
            workspace.user_workspace_dir.to_string_lossy().to_string()
        );
    }

    #[test]
    fn resolves_anonymous_workspace() {
        let workspace = resolve_effective_kernel_workspace(&test_kernel_config(), "anonymous")
            .expect("anonymous should resolve");

        assert!(workspace
            .user_workspace_dir
            .ends_with(PathBuf::from("users").join("anonymous")));
    }

    #[test]
    fn rejects_unsafe_user_id() {
        let error = resolve_effective_kernel_workspace(&test_kernel_config(), "../alice")
            .expect_err("unsafe user id should fail");

        assert!(matches!(error, TalosError::ValidationError(_)));
    }

    #[test]
    fn rejects_inconsistent_session_user() {
        let error = validate_session_workspace_user("user_1", "user_2")
            .expect_err("session user mismatch should fail");

        assert!(matches!(error, TalosError::InternalError(_)));
    }
}
