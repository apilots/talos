use crate::talos::{RunItem, StopReason, TalosContext, TalosInputItem, TalosReasoning, TokenUsage};
use crate::TalosSession;
use serde::{Deserialize, Serialize};

/// Internal request dispatched from TalosService to AgentCoordinator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDispatchParams {
    /// Talos turn id; this is also the internal Agent run identity.
    pub turn_id: String,
    /// Optional Agent routing key. This is not a user workspace path.
    #[serde(default)]
    pub workspace_id: Option<String>,
    /// Prepared product session used as the single session identity source.
    pub session: TalosSession,
    #[serde(default)]
    pub resume_context: Option<AgentDispatchResumeContext>,
    pub input: Vec<TalosInputItem>,
    pub context: TalosContext,
}

/// Internal input request dispatched from TalosService to AgentCoordinator.
///
/// This is the Service -> Coordinator input bridge. New input kinds should
/// extend AgentDispatchInput instead of adding parallel submit_* methods.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDispatchInputParams {
    /// Prepared product session used to route the input to the owning Agent.
    pub session: TalosSession,
    /// Talos turn id; this is also the internal Agent run identity.
    pub turn_id: String,
    /// Structured input payload accepted by the dispatch boundary.
    pub input: AgentDispatchInput,
}

/// Structured input accepted by AgentCoordinator before Harness conversion.
///
/// Variants represent Coordinator-visible Agent inputs. Harness-specific or
/// runtime-private payloads must be converted below this boundary.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentDispatchInput {
    /// Result for a pending dynamic tool call validated by TalosService.
    DynamicToolResult {
        call_id: String,
        tool_name: String,
        output: String,
        success: bool,
    },
}

/// Internal continuation context passed from Service to Coordinator/Harness.
///
/// This context is for resuming a validated pending dynamic tool result within
/// the same Talos turn. It is not a backend continuation token, attach handle,
/// or Gateway/SDK public recovery DTO.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDispatchResumeContext {
    #[serde(default)]
    pub tool_call: Option<AgentDispatchResumeToolCall>,
}

/// Dynamic tool call identity restored from Service pending action state.
///
/// The fields identify the existing timeline item and tool call for the same
/// Talos turn. Runtime-private task or thread identifiers must not be stored
/// in this object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDispatchResumeToolCall {
    pub item_id: String,
    pub call_id: String,
    pub tool_name: String,
}

/// Internal response returned from AgentCoordinator to TalosService.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDispatchResult {
    pub content: String,
    #[serde(default)]
    pub output_items: Vec<RunItem>,
    #[serde(default)]
    pub reasoning: Vec<TalosReasoning>,
    pub stop_reason: StopReason,
    #[serde(default)]
    pub usage: TokenUsage,
    pub success: bool,
    pub model: String,
    pub session_id: String,
}
