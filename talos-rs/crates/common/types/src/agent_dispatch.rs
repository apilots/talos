use crate::talos::{
    StopReason, TalosContext, TalosInputItem, TalosOutputItem, TalosReasoning, TalosUsage,
};
use crate::TalosSession;
use serde::{Deserialize, Serialize};

/// Internal request dispatched from TalosService to AgentCoordinator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDispatchRequest {
    pub session_id: String,
    #[serde(default)]
    pub turn_id: Option<String>,
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub session: Option<Box<TalosSession>>,
    #[serde(default)]
    pub resume_context: Option<AgentDispatchResumeContext>,
    pub input: Vec<TalosInputItem>,
    pub context: TalosContext,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDispatchResumeContext {
    #[serde(default)]
    pub tool_call: Option<AgentDispatchResumeToolCall>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDispatchResumeToolCall {
    pub item_id: String,
    pub call_id: String,
    pub tool_name: String,
}

/// Internal response returned from AgentCoordinator to TalosService.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDispatchResponse {
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
