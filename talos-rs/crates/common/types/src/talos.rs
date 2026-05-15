use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;
use ts_rs::TS;
use uuid::Uuid;

/// Talos gateway protocol types shared by server and client components.

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TalosConnectionMode {
    Embedded,
    #[default]
    Remote,
}

impl fmt::Display for TalosConnectionMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::Embedded => "embedded",
            Self::Remote => "remote",
        };
        write!(f, "{}", label)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    #[default]
    EndTurn,
    ModelOutputLimit,
    ToolRoundLimit,
    TransferUnresolved,
    BeforeModelCallbackStopped,
    AfterModelCallbackStopped,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct TalosJsonValue {
    #[schemars(schema_with = "crate::schema::json_value_schema")]
    #[ts(type = "unknown")]
    value: serde_json::Value,
}

impl TalosJsonValue {
    pub fn new(value: serde_json::Value) -> Self {
        Self { value }
    }

    pub fn into_inner(self) -> serde_json::Value {
        self.value
    }

    pub fn as_inner(&self) -> &serde_json::Value {
        &self.value
    }
}

impl Default for TalosJsonValue {
    fn default() -> Self {
        Self::new(serde_json::Value::Null)
    }
}

impl From<serde_json::Value> for TalosJsonValue {
    fn from(value: serde_json::Value) -> Self {
        Self::new(value)
    }
}

impl From<TalosJsonValue> for serde_json::Value {
    fn from(value: TalosJsonValue) -> Self {
        value.into_inner()
    }
}

impl std::fmt::Display for TalosJsonValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TalosSessionEnvironment {
    pub directory: PathBuf,
    #[serde(default)]
    pub connection_mode: TalosConnectionMode,
}

/// High-level source category.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TalosSourceType {
    Http,
    Desktop,
    Im,
    Cli,
    Ws,
    System,
}

impl Default for TalosSourceType {
    fn default() -> Self {
        Self::System
    }
}

impl TalosSourceType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Desktop => "desktop",
            Self::Im => "im",
            Self::Cli => "cli",
            Self::Ws => "ws",
            Self::System => "system",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "http" => Self::Http,
            "desktop" => Self::Desktop,
            "im" => Self::Im,
            "cli" => Self::Cli,
            "ws" => Self::Ws,
            _ => Self::System,
        }
    }
}

impl fmt::Display for TalosSourceType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Concrete source provider.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TalosProvider {
    Http,
    Sdk,
    Cli,
    Telegram,
    WebSocket,
    System,
}

impl Default for TalosProvider {
    fn default() -> Self {
        Self::System
    }
}

impl TalosProvider {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Sdk => "sdk",
            Self::Cli => "cli",
            Self::Telegram => "telegram",
            Self::WebSocket => "websocket",
            Self::System => "system",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "http" => Self::Http,
            "sdk" => Self::Sdk,
            "cli" => Self::Cli,
            "telegram" => Self::Telegram,
            "websocket" => Self::WebSocket,
            _ => Self::System,
        }
    }
}

impl fmt::Display for TalosProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Cross-channel request context.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TalosContext {
    pub source_type: TalosSourceType,
    pub provider: TalosProvider,
    pub user_id: String,
    pub message_id: String,
    pub trace_id: String,
    #[serde(default)]
    pub channel_id: Option<String>,
    #[serde(default)]
    pub thread_id: Option<String>,
    #[serde(default)]
    pub project_root: Option<PathBuf>,
    #[serde(default)]
    pub environment: Option<TalosSessionEnvironment>,
}

impl TalosContext {
    pub fn new(
        source_type: TalosSourceType,
        provider: TalosProvider,
        user_id: impl Into<String>,
        message_id: impl Into<String>,
        trace_id: impl Into<String>,
    ) -> Self {
        Self {
            source_type,
            provider,
            user_id: user_id.into(),
            message_id: message_id.into(),
            trace_id: trace_id.into(),
            channel_id: None,
            thread_id: None,
            project_root: None,
            environment: None,
        }
    }

    pub fn with_channel_id(mut self, channel_id: impl Into<String>) -> Self {
        self.channel_id = Some(channel_id.into());
        self
    }

    pub fn with_thread_id(mut self, thread_id: impl Into<String>) -> Self {
        self.thread_id = Some(thread_id.into());
        self
    }

    pub fn with_project_root(mut self, project_root: impl Into<PathBuf>) -> Self {
        self.project_root = Some(project_root.into());
        self
    }

    pub fn with_environment(mut self, environment: TalosSessionEnvironment) -> Self {
        self.environment = Some(environment);
        self
    }

    pub fn http(user_id: impl Into<String>) -> Self {
        Self::generated(TalosSourceType::Http, TalosProvider::Http, user_id)
    }

    pub fn sdk(user_id: impl Into<String>) -> Self {
        Self::generated(TalosSourceType::Http, TalosProvider::Sdk, user_id)
    }

    pub fn desktop(user_id: impl Into<String>) -> Self {
        Self::generated(TalosSourceType::Desktop, TalosProvider::Sdk, user_id)
    }

    pub fn cli(user_id: impl Into<String>) -> Self {
        Self::generated(TalosSourceType::Cli, TalosProvider::Cli, user_id)
    }

    pub fn ws(user_id: impl Into<String>) -> Self {
        Self::generated(TalosSourceType::Ws, TalosProvider::WebSocket, user_id)
    }

    pub fn im(provider: TalosProvider, user_id: impl Into<String>) -> Self {
        Self::generated(TalosSourceType::Im, provider, user_id)
    }

    pub fn system(user_id: impl Into<String>) -> Self {
        Self::generated(TalosSourceType::System, TalosProvider::System, user_id)
    }

    fn generated(
        source_type: TalosSourceType,
        provider: TalosProvider,
        user_id: impl Into<String>,
    ) -> Self {
        Self::new(
            source_type,
            provider,
            user_id,
            format!("msg_{}", Uuid::new_v4().simple()),
            format!("trace_{}", Uuid::new_v4().simple()),
        )
    }
}

/// Talos message input item.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TalosInputItem {
    Text {
        text: String,
    },
    Image {
        url: String,
        #[serde(default)]
        mime_type: Option<String>,
    },
    File {
        #[serde(default)]
        url: Option<String>,
        #[serde(default)]
        file_id: Option<String>,
        #[serde(default)]
        mime_type: Option<String>,
    },
    Voice {
        #[serde(default)]
        url: Option<String>,
        #[serde(default)]
        data_base64: Option<String>,
        #[serde(default)]
        mime_type: Option<String>,
    },
    Video {
        #[serde(default)]
        url: Option<String>,
        #[serde(default)]
        mime_type: Option<String>,
    },
}

/// Talos content item for rich output blocks.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TalosContentItem {
    Text { text: String },
    Json { value: TalosJsonValue },
}

/// Talos message output item.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TalosOutputItem {
    UserInput {
        text: String,
    },
    Reasoning {
        text: String,
    },
    ExecutionSummary {
        text: String,
    },
    OutputText {
        text: String,
    },
    ToolCall {
        call_id: String,
        name: String,
        arguments: TalosJsonValue,
    },
    ToolResult {
        call_id: String,
        tool_name: String,
        content: Vec<TalosContentItem>,
        #[serde(default)]
        is_error: bool,
    },
}

/// Reasoning block exposed to clients.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TalosReasoning {
    pub text: String,
}

/// Talos message request payload for session interactions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TalosMessageRequest {
    pub input: Vec<TalosInputItem>,
    pub workspace_id: Option<String>,
}

/// Token usage values.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
pub struct TalosUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub total_tokens: u32,
    #[serde(default)]
    pub cache_creation_input_tokens: Option<u32>,
    #[serde(default)]
    pub cache_read_input_tokens: Option<u32>,
}

impl TalosUsage {
    pub fn accumulate(&mut self, other: &Self) {
        self.input_tokens = self.input_tokens.saturating_add(other.input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(other.output_tokens);
        self.total_tokens = self.total_tokens.saturating_add(other.total_tokens);
        self.cache_creation_input_tokens = sum_optional_tokens(
            self.cache_creation_input_tokens,
            other.cache_creation_input_tokens,
        );
        self.cache_read_input_tokens =
            sum_optional_tokens(self.cache_read_input_tokens, other.cache_read_input_tokens);
    }
}

fn sum_optional_tokens(left: Option<u32>, right: Option<u32>) -> Option<u32> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.saturating_add(right)),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

/// Talos response shape returned by session interaction APIs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TalosMessageResponse {
    pub request_id: String,
    pub message_id: String,
    pub session_id: String,
    pub model: String,
    pub output: Vec<TalosOutputItem>,
    #[serde(default)]
    pub reasoning: Vec<TalosReasoning>,
    pub stop_reason: StopReason,
    pub usage: TalosUsage,
}

/// Timeline stream event lifecycle type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum TalosTimelineEventType {
    TurnStarted,
    ItemStarted,
    ItemDelta,
    ItemCompleted,
    TurnCompleted,
    TurnError,
}

/// Timeline item kind rendered by clients.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum TalosTimelineItemKind {
    UserInput,
    Reasoning,
    AssistantText,
    ToolCall,
    ExecutionSummary,
    SystemNotice,
}

/// Server-owned phase for a timeline item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum TalosTimelinePhase {
    Input,
    Thinking,
    Acting,
    Commentary,
    FinalAnswer,
    System,
}

/// Lifecycle status for a timeline item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum TalosTimelineItemStatus {
    Running,
    WaitingApproval,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum TalosTimelineNoticeLevel {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct TalosToolProgressEntry {
    pub progress_kind: TalosToolProgressKind,
    pub text: String,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct TalosToolResult {
    pub content: Vec<TalosContentItem>,
    pub is_error: bool,
}

/// Current full content for a timeline item.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TalosTimelineContent {
    Empty,
    Text {
        text: String,
    },
    ToolCall {
        call_id: String,
        name: String,
        arguments: String,
        #[serde(default)]
        progress: Vec<TalosToolProgressEntry>,
        #[serde(default)]
        result: Option<TalosToolResult>,
    },
    ExecutionSummary {
        summary: String,
    },
    SystemNotice {
        level: TalosTimelineNoticeLevel,
        message: String,
    },
}

/// Incremental update for an existing timeline item.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TalosTimelineDelta {
    Text {
        text: String,
    },
    ToolArguments {
        arguments_delta: String,
    },
    ToolProgress {
        progress_kind: TalosToolProgressKind,
        text_delta: String,
    },
    ToolResult {
        content: Vec<TalosContentItem>,
        is_error: bool,
    },
    ToolApprovalRequest {
        call_id: String,
        tool_name: String,
        prompt: String,
        arguments: TalosJsonValue,
    },
    ToolDynamicRequest {
        call_id: String,
        tool_name: String,
        arguments: TalosJsonValue,
    },
    Status {
        status: TalosTimelineItemStatus,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct TalosTimelineItem {
    pub item_id: String,
    #[serde(default)]
    pub parent_item_id: Option<String>,
    pub kind: TalosTimelineItemKind,
    pub phase: TalosTimelinePhase,
    pub status: TalosTimelineItemStatus,
    #[serde(default)]
    pub title: Option<String>,
    pub started_at_ms: u64,
    #[serde(default)]
    pub completed_at_ms: Option<u64>,
    pub content: TalosTimelineContent,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct TalosTurnStartedEvent {
    pub request_id: String,
    pub turn_id: String,
    pub started_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct TalosItemStartedEvent {
    pub request_id: String,
    pub turn_id: String,
    pub item: TalosTimelineItem,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct TalosItemDeltaEvent {
    pub request_id: String,
    pub turn_id: String,
    pub item_id: String,
    pub delta: TalosTimelineDelta,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct TalosItemCompletedEvent {
    pub request_id: String,
    pub turn_id: String,
    pub item_id: String,
    pub status: TalosTimelineItemStatus,
    pub completed_at_ms: u64,
    #[serde(default)]
    pub final_content: Option<TalosTimelineContent>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct TalosTurnCompletedEvent {
    pub request_id: String,
    pub turn_id: String,
    pub completed_at_ms: u64,
    pub stop_reason: StopReason,
    pub usage: TalosUsage,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct TalosTurnErrorEvent {
    pub request_id: String,
    pub turn_id: String,
    pub error: String,
    pub failed_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TalosTimelineEventPayload {
    TurnStarted(TalosTurnStartedEvent),
    ItemStarted(TalosItemStartedEvent),
    ItemDelta(TalosItemDeltaEvent),
    ItemCompleted(TalosItemCompletedEvent),
    TurnCompleted(TalosTurnCompletedEvent),
    TurnError(TalosTurnErrorEvent),
}

/// Domain timeline event payload.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct TalosTimelineEvent {
    #[serde(default)]
    pub event_seq: Option<i64>,
    #[serde(default)]
    pub event_id: Option<String>,
    #[serde(default)]
    pub created_at_ms: Option<i64>,
    pub event_type: TalosTimelineEventType,
    pub turn_id: String,
    pub message_id: String,
    pub session_id: String,
    pub sequence: u64,
    pub payload: TalosTimelineEventPayload,
}

/// Public recoverable timeline stream event.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct TimelineEventEnvelope {
    pub event_id: String,
    pub event_seq: i64,
    pub created_at_ms: i64,
    pub session_id: String,
    pub turn_id: String,
    pub message_id: String,
    pub event: TalosTimelineEvent,
}

impl TimelineEventEnvelope {
    pub fn from_persisted_event(event: TalosTimelineEvent) -> Option<Self> {
        Some(Self {
            event_id: event.event_id.clone()?,
            event_seq: event.event_seq?,
            created_at_ms: event.created_at_ms?,
            session_id: event.session_id.clone(),
            turn_id: event.turn_id.clone(),
            message_id: event.message_id.clone(),
            event,
        })
    }
}

/// Stable session history snapshot. This is not a replay cursor envelope.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct SessionHistorySnapshot {
    pub session_id: String,
    pub messages: Vec<SessionHistoryMessage>,
    pub generated_at_ms: i64,
    #[serde(default)]
    pub last_stable_event_id: Option<String>,
}

/// Stable message view for session history.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct SessionHistoryMessage {
    pub message_id: String,
    pub turn_id: String,
    pub items: Vec<TalosTimelineItem>,
    pub stop_reason: StopReason,
    pub usage: TalosUsage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum TalosToolProgressKind {
    Status,
    Stdout,
    Stderr,
}
