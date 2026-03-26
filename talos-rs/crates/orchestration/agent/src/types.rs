use serde::{Deserialize, Serialize};
use talos_llm_provider::FunctionCall;
use talos_types::talos::{TalosOutputItem, TalosReasoning};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResponse {
    pub output_text: String,
    #[serde(default)]
    pub output_items: Vec<TalosOutputItem>,
    #[serde(default)]
    pub reasoning: Vec<TalosReasoning>,
    pub tool_calls: Vec<ToolCall>,
}

pub type ToolCall = FunctionCall;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStream {
    pub event_type: String,
    pub data: serde_json::Value,
}
