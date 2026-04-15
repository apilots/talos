use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;
use uuid::Uuid;

/// Talos gateway protocol types shared by native and compatibility APIs.

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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    #[default]
    EndTurn,
    ToolRoundLimit,
    TransferUnresolved,
    BeforeModelCallbackStopped,
    AfterModelCallbackStopped,
    Cancelled,
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
            Self::Im => "im",
            Self::Cli => "cli",
            Self::Ws => "ws",
            Self::System => "system",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "http" => Self::Http,
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
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TalosContentItem {
    Text { text: String },
    Json { value: serde_json::Value },
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
        arguments: serde_json::Value,
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
    #[serde(default)]
    pub stream: Option<bool>,
    pub workspace_id: Option<String>,
}

/// Token usage values.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TalosUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub total_tokens: u32,
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

/// Stream event type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TalosEventType {
    MessageStarted,
    MessageReasoningDelta,
    MessageExecutionSummary,
    MessageOutputDelta,
    MessageToolCallStarted,
    MessageToolCallArgsDelta,
    MessageToolCallQueued,
    MessageToolCallExecuting,
    MessageToolCallProgressDelta,
    MessageToolCallCompleted,
    MessageToolCallPendingApproval,
    MessageCompleted,
    MessageError,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TalosThinkingSource {
    Provider,
    Framework,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TalosThinkingStage {
    ModelReasoning,
    FrameworkPlan,
    FrameworkAct,
    FrameworkObserve,
    FrameworkReflect,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TalosMessageStartedEvent {
    pub request_id: String,
    pub model: String,
    pub usage: TalosUsage,
    pub iteration: u64,
    pub step_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TalosMessageReasoningDeltaEvent {
    pub request_id: String,
    pub delta: String,
    pub source: TalosThinkingSource,
    pub stage: TalosThinkingStage,
    pub item_index: u64,
    pub iteration: u64,
    pub step_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TalosMessageExecutionSummaryEvent {
    pub request_id: String,
    pub text: String,
    pub item_index: u64,
    pub iteration: u64,
    pub step_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TalosMessageOutputDeltaEvent {
    pub request_id: String,
    pub delta: String,
    pub item_index: u64,
    pub iteration: u64,
    pub step_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TalosMessageToolCallStartedEvent {
    pub request_id: String,
    pub call_id: String,
    pub name: String,
    pub arguments: serde_json::Value,
    pub item_index: u64,
    pub iteration: u64,
    pub step_id: String,
    pub timing_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TalosToolStreamKey {
    pub round: u32,
    pub index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TalosToolProgressKind {
    Status,
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TalosMessageToolCallArgsDeltaEvent {
    pub request_id: String,
    pub stream_key: TalosToolStreamKey,
    pub call_id: Option<String>,
    pub tool_name: Option<String>,
    pub delta: String,
    pub item_index: u64,
    pub iteration: u64,
    pub step_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TalosMessageToolCallQueuedEvent {
    pub request_id: String,
    pub stream_key: TalosToolStreamKey,
    pub call_id: String,
    pub tool_name: String,
    pub item_index: u64,
    pub iteration: u64,
    pub step_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TalosMessageToolCallExecutingEvent {
    pub request_id: String,
    pub stream_key: TalosToolStreamKey,
    pub call_id: String,
    pub tool_name: String,
    pub item_index: u64,
    pub iteration: u64,
    pub step_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TalosMessageToolCallProgressDeltaEvent {
    pub request_id: String,
    pub stream_key: TalosToolStreamKey,
    pub call_id: String,
    pub tool_name: String,
    pub message: String,
    pub progress_kind: TalosToolProgressKind,
    pub item_index: u64,
    pub iteration: u64,
    pub step_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TalosMessageToolCallCompletedEvent {
    pub request_id: String,
    pub call_id: String,
    pub tool: String,
    pub content: Vec<TalosContentItem>,
    pub is_error: bool,
    pub item_index: u64,
    pub iteration: u64,
    pub step_id: String,
    pub timing_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TalosMessageToolCallPendingApprovalEvent {
    pub request_id: String,
    pub call_id: String,
    pub tool_name: String,
    pub prompt: String,
    pub arguments: serde_json::Value,
    pub step_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TalosMessageCompletedEvent {
    pub request_id: String,
    pub stop_reason: StopReason,
    pub usage: TalosUsage,
    pub total_output_items: u64,
    pub iteration: u64,
    pub step_id: String,
    pub task_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TalosMessageErrorEvent {
    pub request_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TalosEventPayload {
    MessageStarted(TalosMessageStartedEvent),
    MessageReasoningDelta(TalosMessageReasoningDeltaEvent),
    MessageExecutionSummary(TalosMessageExecutionSummaryEvent),
    MessageOutputDelta(TalosMessageOutputDeltaEvent),
    MessageToolCallStarted(TalosMessageToolCallStartedEvent),
    MessageToolCallArgsDelta(TalosMessageToolCallArgsDeltaEvent),
    MessageToolCallQueued(TalosMessageToolCallQueuedEvent),
    MessageToolCallExecuting(TalosMessageToolCallExecutingEvent),
    MessageToolCallProgressDelta(TalosMessageToolCallProgressDeltaEvent),
    MessageToolCallCompleted(TalosMessageToolCallCompletedEvent),
    MessageToolCallPendingApproval(TalosMessageToolCallPendingApprovalEvent),
    MessageCompleted(TalosMessageCompletedEvent),
    MessageError(TalosMessageErrorEvent),
}

/// Canonical stream event payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TalosEvent {
    pub event_type: TalosEventType,
    pub message_id: String,
    pub session_id: String,
    pub sequence: u64,
    pub payload: TalosEventPayload,
}
