use crate::talos::{
    StopReason, TalosContext, TalosInputItem, TalosOutputItem, TalosReasoning, TalosUsage,
};
use serde::{Deserialize, Serialize};

/// Request coming into the gateway.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayRequest {
    pub session_id: String,
    #[serde(default)]
    pub turn_id: Option<String>,
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub resume_context: Option<GatewayResumeContext>,
    pub input: Vec<TalosInputItem>,
    pub context: TalosContext,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayResumeContext {
    #[serde(default)]
    pub tool_call: Option<GatewayResumeToolCall>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayResumeToolCall {
    pub item_id: String,
    pub call_id: String,
    pub tool_name: String,
}

/// Response from the gateway.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayResponse {
    pub content: String,
    #[serde(default)]
    pub output_items: Vec<TalosOutputItem>,
    #[serde(default)]
    pub reasoning: Vec<TalosReasoning>,
    pub stop_reason: StopReason,
    #[serde(default)]
    pub usage: TalosUsage,
    pub success: bool,
    pub model: String,
    pub session_id: String,
}
