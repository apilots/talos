use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use talos_types::talos::{TalosContext, TokenUsage};
use talos_types::{SessionConf, TalosSession, DEFAULT_SESSION_TITLE};

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
    pub latest_turn_seq: u64,
    #[serde(default)]
    pub memory_processed_until_seq: u64,
    #[serde(default)]
    pub usage: TokenUsage,
    #[serde(default)]
    pub conf: SessionConf,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub current_directory: Option<PathBuf>,
}

impl SessionRecord {
    pub fn from_session(session: &TalosSession) -> Self {
        let effective_model = session.config.llm.model.clone();

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
            latest_turn_seq: 0,
            memory_processed_until_seq: 0,
            usage: session.state.usage.clone(),
            conf: session.conf.clone(),
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
    pub memory_pending_only: bool,
    pub after: Option<SessionPageCursor>,
}

/// Stable keyset cursor for the session list order.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionPageCursor {
    pub last_activity: u64,
    pub session_id: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionPatchAck {
    pub success: bool,
}

/// Represents current user-scoped Session counts for higher-level read models.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionSummary {
    pub session_count: u64,
    pub message_count: u64,
}
