use std::path::PathBuf;

use talos_types::TalosConfig;

#[derive(Debug, Clone)]
pub struct EffectiveUserWorkspace {
    pub workspace_id: String,
    pub user_id: String,
    pub username: Option<String>,
    pub base_workspace_dir: PathBuf,
    pub user_workspace_dir: PathBuf,
    pub config: TalosConfig,
}
