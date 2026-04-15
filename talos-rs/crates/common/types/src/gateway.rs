use crate::talos::{
    StopReason, TalosContext, TalosInputItem, TalosOutputItem, TalosReasoning, TalosUsage,
};
use serde::{Deserialize, Serialize};

/// Request coming into the gateway.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayRequest {
    pub session_id: String,
    #[serde(default)]
    pub workspace_id: Option<String>,
    pub input: Vec<TalosInputItem>,
    pub context: TalosContext,
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
