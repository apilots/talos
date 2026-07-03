use std::path::PathBuf;

use talos_types::talos::TalosContext;
use talos_types::{ModelRef, SessionConf};

/// Parameters for creating a session from a resolved runtime context.
pub struct CreateSessionParams {
    pub session_id: Option<String>,
    pub context: TalosContext,
    pub target_agent: String,
    pub model_ref: Option<ModelRef>,
    pub agent_model_ref: Option<ModelRef>,
    pub conf: Option<SessionConf>,
    pub workspace_id: Option<String>,
    pub project_id: Option<String>,
    pub current_directory: Option<PathBuf>,
}
