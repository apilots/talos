use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use talos_types::talos::{TalosContext, TalosUsage};
use talos_types::{SessionConf, SessionConfigOverridePolicy, TalosSession, DEFAULT_SESSION_TITLE};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    pub id: String,
    #[serde(default)]
    pub workspace_id: Option<String>,
    pub title: String,
    pub user_id: String,
    pub context: TalosContext,
    pub model: String,
    pub created_at: u64,
    pub last_activity: u64,
    pub message_count: u64,
    #[serde(default)]
    pub usage: TalosUsage,
    #[serde(default)]
    pub conf: SessionConf,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub current_directory: Option<PathBuf>,
}

impl SessionRecord {
    pub fn from_session(session: &TalosSession, base_model: &str) -> Self {
        let effective_model = session.config.llm.model.clone();
        let conf = SessionConfigOverridePolicy::build_conf_for_model(base_model, &effective_model);

        Self {
            id: session.context.session_id.clone(),
            workspace_id: session.context.workspace_id.clone(),
            title: session.title.clone(),
            user_id: session.user_id.clone(),
            context: TalosContext {
                source_type: session.context.source_type.clone(),
                provider: session.context.provider.clone(),
                user_id: session.context.user_id.clone(),
                message_id: session.context.message_id.clone(),
                trace_id: session.context.trace_id.clone(),
                channel_id: session.context.channel_id.clone(),
                thread_id: session.context.thread_id.clone(),
                project_root: session.context.project_root.clone(),
                environment: session.context.environment.clone(),
            },
            model: effective_model,
            created_at: session.state.created_at,
            last_activity: session.state.last_activity,
            message_count: session.state.message_count,
            usage: session.state.usage.clone(),
            conf,
            project_id: session.project_id.clone(),
            current_directory: session.current_directory.clone(),
        }
    }

    pub fn has_real_title(&self) -> bool {
        let normalized = self.title.trim();
        !normalized.is_empty() && normalized != DEFAULT_SESSION_TITLE
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionQuery {
    pub user_id: Option<String>,
    pub channel_id: Option<String>,
    pub provider: Option<String>,
    pub project_id: Option<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionPatchAck {
    pub success: bool,
}
