//! Talos Common Types
//!
//! Defines common types and ID structures

use serde::{Deserialize, Serialize};
use std::fmt;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

// Re-export key types for convenience
pub use async_trait::async_trait;
pub use talos_error::{TalosError, TalosResult};
pub type TalosConfig = talos_config::Config;

/// Agent ID
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct AgentId(pub String);

impl std::fmt::Display for AgentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl AgentId {
    pub fn new(id: &str) -> Self {
        Self(id.to_string())
    }

    pub fn new_random() -> Self {
        Self(Uuid::new_v4().to_string())
    }
}

/// Session ID
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct SessionId(pub String);

impl SessionId {
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

/// User ID
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct UserId(pub String);

impl UserId {
    pub fn new(id: &str) -> Self {
        Self(id.to_string())
    }
}

/// Channel ID
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ChannelId(pub String);

impl ChannelId {
    pub fn new(id: &str) -> Self {
        Self(id.to_string())
    }
}

/// Message ID
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct MessageId(pub String);

impl MessageId {
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }
}

impl Default for MessageId {
    fn default() -> Self {
        Self::new()
    }
}

/// Gateway module
pub mod gateway {
    use crate::talos::{
        TalosContext, TalosInputItem, TalosOutputItem, TalosReasoning, TalosUsage,
    };
    use serde::{Deserialize, Serialize};

    /// Request coming into the gateway
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct GatewayRequest {
        pub session_id: String,
        #[serde(default)]
        pub workspace_id: Option<String>,
        pub input: Vec<TalosInputItem>,
        pub context: TalosContext,
    }

    /// Response from the gateway
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct GatewayResponse {
        pub content: String,
        #[serde(default)]
        pub output_items: Vec<TalosOutputItem>,
        #[serde(default)]
        pub reasoning: Vec<TalosReasoning>,
        pub stop_reason: String,
        #[serde(default)]
        pub usage: TalosUsage,
        pub success: bool,
        pub model: String,
        pub session_id: String,
    }
}

/// Talos gateway protocol types shared by native and compatibility APIs.
pub mod talos {
    use super::*;

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
        Reasoning {
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
    #[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
        pub stop_reason: String,
        pub usage: TalosUsage,
    }

    /// Stream event type.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum TalosEventType {
        MessageStarted,
        MessageReasoningDelta,
        MessageOutputDelta,
        MessageToolCallStarted,
        MessageToolCallCompleted,
        MessageCompleted,
        MessageError,
    }

    /// Canonical stream event payload.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct TalosEvent {
        pub event_type: TalosEventType,
        pub message_id: String,
        pub session_id: String,
        pub sequence: u64,
        pub data: serde_json::Value,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    pub id: String,
    pub context: talos::TalosContext,
    pub model: String,
    pub created_at: u64,
    pub last_activity: u64,
    pub message_count: u64,
    #[serde(default)]
    pub conf: HashMap<String, serde_json::Value>,
}

impl SessionRecord {
    pub fn from_session(session: &TalosSession, base_model: &str) -> Self {
        let effective_model = session.config.llm.model.clone();
        let conf = SessionConfigOverridePolicy::build_conf_for_model(base_model, &effective_model);

        Self {
            id: session.context.session_id.clone(),
            context: talos::TalosContext {
                source_type: session.context.source_type.clone(),
                provider: session.context.provider.clone(),
                user_id: session.context.user_id.clone(),
                message_id: session.context.message_id.clone(),
                trace_id: session.context.trace_id.clone(),
                channel_id: session.context.channel_id.clone(),
                thread_id: session.context.thread_id.clone(),
            },
            model: effective_model,
            created_at: session.state.created_at,
            last_activity: session.state.last_activity,
            message_count: session.state.message_count,
            conf,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionContext {
    pub session_id: String,
    #[serde(default)]
    pub workspace_id: Option<String>,
    pub source_type: talos::TalosSourceType,
    pub provider: talos::TalosProvider,
    pub user_id: String,
    pub message_id: String,
    pub trace_id: String,
    #[serde(default)]
    pub channel_id: Option<String>,
    #[serde(default)]
    pub thread_id: Option<String>,
}

impl SessionContext {
    pub fn from_record(record: &SessionRecord, workspace_id: Option<String>) -> Self {
        Self {
            session_id: record.id.clone(),
            workspace_id,
            source_type: record.context.source_type.clone(),
            provider: record.context.provider.clone(),
            user_id: record.context.user_id.clone(),
            message_id: record.context.message_id.clone(),
            trace_id: record.context.trace_id.clone(),
            channel_id: record.context.channel_id.clone(),
            thread_id: record.context.thread_id.clone(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionState {
    pub created_at: u64,
    pub last_activity: u64,
    pub message_count: u64,
}

impl SessionState {
    pub fn from_record(record: &SessionRecord) -> Self {
        Self {
            created_at: record.created_at,
            last_activity: record.last_activity,
            message_count: record.message_count,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TalosSession {
    pub context: SessionContext,
    pub config: TalosConfig,
    pub state: SessionState,
}

pub struct SessionConfigOverridePolicy;

impl SessionConfigOverridePolicy {
    pub const AGENT_MODEL_KEY: &'static str = "agent.model";

    pub fn validate_conf(conf: &HashMap<String, serde_json::Value>) -> TalosResult<()> {
        for key in conf.keys() {
            if key != Self::AGENT_MODEL_KEY {
                return Err(TalosError::ValidationError(format!(
                    "unsupported session config key: {}",
                    key
                )));
            }
        }

        if let Some(value) = conf.get(Self::AGENT_MODEL_KEY) {
            if !value.is_string() {
                return Err(TalosError::ValidationError(
                    "agent.model must be a string".to_string(),
                ));
            }
        }

        Ok(())
    }

    pub fn extract_agent_model(
        conf: &HashMap<String, serde_json::Value>,
    ) -> TalosResult<Option<String>> {
        Self::validate_conf(conf)?;
        Ok(conf
            .get(Self::AGENT_MODEL_KEY)
            .and_then(|value| value.as_str())
            .map(ToString::to_string))
    }

    pub fn build_conf_for_model(
        base_model: &str,
        effective_model: &str,
    ) -> HashMap<String, serde_json::Value> {
        if effective_model == base_model {
            return HashMap::new();
        }

        let mut conf = HashMap::new();
        conf.insert(
            Self::AGENT_MODEL_KEY.to_string(),
            serde_json::Value::String(effective_model.to_string()),
        );
        conf
    }

    pub fn single_model_conf(model: &str) -> HashMap<String, serde_json::Value> {
        let mut conf = HashMap::new();
        conf.insert(
            Self::AGENT_MODEL_KEY.to_string(),
            serde_json::Value::String(model.to_string()),
        );
        conf
    }

    pub fn apply_to_config(
        base_config: &TalosConfig,
        record_model: &str,
        conf: &HashMap<String, serde_json::Value>,
    ) -> TalosResult<TalosConfig> {
        let mut config = base_config.clone();
        let model = Self::extract_agent_model(conf)?
            .unwrap_or_else(|| record_model.to_string());
        config.agent.model = model.clone();
        config.llm.model = model;
        Ok(config)
    }
}

impl TalosSession {
    pub fn restore(
        record: &SessionRecord,
        base_config: &TalosConfig,
        workspace_id: Option<String>,
    ) -> TalosResult<Self> {
        let config = SessionConfigOverridePolicy::apply_to_config(
            base_config,
            &record.model,
            &record.conf,
        )?;

        Ok(Self {
            context: SessionContext::from_record(record, workspace_id),
            config,
            state: SessionState::from_record(record),
        })
    }

    pub fn from_record(
        record: &SessionRecord,
        base_config: &TalosConfig,
        workspace_id: Option<String>,
    ) -> TalosResult<Self> {
        Self::restore(record, base_config, workspace_id)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionQuery {
    pub user_id: Option<String>,
    pub channel_id: Option<String>,
    pub provider: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionPatch {
    #[serde(default)]
    pub conf: HashMap<String, serde_json::Value>,
}

impl SessionPatch {
    pub fn agent_model(&self) -> TalosResult<Option<String>> {
        SessionConfigOverridePolicy::extract_agent_model(&self.conf)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionPatchAck {
    pub success: bool,
}

#[async_trait]
pub trait SessionService: Send + Sync {
    async fn create_session(
        &self,
        context: talos::TalosContext,
        model: Option<String>,
    ) -> TalosResult<SessionRecord>;

    async fn list_sessions(&self, query: SessionQuery) -> TalosResult<Vec<SessionRecord>>;

    async fn get_session(&self, session_id: &str) -> TalosResult<Option<SessionRecord>>;

    async fn patch_session(
        &self,
        session_id: &str,
        patch: SessionPatch,
    ) -> TalosResult<SessionPatchAck>;

    async fn close_session(&self, session_id: &str) -> TalosResult<()>;
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

/// Messages module
pub mod messages {
    use super::*;

    /// Different types of message payloads
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(tag = "type", content = "data")]
    pub enum MessagePayload {
        Text(String),
        Command {
            name: String,
            args: HashMap<String, String>,
        },
        ToolResult(super::tools::ToolResult), // 使用 super::tools 引用
    }

    /// A message in the system
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Message {
        pub id: String,
        pub agent_id: String,
        pub content: String,
        pub timestamp: std::time::SystemTime,
        pub payload: MessagePayload,
    }
}

/// Agents module
pub mod agents {
    use super::*;

    /// Information about an agent
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct AgentInfo {
        pub id: String,
        pub name: String,
        pub description: String,
        pub agent_type: String,
        pub status: String,
        pub capabilities: Vec<String>,
    }
}

/// Tools module
pub mod tools {
    use super::*;

    /// A tool call to be executed
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ToolCall {
        pub id: String,
        pub name: String,
        pub arguments: serde_json::Value,
    }

    /// Result of a tool execution
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ToolResult {
        pub id: String,
        pub tool_call_id: String,
        pub result: String,
        pub success: bool,
    }
}

/// RequestHandler trait for decoupling Gateway from Orchestration
///
/// This trait defines the interface that Gateway uses to communicate
/// with the orchestration layer without direct dependency.
#[async_trait]
pub trait RequestHandler: Send + Sync {
    /// Handle a gateway request and return a response
    async fn handle(
        &self,
        request: gateway::GatewayRequest,
    ) -> TalosResult<gateway::GatewayResponse>;

    /// Handle a gateway request and return Talos stream events.
    ///
    /// Default implementation falls back to non-stream `handle` and converts
    /// output items to canonical events in process order.
    async fn handle_stream(
        &self,
        request: gateway::GatewayRequest,
    ) -> TalosResult<tokio::sync::mpsc::Receiver<talos::TalosEvent>> {
        let response = self.handle(request.clone()).await?;
        let request_id = format!("req_{}", uuid::Uuid::new_v4().simple());
        let message_id = format!("msg_{}", uuid::Uuid::new_v4().simple());
        let session_id = response.session_id.clone();

        let output = if response.output_items.is_empty() {
            vec![talos::TalosOutputItem::OutputText {
                text: response.content.clone(),
            }]
        } else {
            response.output_items.clone()
        };

        let mut events = Vec::new();
        let mut sequence: u64 = 0;
        events.push(talos::TalosEvent {
            event_type: talos::TalosEventType::MessageStarted,
            message_id: message_id.clone(),
            session_id: session_id.clone(),
            sequence,
            data: serde_json::json!({
                "request_id": request_id,
                "model": response.model,
                "usage": response.usage,
                "iteration": 0_u64,
                "step_id": "start",
            }),
        });
        sequence += 1;

        for (item_index, item) in output.iter().enumerate() {
            let iteration = item_index as u64 + 1;
            let step_id = format!("step-{}", iteration);
            match item {
                talos::TalosOutputItem::Reasoning { text } => {
                    events.push(talos::TalosEvent {
                        event_type: talos::TalosEventType::MessageReasoningDelta,
                        message_id: message_id.clone(),
                        session_id: session_id.clone(),
                        sequence,
                        data: serde_json::json!({
                            "request_id": request_id,
                            "delta": text,
                            "reasoning_source": if text.trim_start().starts_with("Kernel summary:") { "kernel" } else { "provider" },
                            "item_index": item_index,
                            "iteration": iteration,
                            "step_id": step_id,
                        }),
                    });
                }
                talos::TalosOutputItem::OutputText { text } => {
                    events.push(talos::TalosEvent {
                        event_type: talos::TalosEventType::MessageOutputDelta,
                        message_id: message_id.clone(),
                        session_id: session_id.clone(),
                        sequence,
                        data: serde_json::json!({
                            "request_id": request_id,
                            "delta": text,
                            "item_index": item_index,
                            "iteration": iteration,
                            "step_id": step_id,
                        }),
                    });
                }
                talos::TalosOutputItem::ToolCall {
                    call_id,
                    name,
                    arguments,
                } => {
                    events.push(talos::TalosEvent {
                        event_type: talos::TalosEventType::MessageToolCallStarted,
                        message_id: message_id.clone(),
                        session_id: session_id.clone(),
                        sequence,
                        data: serde_json::json!({
                            "request_id": request_id,
                            "call_id": call_id,
                            "name": name,
                            "arguments": arguments,
                            "item_index": item_index,
                            "iteration": iteration,
                            "step_id": step_id,
                            "timing_ms": serde_json::Value::Null,
                        }),
                    });
                }
                talos::TalosOutputItem::ToolResult {
                    call_id,
                    content,
                    is_error,
                } => {
                    events.push(talos::TalosEvent {
                        event_type: talos::TalosEventType::MessageToolCallCompleted,
                        message_id: message_id.clone(),
                        session_id: session_id.clone(),
                        sequence,
                        data: serde_json::json!({
                            "request_id": request_id,
                            "call_id": call_id,
                            "tool": serde_json::Value::Null,
                            "content": content,
                            "is_error": is_error,
                            "item_index": item_index,
                            "iteration": iteration,
                            "step_id": step_id,
                            "timing_ms": serde_json::Value::Null,
                        }),
                    });
                }
            }
            sequence = sequence.saturating_add(1);
        }

        events.push(talos::TalosEvent {
            event_type: talos::TalosEventType::MessageCompleted,
            message_id: message_id.clone(),
            session_id: session_id.clone(),
            sequence,
            data: serde_json::json!({
                "request_id": request_id,
                "stop_reason": response.stop_reason,
                "usage": response.usage,
                "total_output_items": output.len(),
                "iteration": output.len() as u64 + 1,
                "step_id": "completed",
            }),
        });

        let (tx, rx) = tokio::sync::mpsc::channel(events.len().max(1));
        tokio::spawn(async move {
            for event in events {
                if tx.send(event).await.is_err() {
                    break;
                }
            }
        });
        Ok(rx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_id() {
        let id = AgentId::new("test");
        assert_eq!(id.0, "test");

        let id2 = AgentId::new_random();
        assert!(!id2.0.is_empty());
    }

    #[test]
    fn test_session_id() {
        let id = SessionId::new();
        assert!(!id.0.is_empty());
    }

    #[test]
    fn test_talos_roundtrip() {
        let req = talos::TalosMessageRequest {
            input: vec![talos::TalosInputItem::Text {
                text: "hello".to_string(),
            }],
            stream: Some(false),
            workspace_id: Some("workspace-1".to_string()),
        };

        let encoded = serde_json::to_string(&req).unwrap();
        let decoded: talos::TalosMessageRequest = serde_json::from_str(&encoded).unwrap();
        assert!(decoded.stream.is_some());
        assert_eq!(decoded.workspace_id.as_deref(), Some("workspace-1"));
        assert!(matches!(
            decoded.input.first(),
            Some(talos::TalosInputItem::Text { text }) if text == "hello"
        ));
    }

    #[test]
    fn test_session_context_and_state_from_record() {
        let record = SessionRecord {
            id: "session-1".to_string(),
            context: talos::TalosContext {
                source_type: talos::TalosSourceType::Im,
                provider: talos::TalosProvider::Telegram,
                user_id: "user-1".to_string(),
                message_id: "message-1".to_string(),
                trace_id: "trace-1".to_string(),
                channel_id: Some("channel-1".to_string()),
                thread_id: Some("thread-1".to_string()),
            },
            model: "qwen-max".to_string(),
            created_at: 100,
            last_activity: 200,
            message_count: 3,
            conf: HashMap::new(),
        };

        let context = SessionContext::from_record(&record, Some("workspace-1".to_string()));
        let state = SessionState::from_record(&record);

        assert_eq!(context.session_id, "session-1");
        assert_eq!(context.workspace_id.as_deref(), Some("workspace-1"));
        assert_eq!(context.provider, talos::TalosProvider::Telegram);
        assert_eq!(state.created_at, 100);
        assert_eq!(state.last_activity, 200);
        assert_eq!(state.message_count, 3);
    }

    #[test]
    fn test_talos_session_from_record_applies_conf_override() {
        let mut record = SessionRecord {
            id: "session-1".to_string(),
            context: talos::TalosContext {
                source_type: talos::TalosSourceType::Http,
                provider: talos::TalosProvider::Http,
                user_id: "user-1".to_string(),
                message_id: "message-1".to_string(),
                trace_id: "trace-1".to_string(),
                channel_id: Some("default".to_string()),
                thread_id: None,
            },
            model: "qwen-max".to_string(),
            created_at: 100,
            last_activity: 200,
            message_count: 3,
            conf: HashMap::new(),
        };
        record.conf.insert(
            "agent.model".to_string(),
            serde_json::Value::String("qwen-plus".to_string()),
        );

        let session = TalosSession::from_record(&record, &TalosConfig::default(), None)
            .expect("talos session restore should succeed");
        assert_eq!(session.config.agent.model, "qwen-plus");
        assert_eq!(session.config.llm.model, "qwen-plus");
    }

    #[test]
    fn test_session_override_policy_rejects_unknown_keys() {
        let mut conf = HashMap::new();
        conf.insert(
            "agent.kernel.prompt.max_dynamic_tokens".to_string(),
            serde_json::Value::Number(1.into()),
        );

        let error = SessionConfigOverridePolicy::validate_conf(&conf)
            .expect_err("unknown conf key should be rejected");
        assert!(error.to_string().contains("unsupported session config key"));
    }

    #[test]
    fn test_session_override_policy_builds_model_conf() {
        let conf = SessionConfigOverridePolicy::single_model_conf("gpt-4o");
        let model = SessionConfigOverridePolicy::extract_agent_model(&conf)
            .expect("conf extraction should succeed");

        assert_eq!(model.as_deref(), Some("gpt-4o"));
    }
}
