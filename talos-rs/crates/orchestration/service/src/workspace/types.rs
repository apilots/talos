use std::path::PathBuf;

use talos_types::TalosConfig;

#[derive(Debug, Clone)]
pub struct EffectiveAgentWorkspace {
    pub user_id: String,
    pub username: Option<String>,
    pub base_workspace_dir: PathBuf,
    pub user_workspace_dir: PathBuf,
    pub config: TalosConfig,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AgentWorkspaceInfo {
    pub user_id: String,
    pub username: Option<String>,
    pub base_workspace_dir: String,
    pub user_workspace_dir: String,
}

impl From<EffectiveAgentWorkspace> for AgentWorkspaceInfo {
    fn from(workspace: EffectiveAgentWorkspace) -> Self {
        Self {
            user_id: workspace.user_id,
            username: workspace.username,
            base_workspace_dir: workspace.base_workspace_dir.to_string_lossy().to_string(),
            user_workspace_dir: workspace.user_workspace_dir.to_string_lossy().to_string(),
        }
    }
}
