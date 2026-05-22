use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use talos_llm_provider::ChatMessage;
use talos_types::talos::{StopReason, TalosJsonValue, TalosOutputItem, TalosUsage};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TalosMessageRole {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecordHeader {
    pub id: String,
    pub session_id: String,
    pub turn_id: Option<String>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TalosMessage {
    pub header: RecordHeader,
    pub role: TalosMessageRole,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRecord {
    pub call_id: String,
    pub name: String,
    #[serde(default)]
    pub arguments: TalosJsonValue,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultRecord {
    pub header: RecordHeader,
    pub call_id: String,
    pub tool: String,
    #[serde(default)]
    pub payload: TalosJsonValue,
    #[serde(default)]
    pub is_error: bool,
    #[serde(default)]
    pub truncated: bool,
    #[serde(default)]
    pub size_bytes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TalosTurn {
    pub turn_id: String,
    #[serde(default)]
    pub message_id: String,
    pub session_id: String,
    pub seq: u64,
    pub timestamp: DateTime<Utc>,
    #[serde(default)]
    pub items: Vec<TalosOutputItem>,
    #[serde(default)]
    pub usage: TalosUsage,
    #[serde(default)]
    pub stop_reason: StopReason,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionTranscriptBranch {
    DirectAnswer,
    ReactiveLoop,
    ModelThinking,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionTranscriptAuthor {
    User,
    Agent,
    Tool,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionTranscriptEventKind {
    UserMessage,
    AgentMessage,
    ToolCall,
    ToolResult,
    SystemMessage,
    Planning,
    StateUpdate,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionTranscriptStateNamespace {
    App,
    User,
    Temp,
    Session,
    Workspace,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionTranscriptStateDelta {
    pub namespace: SessionTranscriptStateNamespace,
    pub key: String,
    #[serde(default)]
    pub value: TalosJsonValue,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionTranscriptActions {
    #[serde(default)]
    pub state_delta: Vec<SessionTranscriptStateDelta>,
    #[serde(default)]
    pub end_turn: bool,
    pub transfer_to: Option<String>,
    pub branch_override: Option<SessionTranscriptBranch>,
    pub rewind_before_invocation_id: Option<String>,
    pub compaction: Option<SessionTranscriptCompaction>,
}

impl Default for SessionTranscriptActions {
    fn default() -> Self {
        Self {
            state_delta: Vec::new(),
            end_turn: false,
            transfer_to: None,
            branch_override: None,
            rewind_before_invocation_id: None,
            compaction: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionTranscriptCompaction {
    pub start_event_id: String,
    pub end_event_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionTranscriptHeader {
    pub id: String,
    pub session_id: String,
    pub invocation_id: String,
    pub branch: SessionTranscriptBranch,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionTranscriptPayload {
    Message {
        message: ChatMessage,
    },
    StateOnly,
    Transfer {
        payload: SessionTranscriptTransferPayload,
    },
    Auth {
        payload: SessionTranscriptAuthPayload,
    },
    DynamicToolRequest {
        payload: SessionTranscriptDynamicToolRequestPayload,
    },
    DynamicToolResponse {
        payload: SessionTranscriptDynamicToolResponsePayload,
    },
    Compaction {
        payload: SessionTranscriptCompactionPayload,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionTranscriptTransferPayload {
    pub target_agent: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionTranscriptAuthPayload {
    pub provider: String,
    pub prompt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionTranscriptDynamicToolRequestPayload {
    pub call_id: String,
    pub tool_name: String,
    pub arguments: TalosJsonValue,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionTranscriptDynamicToolResponsePayload {
    pub call_id: String,
    pub tool_name: String,
    pub output: String,
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionTranscriptCompactionPayload {
    pub summary_message: ChatMessage,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionTranscriptRecord {
    pub header: SessionTranscriptHeader,
    pub author: SessionTranscriptAuthor,
    pub kind: SessionTranscriptEventKind,
    pub payload: SessionTranscriptPayload,
    #[serde(default)]
    pub actions: SessionTranscriptActions,
    #[serde(default)]
    pub partial: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummaryRecord {
    pub version: u8,
    pub header: RecordHeader,
    pub covers_until_seq: u64,
    pub source_turn_count: usize,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionToolResultContext {
    pub header: RecordHeader,
    pub tool_name: String,
    pub content: String,
    #[serde(default)]
    pub is_error: bool,
    #[serde(default)]
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
    pub header: RecordHeader,
    pub call_id: String,
    pub tool_name: String,
    pub arguments_preview: String,
    pub outcome: SessionToolExchangeOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_preview: Option<String>,
    #[serde(default)]
    pub output_truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionPromptContext {
    #[serde(default)]
    pub summaries: Vec<SummaryRecord>,
    #[serde(default)]
    pub recent_messages: Vec<TalosMessage>,
    #[serde(default)]
    pub recent_tool_results: Vec<SessionToolResultContext>,
    #[serde(default)]
    pub recent_tool_exchanges: Vec<SessionToolExchangeContext>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionPromptContextConfig {
    pub recent_messages_limit: usize,
    pub recent_tool_results_limit: usize,
    pub tool_output_max_chars: usize,
    pub recent_tool_exchanges_limit: usize,
    pub tool_exchange_max_chars: usize,
    pub include_summary: bool,
    pub summary_recent_count: usize,
    pub summary_max_chars: usize,
}

impl Default for SessionPromptContextConfig {
    fn default() -> Self {
        Self {
            recent_messages_limit: 24,
            recent_tool_results_limit: 8,
            tool_output_max_chars: 2_000,
            recent_tool_exchanges_limit: 8,
            tool_exchange_max_chars: 600,
            include_summary: true,
            summary_recent_count: 2,
            summary_max_chars: 1200,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySearchQuery {
    pub query: String,
    pub max_results: usize,
}

impl Default for MemorySearchQuery {
    fn default() -> Self {
        Self {
            query: String::new(),
            max_results: 6,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySearchResult {
    pub path: String,
    pub snippet: String,
    pub start_line: usize,
    pub end_line: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryGetResult {
    pub path: String,
    pub text: String,
    pub from: usize,
    pub lines: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RetrievedMemoryItem {
    pub path: String,
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceMemoryRecord {
    pub timestamp: DateTime<Utc>,
    pub session_id: String,
    pub title: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMemorySummary {
    pub title: String,
    #[serde(default)]
    pub facts: Vec<String>,
    #[serde(default)]
    pub preferences: Vec<String>,
    #[serde(default)]
    pub open_tasks: Vec<String>,
    #[serde(default)]
    pub risks: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryIndexSection {
    pub title: String,
    pub session_id: String,
    pub summary_preview: String,
    pub start_line: usize,
    pub end_line: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryIndexFile {
    pub path: String,
    pub date: String,
    #[serde(default)]
    pub source_content_hash: String,
    #[serde(default)]
    pub sections: Vec<MemoryIndexSection>,
}
