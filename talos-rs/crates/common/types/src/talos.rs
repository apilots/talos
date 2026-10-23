use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{fmt, path::PathBuf};
use ts_rs::TS;

use crate::subagent::CollaborationToolResult;

pub use crate::event_api::{
    ContextCompactionMetrics, ContextCompactionOutcome, ContextCompactionStage,
    ContextCompactionTrigger,
};

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
    WaitingUserAction,
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
        #[ts(type = "unknown")]
        pub struct $name(
            #[schemars(schema_with = "crate::schema::json_value_schema")]
            #[ts(type = "unknown")]
            serde_json::Value,
        );

        impl $name {
            pub fn new(value: serde_json::Value) -> Self {
                Self(value)
            }

            pub fn into_inner(self) -> serde_json::Value {
                self.0
            }

            pub fn as_inner(&self) -> &serde_json::Value {
                &self.0
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
                write!(f, "{}", self.0)
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
pub enum SessionSourceType {
    Http,
    Desktop,
    Im,
    Cli,
    Ws,
    #[default]
    System,
}

impl SessionSourceType {
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

impl fmt::Display for SessionSourceType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Concrete source provider.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum SessionSourceProvider {
    Http,
    Sdk,
    Cli,
    Telegram,
    WebSocket,
    #[default]
    System,
}

impl SessionSourceProvider {
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

impl fmt::Display for SessionSourceProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Trusted origin metadata used when a Service creates a Session.
///
/// Gateways construct this value from their transport or channel. It must not
/// carry authenticated identity, request identity, paths, Agent selection, or
/// execution policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSource {
    pub source_type: SessionSourceType,
    pub source_provider: SessionSourceProvider,
    pub channel_id: Option<String>,
    pub thread_id: Option<String>,
    pub environment: Option<TalosSessionEnvironment>,
}

impl SessionSource {
    pub fn new(source_type: SessionSourceType, source_provider: SessionSourceProvider) -> Self {
        Self {
            source_type,
            source_provider,
            channel_id: None,
            thread_id: None,
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

    pub fn with_environment(mut self, environment: TalosSessionEnvironment) -> Self {
        self.environment = Some(environment);
        self
    }

    pub fn http() -> Self {
        Self::new(SessionSourceType::Http, SessionSourceProvider::Http)
    }

    pub fn sdk() -> Self {
        Self::new(SessionSourceType::Http, SessionSourceProvider::Sdk)
    }

    pub fn desktop() -> Self {
        Self::new(SessionSourceType::Desktop, SessionSourceProvider::Sdk)
    }

    pub fn cli() -> Self {
        Self::new(SessionSourceType::Cli, SessionSourceProvider::Cli)
    }

    pub fn ws() -> Self {
        Self::new(SessionSourceType::Ws, SessionSourceProvider::WebSocket)
    }

    pub fn im(source_provider: SessionSourceProvider) -> Self {
        Self::new(SessionSourceType::Im, source_provider)
    }

    pub fn system() -> Self {
        Self::new(SessionSourceType::System, SessionSourceProvider::System)
    }
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
    SubagentResult {
        result: CollaborationToolResult,
    },
    FileRead {
        path: String,
        offset: usize,
        limit: usize,
        total_lines: Option<usize>,
        #[serde(default, rename = "nextByteOffset")]
        #[ts(rename = "nextByteOffset", type = "number | null")]
        next_byte_offset: Option<u64>,
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
        #[serde(default, rename = "outputLog")]
        #[ts(rename = "outputLog")]
        output_log: Option<BashOutputLog>,
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
    /// A tool-produced image persisted in the authenticated user's FileService.
    Image {
        #[serde(rename = "fileId")]
        #[ts(rename = "fileId")]
        file_id: String,
        #[serde(rename = "mimeType")]
        #[ts(rename = "mimeType")]
        mime_type: String,
        label: String,
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

/// A bounded reference to output on the Session's executing Computer.
/// Paths are relative to the already user-scoped Workspace and resolved there;
/// the absolute path lets existing Read/Grep work independently of project CWD.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct BashOutputLog {
    pub path: String,
    pub relative_path: String,
    #[ts(type = "number")]
    pub total_bytes: u64,
    #[ts(type = "number")]
    pub stored_bytes: u64,
    #[ts(type = "number")]
    pub total_lines: u64,
    pub capture_status: BashOutputCaptureStatus,
    #[ts(type = "number")]
    pub expires_at: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum BashOutputCaptureStatus {
    Capturing,
    Complete,
    QuotaLimited,
    Interrupted,
    WriteFailed,
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

pub use crate::usage::TokenUsage;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum TalosTimelineNoticeLevel {
    Info,
    Warning,
    Error,
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
