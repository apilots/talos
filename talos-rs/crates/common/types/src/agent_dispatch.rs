use crate::talos::{RunItem, StopReason, TalosInputItem, TalosReasoning, TokenUsage};
use crate::TalosSession;
use serde::{Deserialize, Serialize};

/// Deterministic Agent selection at the Service-to-Coordinator boundary.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DispatchBinding {
    /// Apply the existing Coordinator routing policy.
    Routed,
    /// Dispatch only to the named registered Agent.
    Exact { agent_id: String },
}

/// Internal request dispatched from TalosService to AgentCoordinator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDispatchParams {
    /// Talos turn id; this is also the internal Agent run identity.
    pub turn_id: String,
    /// Exhaustive routed or exact Agent binding selected by trusted Service logic.
    pub binding: DispatchBinding,
    /// Prepared product session used as the single identity and context source.
    ///
    /// Service must merge per-turn product context, such as workspace override
    /// or resume message id, into this session before crossing Coordinator.
    pub session: TalosSession,
    #[serde(default)]
    pub resume_context: Option<AgentDispatchResumeContext>,
    pub input: Vec<TalosInputItem>,
}

/// Internal input request dispatched from TalosService to AgentCoordinator.
///
/// This is the Service -> Coordinator input bridge. New input kinds should
/// extend AgentDispatchInput instead of adding parallel submit_* methods.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDispatchInputParams {
    /// Exhaustive routed or exact Agent binding selected by trusted Service logic.
    pub binding: DispatchBinding,
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
    /// Message delivered to the exact currently running Agent turn.
    Message { message: String },
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
    pub output_text: String,
    #[serde(default)]
    pub output_items: Vec<RunItem>,
    #[serde(default)]
    pub reasoning: Vec<TalosReasoning>,
    pub stop_reason: StopReason,
    #[serde(default)]
    pub usage: TokenUsage,
    pub model: String,
    pub session_id: String,
}
