use crate::gateway;
use crate::talos;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

pub mod agents {
    use super::*;

    /// Information about an agent.
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

pub mod messages {
    use super::*;

    /// Different types of message payloads.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(tag = "type", content = "data")]
    pub enum MessagePayload {
        Text(String),
        Command {
            name: String,
            args: HashMap<String, String>,
        },
        ToolResult(super::tools::ToolResult),
    }

    /// A message in the system.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Message {
        pub id: String,
        pub agent_id: String,
        pub content: String,
        pub timestamp: std::time::SystemTime,
        pub payload: MessagePayload,
    }
}

pub mod tools {
    use super::*;

    /// A tool call to be executed.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ToolCall {
        pub id: String,
        pub name: String,
        pub arguments: talos::TalosJsonValue,
    }

    /// Result of a tool execution.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ToolResult {
        pub id: String,
        pub tool_call_id: String,
        pub result: String,
        pub success: bool,
    }
}

pub use agents::AgentInfo;
pub use messages::{Message, MessagePayload};
pub use tools::{ToolCall, ToolResult};

/// Return a millisecond timestamp.
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

/// RequestHandler trait for decoupling Gateway from Orchestration.
///
/// This trait defines the interface that Gateway uses to communicate
/// with the orchestration layer without direct dependency.
#[async_trait]
pub trait RequestHandler: Send + Sync {
    /// Handle a gateway request and return a response.
    async fn handle(
        &self,
        request: gateway::GatewayRequest,
    ) -> crate::TalosResult<gateway::GatewayResponse>;

    /// Return the internal domain timeline stream before application-service persistence.
    ///
    /// Public transports must expose `TimelineEventEnvelope` after application-service records
    /// these domain events and attaches recovery cursor metadata.
    async fn handle_stream(
        &self,
        request: gateway::GatewayRequest,
    ) -> crate::TalosResult<tokio::sync::mpsc::Receiver<talos::TalosTimelineEvent>>;

    async fn cancel_active_turn(&self, session_id: &str) -> crate::TalosResult<()>;

    async fn submit_tool_approval(
        &self,
        _session_id: &str,
        _call_id: &str,
        _tool_name: &str,
        _approved: bool,
        _reason: Option<String>,
    ) -> crate::TalosResult<()> {
        Err(crate::TalosError::InternalError(
            "submit_tool_approval is not supported by this request handler".to_string(),
        ))
    }

    async fn submit_dynamic_tool_result(
        &self,
        _session_id: &str,
        _call_id: &str,
        _tool_name: &str,
        _output: String,
        _success: bool,
    ) -> crate::TalosResult<()> {
        Err(crate::TalosError::InternalError(
            "submit_dynamic_tool_result is not supported by this request handler".to_string(),
        ))
    }
}
