//! Talos Common Types
//!
//! Defines common types and ID structures

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

// Re-export key types for convenience
pub use async_trait::async_trait;
pub use talos_error::{TalosError, TalosResult};

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
    use crate::canonical::{CanonicalContext, CanonicalOutputItem, CanonicalUsage};
    use serde::{Deserialize, Serialize};
    use serde_json::Value;

    /// Request coming into the gateway
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct GatewayRequest {
        pub channel: String,
        pub user_id: String,
        pub content: String,
        pub model: Option<String>,
        pub session_id: Option<String>,
        pub context: Option<CanonicalContext>,
        pub metadata: Option<Value>,
    }

    /// Response from the gateway
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct GatewayResponse {
        pub content: String,
        #[serde(default)]
        pub output_items: Vec<CanonicalOutputItem>,
        pub stop_reason: String,
        #[serde(default)]
        pub usage: CanonicalUsage,
        pub success: bool,
        pub model: String,
        pub session_id: Option<String>,
    }
}

/// Canonical gateway protocol types shared by native and compatibility APIs.
pub mod canonical {
    use super::*;

    /// Cross-channel request context.
    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    pub struct CanonicalContext {
        pub source_type: String,
        pub provider: String,
        pub channel_id: String,
        pub user_id: String,
        pub thread_id: Option<String>,
        pub message_id: Option<String>,
        pub trace_id: Option<String>,
    }

    /// Canonical message input item.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(tag = "type", rename_all = "snake_case")]
    pub enum CanonicalInputItem {
        InputText {
            text: String,
        },
        InputImage {
            url: String,
            #[serde(default)]
            mime_type: Option<String>,
        },
        InputFile {
            #[serde(default)]
            url: Option<String>,
            #[serde(default)]
            file_id: Option<String>,
            #[serde(default)]
            mime_type: Option<String>,
        },
        InputAudio {
            #[serde(default)]
            url: Option<String>,
            #[serde(default)]
            data_base64: Option<String>,
            #[serde(default)]
            mime_type: Option<String>,
        },
    }

    /// Canonical content item for rich output blocks.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(tag = "type", rename_all = "snake_case")]
    pub enum CanonicalContentItem {
        Text { text: String },
        Json { value: serde_json::Value },
    }

    /// Canonical message output item.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(tag = "type", rename_all = "snake_case")]
    pub enum CanonicalOutputItem {
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
            content: Vec<CanonicalContentItem>,
            #[serde(default)]
            is_error: bool,
        },
    }

    /// Canonical message request payload for session interactions.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct CanonicalMessageRequest {
        pub input: Vec<CanonicalInputItem>,
        #[serde(default)]
        pub stream: Option<bool>,
        pub model: Option<String>,
        pub metadata: Option<serde_json::Value>,
    }

    /// Token usage values.
    #[derive(Debug, Clone, Default, Serialize, Deserialize)]
    pub struct CanonicalUsage {
        pub input_tokens: u32,
        pub output_tokens: u32,
        pub total_tokens: u32,
    }

    /// Canonical response shape returned by session interaction APIs.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct CanonicalMessageResponse {
        pub request_id: String,
        pub message_id: String,
        pub session_id: String,
        pub model: String,
        pub output: Vec<CanonicalOutputItem>,
        pub stop_reason: String,
        pub usage: CanonicalUsage,
    }

    /// Stream event type.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum CanonicalEventType {
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
    pub struct CanonicalEvent {
        pub event_type: CanonicalEventType,
        pub message_id: String,
        pub session_id: String,
        pub sequence: u64,
        pub data: serde_json::Value,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    pub id: String,
    pub context: canonical::CanonicalContext,
    pub model: String,
    pub created_at: u64,
    pub last_activity: u64,
    pub message_count: u64,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionQuery {
    pub user_id: Option<String>,
    pub channel_id: Option<String>,
    pub provider: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionPatch {
    pub model: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

#[async_trait]
pub trait SessionService: Send + Sync {
    async fn create_session(
        &self,
        context: canonical::CanonicalContext,
        model: Option<String>,
        metadata: Option<serde_json::Value>,
    ) -> TalosResult<SessionRecord>;

    async fn list_sessions(&self, query: SessionQuery) -> TalosResult<Vec<SessionRecord>>;

    async fn get_session(&self, session_id: &str) -> TalosResult<Option<SessionRecord>>;

    async fn patch_session(
        &self,
        session_id: &str,
        patch: SessionPatch,
    ) -> TalosResult<Option<SessionRecord>>;
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
    use serde_json;

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
        pub metadata: HashMap<String, serde_json::Value>,
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
        pub metadata: HashMap<String, serde_json::Value>,
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
        pub execution_context: HashMap<String, serde_json::Value>,
    }

    /// Result of a tool execution
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ToolResult {
        pub id: String,
        pub tool_call_id: String,
        pub result: String,
        pub success: bool,
        pub execution_context: HashMap<String, serde_json::Value>,
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

    /// Handle a gateway request and return canonical stream events.
    ///
    /// Default implementation falls back to non-stream `handle` and converts
    /// output items to canonical events in process order.
    async fn handle_stream(
        &self,
        request: gateway::GatewayRequest,
    ) -> TalosResult<tokio::sync::mpsc::Receiver<canonical::CanonicalEvent>> {
        let response = self.handle(request.clone()).await?;
        let request_id = format!("req_{}", uuid::Uuid::new_v4().simple());
        let message_id = format!("msg_{}", uuid::Uuid::new_v4().simple());
        let session_id = response
            .session_id
            .clone()
            .or(request.session_id.clone())
            .unwrap_or_else(|| "session-unknown".to_string());

        let output = if response.output_items.is_empty() {
            vec![canonical::CanonicalOutputItem::OutputText {
                text: response.content.clone(),
            }]
        } else {
            response.output_items.clone()
        };

        let mut events = Vec::new();
        let mut sequence: u64 = 0;
        events.push(canonical::CanonicalEvent {
            event_type: canonical::CanonicalEventType::MessageStarted,
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
                canonical::CanonicalOutputItem::Reasoning { text } => {
                    events.push(canonical::CanonicalEvent {
                        event_type: canonical::CanonicalEventType::MessageReasoningDelta,
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
                canonical::CanonicalOutputItem::OutputText { text } => {
                    events.push(canonical::CanonicalEvent {
                        event_type: canonical::CanonicalEventType::MessageOutputDelta,
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
                canonical::CanonicalOutputItem::ToolCall {
                    call_id,
                    name,
                    arguments,
                } => {
                    events.push(canonical::CanonicalEvent {
                        event_type: canonical::CanonicalEventType::MessageToolCallStarted,
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
                canonical::CanonicalOutputItem::ToolResult {
                    call_id,
                    content,
                    is_error,
                } => {
                    events.push(canonical::CanonicalEvent {
                        event_type: canonical::CanonicalEventType::MessageToolCallCompleted,
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

        events.push(canonical::CanonicalEvent {
            event_type: canonical::CanonicalEventType::MessageCompleted,
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
    fn test_canonical_roundtrip() {
        let req = canonical::CanonicalMessageRequest {
            input: vec![canonical::CanonicalInputItem::InputText {
                text: "hello".to_string(),
            }],
            stream: Some(false),
            model: Some("gpt-4o-mini".to_string()),
            metadata: Some(serde_json::json!({"k":"v"})),
        };

        let encoded = serde_json::to_string(&req).unwrap();
        let decoded: canonical::CanonicalMessageRequest = serde_json::from_str(&encoded).unwrap();
        assert!(decoded.stream.is_some());
        assert!(matches!(
            decoded.input.first(),
            Some(canonical::CanonicalInputItem::InputText { text }) if text == "hello"
        ));
    }
}
