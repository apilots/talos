use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use talos_types::{
    talos::{SessionSourceProvider, SessionSourceType, TalosSessionEnvironment, TokenUsage},
    AgentPath, AgentTurnRef, SessionConf, SessionUsage, DEFAULT_SESSION_TITLE,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionRecord {
    pub id: String,
    pub agent_id: String,
    pub execution_node_id: String,
    pub tree_identity: SessionTreeIdentity,
    pub workspace_id: String,
    pub title: String,
    pub user_id: String,
    pub source_type: SessionSourceType,
    pub source_provider: SessionSourceProvider,
    pub channel_id: Option<String>,
    pub thread_id: Option<String>,
    pub environment: Option<TalosSessionEnvironment>,
    pub created_at: u64,
    pub last_activity: u64,
    pub message_count: u64,
    #[serde(default)]
    pub latest_turn_seq: u64,
    #[serde(default)]
    pub memory_processed_until_event_seq: u64,
    #[serde(default)]
    pub usage: SessionUsage,
    #[serde(default)]
    pub conf: SessionConf,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub current_directory: Option<PathBuf>,
}

/// Exhaustive persisted identity of a Primary or Subagent Session.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionTreeIdentity {
    #[default]
    Primary,
    Subagent {
        parent: AgentTurnRef,
        tree_root_session_id: String,
        path: AgentPath,
    },
}

impl SessionRecord {
    pub fn has_real_title(&self) -> bool {
        let normalized = self.title.trim();
        !normalized.is_empty() && normalized != DEFAULT_SESSION_TITLE
    }
}

/// Selects the persisted Session tree relation for one Store query.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum SessionTreeScope {
    Primary,
    SubagentTree {
        tree_root_session_id: String,
    },
    #[default]
    All,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionQuery {
    pub execution_node_id: Option<String>,
    pub tree_scope: SessionTreeScope,
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

/// Represents current user-scoped Session counts for higher-level read models.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionSummary {
    pub session_count: u64,
    pub message_count: u64,
}

/// Store-owned columns replaced by one normalized Session settings patch.
pub struct PatchSessionParams {
    pub session_id: String,
    pub conf: SessionConf,
    pub current_directory: Option<PathBuf>,
    pub reset_context_usage: bool,
    pub last_activity: u64,
}

/// Token delta accumulated into the latest persisted Session usage.
pub struct AccumulateSessionUsageParams {
    pub session_id: String,
    pub tokens: TokenUsage,
    pub last_activity: u64,
}
