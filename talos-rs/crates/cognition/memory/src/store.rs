use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use talos_config::{AgentKernelConfig, TalosTimezone};
use talos_llm_provider::{ChatMessage, LlmProvider};
use talos_types::talos::{StopReason, TalosOutputItem, TalosUsage};

use crate::path_policy;

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
    pub arguments: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultRecord {
    pub header: RecordHeader,
    pub call_id: String,
    pub tool: String,
    #[serde(default)]
    pub payload: Value,
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
    pub value: Value,
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
    Confirmation {
        payload: SessionTranscriptConfirmationPayload,
    },
    ApprovalResponse {
        payload: SessionTranscriptApprovalResponsePayload,
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
pub struct SessionTranscriptConfirmationPayload {
    pub tool_name: String,
    pub call_id: String,
    pub prompt: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionTranscriptApprovalResponsePayload {
    pub call_id: String,
    pub tool_name: String,
    pub approved: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionTranscriptDynamicToolRequestPayload {
    pub call_id: String,
    pub tool_name: String,
    pub arguments: serde_json::Value,
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionPromptContext {
    #[serde(default)]
    pub summaries: Vec<SummaryRecord>,
    #[serde(default)]
    pub recent_messages: Vec<TalosMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionPromptContextConfig {
    pub recent_messages_limit: usize,
    pub include_summary: bool,
    pub summary_recent_count: usize,
    pub summary_max_chars: usize,
}

impl Default for SessionPromptContextConfig {
    fn default() -> Self {
        Self {
            recent_messages_limit: 24,
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
    pub sections: Vec<MemoryIndexSection>,
}

#[derive(Clone)]
pub struct MemoryStore {
    pub(crate) workspace_dir: PathBuf,
    pub(crate) timezone: TalosTimezone,
    pub(crate) llm: Option<Arc<dyn LlmProvider>>,
    pub(crate) hot_recent_messages: usize,
    pub(crate) hot_cache: Arc<tokio::sync::RwLock<HashMap<String, VecDeque<TalosMessage>>>>,
    pub(crate) transcript_cache:
        Arc<tokio::sync::RwLock<HashMap<String, VecDeque<SessionTranscriptRecord>>>>,
    pub(crate) seq_cache: Arc<tokio::sync::RwLock<HashMap<String, u64>>>,
}

impl std::fmt::Debug for MemoryStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryStore")
            .field("workspace_dir", &self.workspace_dir)
            .field("timezone", &self.timezone.to_string())
            .field("hot_recent_messages", &self.hot_recent_messages)
            .field("has_llm", &self.llm.is_some())
            .finish()
    }
}

impl MemoryStore {
    pub fn new(workspace_dir: impl Into<PathBuf>) -> Self {
        Self::new_internal(workspace_dir, chrono_tz::UTC, None, 64)
    }

    pub fn new_with_llm(workspace_dir: impl Into<PathBuf>, llm: Arc<dyn LlmProvider>) -> Self {
        Self::new_internal(workspace_dir, chrono_tz::UTC, Some(llm), 64)
    }

    fn new_internal(
        workspace_dir: impl Into<PathBuf>,
        timezone: TalosTimezone,
        llm: Option<Arc<dyn LlmProvider>>,
        hot_recent_messages: usize,
    ) -> Self {
        let workspace_dir = workspace_dir.into();
        Self {
            workspace_dir: match workspace_dir.to_str() {
                Some(path) => Self::resolve_workspace_dir(path),
                None => workspace_dir,
            },
            timezone,
            llm,
            hot_recent_messages,
            hot_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            transcript_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            seq_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        }
    }

    pub fn from_config(config: &AgentKernelConfig) -> Self {
        Self::new_internal(
            Self::resolve_workspace_dir(&config.memory.workspace_dir),
            talos_config::parse_timezone(&config.timezone)
                .expect("agent kernel timezone should be validated before memory store init"),
            None,
            config.memory.hot_recent_messages.max(1),
        )
    }

    pub fn from_config_with_llm(config: &AgentKernelConfig, llm: Arc<dyn LlmProvider>) -> Self {
        Self::new_internal(
            Self::resolve_workspace_dir(&config.memory.workspace_dir),
            talos_config::parse_timezone(&config.timezone)
                .expect("agent kernel timezone should be validated before memory store init"),
            Some(llm),
            config.memory.hot_recent_messages.max(1),
        )
    }

    pub fn new_default() -> Self {
        Self::new(Self::default_workspace_dir())
    }

    pub async fn load_session_turns(
        &self,
        session_id: &str,
    ) -> talos_error::TalosResult<Vec<TalosTurn>> {
        crate::session_store::load_turns(self, session_id).await
    }

    pub fn default_workspace_dir() -> PathBuf {
        path_policy::default_workspace_dir()
    }

    pub fn workspace_dir(&self) -> &Path {
        &self.workspace_dir
    }

    pub(crate) fn timezone(&self) -> TalosTimezone {
        self.timezone
    }

    pub(crate) fn resolve_workspace_dir(path: &str) -> PathBuf {
        path_policy::resolve_workspace_dir(path)
    }

    pub(crate) fn workspace_memory_dir(&self) -> PathBuf {
        path_policy::workspace_memory_dir(self)
    }

    pub(crate) fn workspace_memory_file_for(&self, timestamp: DateTime<Utc>) -> PathBuf {
        path_policy::workspace_memory_file_for(self, timestamp)
    }

    pub fn llm_provider(&self) -> Option<Arc<dyn LlmProvider>> {
        self.llm.clone()
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use talos_error::TalosError;
    use tempfile::TempDir;
    use tokio::fs;

    use crate::Memory;

    use super::{
        MemorySearchQuery, MemoryStore, RecordHeader, SessionPromptContextConfig,
        SessionTranscriptActions, SessionTranscriptAuthor, SessionTranscriptBranch,
        SessionTranscriptEventKind, SessionTranscriptHeader, SessionTranscriptPayload,
        SessionTranscriptRecord, SummaryRecord, TalosTurn, WorkspaceMemoryRecord,
    };
    use crate::TalosMessageRole;
    use talos_types::talos::{StopReason, TalosContentItem, TalosOutputItem, TalosUsage};

    fn new_temp_store() -> (TempDir, MemoryStore) {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let store = MemoryStore::new(temp_dir.path().to_path_buf());
        (temp_dir, store)
    }

    #[tokio::test]
    async fn test_turn_roundtrip_and_context_dedup() {
        let (_guard, store) = new_temp_store();
        let session_id = "s1";
        let seq1 = store
            .next_turn_seq(session_id)
            .await
            .expect("seq should be 1");
        assert_eq!(seq1, 1);

        let turn_1 = TalosTurn {
            turn_id: "t1".to_string(),
            session_id: session_id.to_string(),
            seq: seq1,
            timestamp: Utc::now(),
            items: vec![
                TalosOutputItem::UserInput {
                    text: "hello".to_string(),
                },
                TalosOutputItem::OutputText {
                    text: "world".to_string(),
                },
            ],
            usage: TalosUsage::default(),
            stop_reason: StopReason::EndTurn,
        };

        store
            .append_turn(&turn_1)
            .await
            .expect("append should succeed");

        let seq2 = store
            .next_turn_seq(session_id)
            .await
            .expect("seq should be 2");
        assert_eq!(seq2, 2);

        let context = store
            .build_session_prompt_context(
                session_id,
                &SessionPromptContextConfig {
                    recent_messages_limit: 8,
                    include_summary: false,
                    summary_recent_count: 0,
                    summary_max_chars: 0,
                },
            )
            .await
            .expect("context should build");
        assert_eq!(context.recent_messages.len(), 2);
    }

    #[tokio::test]
    async fn test_turn_schema_items_roundtrip() {
        let (_guard, store) = new_temp_store();
        let session_id = "roundtrip-items";
        let turn = TalosTurn {
            turn_id: "turn-items-1".to_string(),
            session_id: session_id.to_string(),
            seq: store
                .next_turn_seq(session_id)
                .await
                .expect("seq should load"),
            timestamp: Utc::now(),
            items: vec![
                TalosOutputItem::UserInput {
                    text: "user".to_string(),
                },
                TalosOutputItem::Reasoning {
                    text: "think".to_string(),
                },
                TalosOutputItem::ToolCall {
                    call_id: "call-1".to_string(),
                    name: "read_file".to_string(),
                    arguments: serde_json::json!({"path":"/tmp/a.txt"}),
                },
                TalosOutputItem::ToolResult {
                    call_id: "call-1".to_string(),
                    tool_name: "read_file".to_string(),
                    content: vec![TalosContentItem::Text {
                        text: "contents".to_string(),
                    }],
                    is_error: false,
                },
                TalosOutputItem::OutputText {
                    text: "first answer".to_string(),
                },
                TalosOutputItem::OutputText {
                    text: "second answer".to_string(),
                },
            ],
            usage: TalosUsage::default(),
            stop_reason: StopReason::EndTurn,
        };

        store
            .append_turn(&turn)
            .await
            .expect("append turn should succeed");

        let turns = store
            .load_session_turns(session_id)
            .await
            .expect("turns should load");

        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].items.len(), 6);
        assert!(matches!(
            turns[0].items.first(),
            Some(TalosOutputItem::UserInput { text }) if text == "user"
        ));
        assert!(matches!(
            turns[0].items.get(1),
            Some(TalosOutputItem::Reasoning { text }) if text == "think"
        ));
        assert!(matches!(
            turns[0].items.get(2),
            Some(TalosOutputItem::ToolCall { name, .. }) if name == "read_file"
        ));
        assert!(matches!(
            turns[0].items.get(3),
            Some(TalosOutputItem::ToolResult { tool_name, .. }) if tool_name == "read_file"
        ));
        assert!(matches!(
            turns[0].items.get(4),
            Some(TalosOutputItem::OutputText { text }) if text == "first answer"
        ));
        assert!(matches!(
            turns[0].items.get(5),
            Some(TalosOutputItem::OutputText { text }) if text == "second answer"
        ));
    }

    #[tokio::test]
    async fn test_summary_records() {
        let (_guard, store) = new_temp_store();
        let record = SummaryRecord {
            version: 2,
            header: RecordHeader {
                id: "sum-1".to_string(),
                session_id: "s1".to_string(),
                turn_id: None,
                timestamp: Utc::now(),
            },
            covers_until_seq: 9,
            source_turn_count: 12,
            content: "facts".to_string(),
        };

        store
            .save_summary(&record)
            .await
            .expect("save summary should succeed");
        let loaded = store
            .load_summaries("s1", 4)
            .await
            .expect("load summary should succeed");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].header.id, "sum-1");
    }

    #[tokio::test]
    async fn test_transcript_records_roundtrip_from_jsonl_artifacts() {
        let (_guard, store) = new_temp_store();
        let timestamp = Utc::now();
        let record = SessionTranscriptRecord {
            header: SessionTranscriptHeader {
                id: "transcript-1".to_string(),
                session_id: "s1".to_string(),
                invocation_id: "inv-1".to_string(),
                branch: SessionTranscriptBranch::ModelThinking,
                timestamp,
            },
            author: SessionTranscriptAuthor::Agent,
            kind: SessionTranscriptEventKind::AgentMessage,
            payload: SessionTranscriptPayload::StateOnly,
            actions: SessionTranscriptActions::default(),
            partial: false,
        };

        store
            .append_transcript_record(&record)
            .await
            .expect("append transcript should succeed");

        let loaded = store
            .load_transcript_records("s1", Some("inv-1"), SessionTranscriptBranch::ModelThinking)
            .await
            .expect("load transcript should succeed");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].header.id, "transcript-1");

        let session_files = std::fs::read_dir(_guard.path().join("sessions").join("s1"))
            .expect("session dir should exist")
            .map(|entry| entry.expect("dir entry should load").path())
            .collect::<Vec<_>>();
        assert_eq!(session_files.len(), 1);
        assert_eq!(
            session_files[0].extension().and_then(|ext| ext.to_str()),
            Some("jsonl")
        );
    }

    #[tokio::test]
    async fn test_build_prompt_context_includes_summary() {
        let (_guard, store) = new_temp_store();
        let session_id = "s1";
        let timestamp = Utc::now();

        store
            .append_turn(&TalosTurn {
                turn_id: "turn-1".to_string(),
                session_id: session_id.to_string(),
                seq: store
                    .next_turn_seq(session_id)
                    .await
                    .expect("seq should load"),
                timestamp,
                items: vec![TalosOutputItem::UserInput {
                    text: "remember this".to_string(),
                }],
                usage: TalosUsage::default(),
                stop_reason: StopReason::EndTurn,
            })
            .await
            .expect("append turn should succeed");

        store
            .save_summary(&SummaryRecord {
                version: 2,
                header: RecordHeader {
                    id: "summary-1".to_string(),
                    session_id: session_id.to_string(),
                    turn_id: None,
                    timestamp,
                },
                covers_until_seq: 1,
                source_turn_count: 1,
                content: "facts".to_string(),
            })
            .await
            .expect("save summary should succeed");

        let context = store
            .build_session_prompt_context(
                session_id,
                &SessionPromptContextConfig {
                    recent_messages_limit: 10,
                    include_summary: true,
                    summary_recent_count: 4,
                    summary_max_chars: 0,
                },
            )
            .await
            .expect("context should load");
        assert_eq!(context.recent_messages.len(), 1);
        assert_eq!(context.recent_messages[0].text, "remember this");
        assert_eq!(context.summaries.len(), 1);
        assert_eq!(context.summaries[0].header.id, "summary-1");
    }

    #[tokio::test]
    async fn test_prompt_context_still_builds_after_schema_change() {
        let (_guard, store) = new_temp_store();
        let session_id = "prompt-context-items";
        let timestamp = Utc::now();

        store
            .append_turn(&TalosTurn {
                turn_id: "turn-items-context".to_string(),
                session_id: session_id.to_string(),
                seq: store
                    .next_turn_seq(session_id)
                    .await
                    .expect("seq should load"),
                timestamp,
                items: vec![
                    TalosOutputItem::UserInput {
                        text: "hello".to_string(),
                    },
                    TalosOutputItem::Reasoning {
                        text: "internal".to_string(),
                    },
                    TalosOutputItem::ToolCall {
                        call_id: "call-context".to_string(),
                        name: "read_file".to_string(),
                        arguments: serde_json::json!({"path":"/tmp/example.txt"}),
                    },
                    TalosOutputItem::ToolResult {
                        call_id: "call-context".to_string(),
                        tool_name: "read_file".to_string(),
                        content: vec![TalosContentItem::Text {
                            text: "result".to_string(),
                        }],
                        is_error: false,
                    },
                    TalosOutputItem::OutputText {
                        text: "world".to_string(),
                    },
                ],
                usage: TalosUsage::default(),
                stop_reason: StopReason::EndTurn,
            })
            .await
            .expect("append turn should succeed");

        let context = store
            .build_session_prompt_context(
                session_id,
                &SessionPromptContextConfig {
                    recent_messages_limit: 10,
                    include_summary: false,
                    summary_recent_count: 0,
                    summary_max_chars: 0,
                },
            )
            .await
            .expect("context should build");

        assert_eq!(context.recent_messages.len(), 2);
        assert_eq!(context.recent_messages[0].text, "hello");
        assert_eq!(context.recent_messages[1].text, "world");
        assert!(context.recent_messages.iter().all(|message| {
            matches!(
                message.role,
                TalosMessageRole::User | TalosMessageRole::Assistant
            )
        }));
    }

    #[tokio::test]
    async fn test_session_size_bytes_after_append_and_summary() {
        let (_guard, store) = new_temp_store();
        let session_id = "s-size";
        let timestamp = Utc::now();

        store
            .append_turn(&TalosTurn {
                turn_id: "turn-size".to_string(),
                session_id: session_id.to_string(),
                seq: store
                    .next_turn_seq(session_id)
                    .await
                    .expect("seq should load"),
                timestamp,
                items: vec![TalosOutputItem::UserInput {
                    text: "payload".to_string(),
                }],
                usage: TalosUsage::default(),
                stop_reason: StopReason::EndTurn,
            })
            .await
            .expect("append turn should succeed");

        store
            .save_summary(&SummaryRecord {
                version: 2,
                header: RecordHeader {
                    id: "summary-size".to_string(),
                    session_id: session_id.to_string(),
                    turn_id: None,
                    timestamp,
                },
                covers_until_seq: 1,
                source_turn_count: 1,
                content: "summary".to_string(),
            })
            .await
            .expect("save summary should succeed");

        let size = store
            .session_size_bytes(session_id)
            .await
            .expect("size should compute");
        assert!(size > 0);
    }

    #[tokio::test]
    async fn test_latest_seq_tracks_appended_turns() {
        let (_guard, store) = new_temp_store();
        let session_id = "s-seq";
        let timestamp = Utc::now();

        for turn_id in 1..=2 {
            store
                .append_turn(&TalosTurn {
                    turn_id: format!("turn-{}", turn_id),
                    session_id: session_id.to_string(),
                    seq: store
                        .next_turn_seq(session_id)
                        .await
                        .expect("seq should load"),
                    timestamp,
                    items: Vec::new(),
                    usage: TalosUsage::default(),
                    stop_reason: StopReason::EndTurn,
                })
                .await
                .expect("append turn should succeed");
        }

        assert_eq!(
            store
                .latest_seq(session_id)
                .await
                .expect("latest seq should load"),
            2
        );
    }

    #[tokio::test]
    async fn test_root_memory_and_workspace_memory_search() {
        let (_guard, store) = new_temp_store();
        store
            .append_workspace_memory(&WorkspaceMemoryRecord {
                timestamp: Utc::now(),
                session_id: "session-1".to_string(),
                title: "Daily".to_string(),
                content: "**Facts:**\n- deployment decision: blue/green".to_string(),
            })
            .await
            .expect("workspace memory append should succeed");

        let _error = store
            .search_memory(&MemorySearchQuery {
                query: "deployment".to_string(),
                max_results: 5,
            })
            .await
            .expect_err("memory search should require llm");
    }

    #[tokio::test]
    async fn test_get_memory_rejects_path_escape() {
        let (_guard, store) = new_temp_store();
        let result = store.get_memory("../secrets.txt", Some(1), Some(5)).await;
        assert!(matches!(result, Err(TalosError::ValidationError(_))));
    }

    #[tokio::test]
    async fn test_append_workspace_memory_creates_daily_file_on_first_write() {
        let (_guard, store) = new_temp_store();
        let timestamp = Utc::now();
        let record = WorkspaceMemoryRecord {
            timestamp,
            session_id: "session-1".to_string(),
            title: "Daily Memory".to_string(),
            content: "**Facts:**\n- first memory entry".to_string(),
        };

        store
            .append_workspace_memory(&record)
            .await
            .expect("first append should succeed");

        let file_path = store.workspace_memory_file_for(timestamp);
        let content = fs::read_to_string(file_path)
            .await
            .expect("workspace memory file should exist after first append");

        assert!(content.contains("first memory entry"));
        assert!(content.contains("session: session-1"));
    }

    #[tokio::test]
    async fn test_append_workspace_memory_dedupes_duplicate_records() {
        let (_guard, store) = new_temp_store();
        let record = WorkspaceMemoryRecord {
            timestamp: Utc::now(),
            session_id: "session-1".to_string(),
            title: "Durable Memory".to_string(),
            content: "**Facts:**\n- user prefers tea over coffee".to_string(),
        };

        store
            .append_workspace_memory(&record)
            .await
            .expect("first append should succeed");
        store
            .append_workspace_memory(&record)
            .await
            .expect("second append should dedupe");

        let file_path = store.workspace_memory_file_for(record.timestamp);
        let content = fs::read_to_string(file_path)
            .await
            .expect("workspace memory file should exist");
        assert_eq!(content.matches("session: session-1").count(), 1);
    }

    #[test]
    fn test_memory_store_new_expands_tilde_workspace_dir() {
        let store = MemoryStore::new("~/talos-memory-test");
        assert_eq!(
            store.workspace_dir(),
            talos_config::resolve_runtime_path("~/talos-memory-test")
        );
    }

    #[tokio::test]
    async fn test_search_memory_reflects_user_file_edits() {
        let (_guard, store) = new_temp_store();
        let timestamp = Utc::now();
        store
            .append_workspace_memory(&WorkspaceMemoryRecord {
                timestamp,
                session_id: "session-1".to_string(),
                title: "Durable Memory".to_string(),
                content: "**Facts:**\n- deployment decision: blue green".to_string(),
            })
            .await
            .expect("workspace memory append should succeed");

        let file_path = store.workspace_memory_file_for(timestamp);
        fs::write(
            &file_path,
            "## Durable Memory 10:00\ndeployment decision: canary\n\n",
        )
        .await
        .expect("user edit should succeed");

        let _error = store
            .search_memory(&MemorySearchQuery {
                query: "canary".to_string(),
                max_results: 5,
            })
            .await
            .expect_err("search should require llm");
    }
}
