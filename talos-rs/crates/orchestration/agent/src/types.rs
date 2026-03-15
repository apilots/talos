use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use talos_llm_provider::FunctionCall;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResponse {
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
    pub metadata: HashMap<String, serde_json::Value>,
}

pub type ToolCall = FunctionCall;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStream {
    pub event_type: String,
    pub data: serde_json::Value,
}
