use std::path::PathBuf;

use talos_types::talos::SessionSource;
use talos_types::{ModelSelection, SessionConf};

/// Parameters for creating a Session from a resolved request context.
pub(crate) struct CreateSessionParams {
    pub session_id: Option<String>,
    pub source: SessionSource,
    pub user_id: String,
    pub target_agent: String,
    pub execution_node_id: String,
    pub model_selection: Option<ModelSelection>,
    pub conf: Option<SessionConf>,
    pub workspace_id: String,
    pub project_id: Option<String>,
    pub current_directory: Option<PathBuf>,
}
