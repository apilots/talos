use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;
use ts_rs::TS;
use uuid::Uuid;

/// Talos gateway protocol types shared by server and client components.

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default, JsonSchema, TS)]
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
    WaitingDynamicInput,
    TransferUnresolved,
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

macro_rules! semantic_json_value {
    ($name:ident) => {
        #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
        pub struct $name {
            #[schemars(schema_with = "crate::schema::json_value_schema")]
            #[ts(type = "unknown")]
            value: serde_json::Value,
        }

        impl $name {
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

        impl Default for $name {
            fn default() -> Self {
                Self::new(serde_json::Value::Null)
            }
        }

        impl From<serde_json::Value> for $name {
            fn from(value: serde_json::Value) -> Self {
                Self::new(value)
            }
        }

        impl From<TalosJsonValue> for $name {
            fn from(value: TalosJsonValue) -> Self {
                Self::new(value.into_inner())
            }
        }

        impl From<$name> for serde_json::Value {
            fn from(value: $name) -> Self {
                value.into_inner()
            }
        }

        impl From<$name> for TalosJsonValue {
            fn from(value: $name) -> Self {
                TalosJsonValue::new(value.into_inner())
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.value)
            }
        }
    };
}

semantic_json_value!(JsonContentValue);
semantic_json_value!(ToolArguments);
semantic_json_value!(DynamicToolArguments);
semantic_json_value!(ToolExecutionResult);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
pub struct TalosSessionEnvironment {
    pub directory: PathBuf,
    #[serde(default)]
    pub connection_mode: TalosConnectionMode,
}

/// High-level source category.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum TalosSourceType {
    Http,
    Desktop,
    Im,
    Cli,
    Ws,
    #[default]
    System,
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
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum TalosProvider {
    Http,
    Sdk,
    Cli,
    Telegram,
    WebSocket,
    #[default]
    System,
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TalosInputItem {
    Text {
        text: String,
    },
    SkillSelection {
        name: String,
        path: String,
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
    Text {
        text: String,
    },
    Json {
        value: JsonContentValue,
    },
    FileRead {
        path: String,
        offset: usize,
        limit: usize,
        total_lines: usize,
        content: String,
        truncated: bool,
    },
    Diff {
        files: Vec<TalosToolFileDiff>,
    },
    SearchResults {
        #[serde(default)]
        query: Option<String>,
        matches: Vec<TalosToolSearchMatch>,
        offset: usize,
        limit: usize,
        #[serde(default)]
        total_matches: Option<usize>,
        more_available: bool,
        partial: bool,
    },
    BashOutput {
        #[serde(default)]
        #[ts(type = "number | null")]
        session_id: Option<i64>,
        status: TalosToolTaskStatus,
        #[serde(default)]
        exit_code: Option<i32>,
        stdout: String,
        stderr: String,
        truncated: bool,
        #[serde(default)]
        #[ts(type = "number | null")]
        elapsed_ms: Option<u64>,
    },
    WebReference {
        url: String,
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        citation_id: Option<String>,
        #[serde(default)]
        snippet: Option<String>,
        #[serde(default)]
        source_domain: Option<String>,
        #[serde(default)]
        published_at: Option<String>,
        #[serde(default)]
        provider_rank: Option<usize>,
    },
    ArtifactRef {
        artifact_id: String,
        #[serde(default)]
        mime_type: Option<String>,
        #[serde(default)]
        label: Option<String>,
    },
    McpResourceRef {
        server: String,
        uri: String,
        #[serde(default)]
        mime_type: Option<String>,
    },
    Progress {
        message: String,
        #[serde(default)]
        #[ts(type = "number | null")]
        elapsed_ms: Option<u64>,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TalosToolTaskStatus {
    Queued,
    Running,
    Completed,
    Failed,
    TimedOut,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct TalosToolFileDiff {
    pub path: String,
    #[serde(default)]
    pub old_path: Option<String>,
    pub hunks: Vec<TalosToolDiffHunk>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct TalosToolDiffHunk {
    #[serde(default)]
    pub old_start: Option<u32>,
    #[serde(default)]
    pub old_lines: Option<u32>,
    #[serde(default)]
    pub new_start: Option<u32>,
    #[serde(default)]
    pub new_lines: Option<u32>,
    pub lines: Vec<TalosToolDiffLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TalosToolDiffLine {
    Context { text: String },
    Added { text: String },
    Removed { text: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct TalosToolSearchMatch {
    pub path: String,
    pub line: u32,
    #[serde(default)]
    pub column: Option<u32>,
    pub text: String,
    #[serde(default)]
    pub before: Vec<String>,
    #[serde(default)]
    pub after: Vec<String>,
}

impl TalosContentItem {
    pub fn display_text(&self) -> String {
        match self {
            Self::Text { text } => text.clone(),
            Self::Json { value } => value.to_string(),
            Self::FileRead { content, .. } => content.clone(),
            Self::Diff { files } => format!("diff: {} file(s)", files.len()),
            Self::SearchResults {
                matches,
                more_available,
                partial,
                ..
            } => {
                let suffix = if *partial {
                    " partial"
                } else if *more_available {
                    " more_available"
                } else {
                    ""
                };
                format!("search_results: {} match(es){suffix}", matches.len())
            }
            Self::BashOutput {
                stdout,
                stderr,
                exit_code,
                status,
                ..
            } => {
                let mut parts = Vec::new();
                if !stdout.trim().is_empty() {
                    parts.push(stdout.trim().to_string());
                }
                if !stderr.trim().is_empty() {
                    parts.push(stderr.trim().to_string());
                }
                if parts.is_empty() {
                    parts.push(format!("bash: {:?} exit_code={exit_code:?}", status));
                }
                parts.join("\n")
            }
            Self::WebReference {
                url,
                title,
                citation_id,
                snippet,
                ..
            } => format!(
                "web_reference: {}{}{}{}",
                title.as_deref().unwrap_or(url),
                title
                    .as_ref()
                    .map(|_| format!(" ({url})"))
                    .unwrap_or_default(),
                citation_id
                    .as_ref()
                    .map(|id| format!(" [{id}]"))
                    .unwrap_or_default(),
                snippet
                    .as_ref()
                    .filter(|value| !value.trim().is_empty())
                    .map(|value| format!("\n{value}"))
                    .unwrap_or_default()
            ),
            Self::ArtifactRef {
                artifact_id,
                label,
                mime_type,
            } => format!(
                "artifact: {}{}",
                label.as_deref().unwrap_or(artifact_id),
                mime_type
                    .as_ref()
                    .map(|mime| format!(" ({mime})"))
                    .unwrap_or_default()
            ),
            Self::McpResourceRef {
                server,
                uri,
                mime_type,
            } => format!(
                "mcp_resource: {server}:{uri}{}",
                mime_type
                    .as_ref()
                    .map(|mime| format!(" ({mime})"))
                    .unwrap_or_default()
            ),
            Self::Progress { message, .. } => message.clone(),
        }
    }
}

/// Structured item produced while executing an agent run.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RunItem {
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
        arguments: ToolArguments,
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
pub struct TalosReasoning {
    pub text: String,
}

/// Token usage values shared by runs, sessions, history, and metrics.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
pub struct TokenUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub total_tokens: u32,
    #[serde(default)]
    #[ts(optional)]
    #[ts(type = "number | null")]
    pub cache_creation_input_tokens: Option<u32>,
    #[serde(default)]
    #[ts(optional)]
    #[ts(type = "number | null")]
    pub cache_read_input_tokens: Option<u32>,
}

impl TokenUsage {
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

/// Kernel-owned execution metrics for a turn.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
pub struct TurnExecutionMetrics {
    pub model_rounds: u32,
    pub tool_calls: u32,
    pub tool_successes: u32,
    pub tool_errors: u32,
    pub stop_reason: StopReason,
    pub usage: TokenUsage,
    #[serde(default)]
    #[ts(type = "number | null")]
    pub duration_ms: Option<u64>,
}

fn sum_optional_tokens(left: Option<u32>, right: Option<u32>) -> Option<u32> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.saturating_add(right)),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

/// Agent execution event lifecycle type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum AgentEventType {
    TurnStarted,
    ItemStarted,
    ItemDelta,
    ItemCompleted,
    AssistantTextCommitted,
    TurnMetrics,
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
    Unknown,
    Commentary,
    FinalAnswer,
    System,
}

/// Lifecycle status for a timeline item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum TalosTimelineItemStatus {
    Running,
    WaitingDynamicInput,
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
    #[ts(type = "number")]
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
    ToolDynamicRequest {
        call_id: String,
        tool_name: String,
        arguments: DynamicToolArguments,
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
    #[ts(type = "number")]
    pub started_at_ms: u64,
    #[serde(default)]
    #[ts(type = "number | null")]
    pub completed_at_ms: Option<u64>,
    pub content: TalosTimelineContent,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct TalosTurnStartedEvent {
    pub request_id: String,
    pub turn_id: String,
    #[ts(type = "number")]
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
    #[ts(type = "number")]
    pub completed_at_ms: u64,
    #[serde(default)]
    pub final_content: Option<TalosTimelineContent>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct TalosAssistantTextCommittedEvent {
    pub request_id: String,
    pub turn_id: String,
    pub item_id: String,
    pub phase: TalosTimelinePhase,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct TalosTurnMetricsEvent {
    pub request_id: String,
    pub turn_id: String,
    pub metrics: TurnExecutionMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct TalosTurnCompletedEvent {
    pub request_id: String,
    pub turn_id: String,
    #[ts(type = "number")]
    pub completed_at_ms: u64,
    pub stop_reason: StopReason,
    pub usage: TokenUsage,
    #[serde(default)]
    pub metrics: Option<TurnExecutionMetrics>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct TalosTurnErrorEvent {
    pub request_id: String,
    pub turn_id: String,
    pub error: String,
    #[ts(type = "number")]
    pub failed_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEventPayload {
    TurnStarted(TalosTurnStartedEvent),
    ItemStarted(TalosItemStartedEvent),
    ItemDelta(TalosItemDeltaEvent),
    ItemCompleted(TalosItemCompletedEvent),
    AssistantTextCommitted(TalosAssistantTextCommittedEvent),
    TurnMetrics(TalosTurnMetricsEvent),
    TurnCompleted(TalosTurnCompletedEvent),
    TurnError(TalosTurnErrorEvent),
}

/// Agent execution event.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct AgentEvent {
    #[serde(default)]
    #[ts(type = "number | null")]
    pub event_seq: Option<i64>,
    #[serde(default)]
    pub event_id: Option<String>,
    #[serde(default)]
    #[ts(type = "number | null")]
    pub created_at_ms: Option<i64>,
    pub event_type: AgentEventType,
    pub turn_id: String,
    pub message_id: String,
    pub session_id: String,
    #[ts(type = "number")]
    pub sequence: u64,
    pub payload: AgentEventPayload,
}

/// Public recoverable timeline stream event.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct TimelineEventEnvelope {
    pub event_id: String,
    #[ts(type = "number")]
    pub event_seq: i64,
    #[ts(type = "number")]
    pub created_at_ms: i64,
    pub session_id: String,
    pub turn_id: String,
    pub message_id: String,
    pub event: AgentEvent,
}

impl TimelineEventEnvelope {
    pub fn from_persisted_event(event: AgentEvent) -> Option<Self> {
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
    #[ts(type = "number")]
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
    pub usage: TokenUsage,
    #[serde(default)]
    pub metrics: Option<TurnExecutionMetrics>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum TalosToolProgressKind {
    Status,
    Stdout,
    Stderr,
}

#[cfg(test)]
#[path = "talos_tests.rs"]
mod tests;
