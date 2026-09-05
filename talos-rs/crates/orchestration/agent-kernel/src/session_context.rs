use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};
use talos_error::TalosResult;
use talos_prompt::CompactPromptCursor;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionPromptMessageRole {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionPromptEntryHeader {
    pub id: String,
    pub turn_id: String,
    pub completed_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionPromptMessage {
    pub header: SessionPromptEntryHeader,
    pub role: SessionPromptMessageRole,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionToolResultContext {
    pub header: SessionPromptEntryHeader,
    pub tool_name: String,
    pub content: String,
    pub is_error: bool,
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionToolExchangeOutcome {
    Succeeded,
    Failed,
    Pending,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionToolExchangeContext {
    pub header: SessionPromptEntryHeader,
    pub call_id: String,
    pub tool_name: String,
    pub arguments_preview: String,
    pub outcome: SessionToolExchangeOutcome,
    pub output_preview: Option<String>,
    pub output_truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionPromptSummary {
    pub id: String,
    pub covers_until_seq: u64,
    pub source_turn_count: usize,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionPromptSourceTurn {
    pub turn_id: String,
    pub completed_event_seq: i64,
    pub completed_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct SessionPromptContext {
    pub summaries: Vec<SessionPromptSummary>,
    pub source_turns: Vec<SessionPromptSourceTurn>,
    pub recent_messages: Vec<SessionPromptMessage>,
    pub recent_tool_results: Vec<SessionToolResultContext>,
    pub recent_tool_exchanges: Vec<SessionToolExchangeContext>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionPromptRecentWindow {
    #[default]
    Latest,
    OldestUncovered,
}

#[derive(Debug, Clone)]
pub struct SessionPromptContextConfig {
    pub compact_cursor: Option<CompactPromptCursor>,
    pub recent_window: SessionPromptRecentWindow,
    pub recent_messages_limit: usize,
    pub recent_tool_results_limit: usize,
    pub tool_output_max_chars: usize,
    pub recent_tool_exchanges_limit: usize,
    pub tool_exchange_max_chars: usize,
    pub include_summary: bool,
    pub summary_recent_count: usize,
    pub summary_max_chars: usize,
}

pub type SessionPromptContextFuture<'a> =
    Pin<Box<dyn Future<Output = TalosResult<SessionPromptContext>> + Send + 'a>>;

/// Provider-owned source for one bounded Kernel prompt projection.
///
/// Implementations derive this projection from Talos canonical Session history
/// and Provider-private summaries. They must not expose Store records or create
/// another product history authority.
pub trait SessionPromptContextSource: Send + Sync {
    fn load(&self, config: SessionPromptContextConfig) -> SessionPromptContextFuture<'_>;
}
