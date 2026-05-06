use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use talos_agent_kernel::TurnPhase;
use talos_error::TalosResult;
use talos_types::talos::{
    StopReason, TalosInputItem, TalosJsonValue, TalosOutputItem, TalosReasoning,
    TalosThinkingSource, TalosThinkingStage, TalosUsage,
};
use talos_types::TalosSession;
use tokio::sync::mpsc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorRequest {
    pub session: TalosSession,
    pub input: Vec<TalosInputItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorExecution {
    pub output_text: String,
    #[serde(default)]
    pub output_items: Vec<TalosOutputItem>,
    #[serde(default)]
    pub reasoning: Vec<TalosReasoning>,
    pub stop_reason: StopReason,
    #[serde(default)]
    pub usage: TalosUsage,
    pub task_count: u32,
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRequest {
    pub session: TalosSession,
    pub input: Vec<TalosInputItem>,
    #[serde(default)]
    pub phase: TurnPhase,
    pub planning_instruction: Option<String>,
    pub transfer_to: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrchestratorEvent {
    MessageStarted(OrchestratorMessageStartedEvent),
    MessageReasoningDelta(OrchestratorMessageReasoningDeltaEvent),
    MessageExecutionSummary(OrchestratorMessageExecutionSummaryEvent),
    MessageOutputDelta(OrchestratorMessageOutputDeltaEvent),
    MessageToolCallStarted(OrchestratorMessageToolCallStartedEvent),
    MessageToolCallArgsDelta(OrchestratorMessageToolCallArgsDeltaEvent),
    MessageToolCallQueued(OrchestratorMessageToolCallQueuedEvent),
    MessageToolCallExecuting(OrchestratorMessageToolCallExecutingEvent),
    MessageToolCallProgressDelta(OrchestratorMessageToolCallProgressDeltaEvent),
    MessageToolCallCompleted(OrchestratorMessageToolCallCompletedEvent),
    MessageToolCallPendingApproval(OrchestratorMessageToolCallPendingApprovalEvent),
    MessageToolCallPendingDynamicResult(OrchestratorMessageToolCallPendingDynamicResultEvent),
    MessageCompleted(OrchestratorMessageCompletedEvent),
    MessageError(OrchestratorMessageErrorEvent),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OrchestratorToolStreamKey {
    pub round: u32,
    pub index: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OrchestratorToolProgressKind {
    Status,
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorMessageToolCallPendingApprovalEvent {
    pub request_id: String,
    pub call_id: String,
    pub tool_name: String,
    pub prompt: String,
    pub arguments: TalosJsonValue,
    pub step_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorMessageToolCallPendingDynamicResultEvent {
    pub request_id: String,
    pub call_id: String,
    pub tool_name: String,
    pub arguments: TalosJsonValue,
    pub step_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorMessageStartedEvent {
    pub request_id: String,
    pub model: String,
    pub usage: TalosUsage,
    pub iteration: u64,
    pub step_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorMessageReasoningDeltaEvent {
    pub request_id: String,
    pub delta: String,
    pub source: TalosThinkingSource,
    pub stage: TalosThinkingStage,
    pub item_index: u64,
    pub iteration: u64,
    pub step_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorMessageExecutionSummaryEvent {
    pub request_id: String,
    pub text: String,
    pub item_index: u64,
    pub iteration: u64,
    pub step_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorMessageOutputDeltaEvent {
    pub request_id: String,
    pub delta: String,
    pub item_index: u64,
    pub iteration: u64,
    pub step_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorMessageToolCallStartedEvent {
    pub request_id: String,
    pub call_id: String,
    pub name: String,
    pub arguments: TalosJsonValue,
    pub item_index: u64,
    pub iteration: u64,
    pub step_id: String,
    pub timing_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorMessageToolCallArgsDeltaEvent {
    pub request_id: String,
    pub stream_key: OrchestratorToolStreamKey,
    pub call_id: Option<String>,
    pub tool_name: Option<String>,
    pub delta: String,
    pub item_index: u64,
    pub iteration: u64,
    pub step_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorMessageToolCallQueuedEvent {
    pub request_id: String,
    pub stream_key: OrchestratorToolStreamKey,
    pub call_id: String,
    pub tool_name: String,
    pub item_index: u64,
    pub iteration: u64,
    pub step_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorMessageToolCallExecutingEvent {
    pub request_id: String,
    pub stream_key: OrchestratorToolStreamKey,
    pub call_id: String,
    pub tool_name: String,
    pub item_index: u64,
    pub iteration: u64,
    pub step_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorMessageToolCallProgressDeltaEvent {
    pub request_id: String,
    pub stream_key: OrchestratorToolStreamKey,
    pub call_id: String,
    pub tool_name: String,
    pub message: String,
    pub progress_kind: OrchestratorToolProgressKind,
    pub item_index: u64,
    pub iteration: u64,
    pub step_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorMessageToolCallCompletedEvent {
    pub request_id: String,
    pub call_id: String,
    pub tool: String,
    pub content: Vec<talos_types::talos::TalosContentItem>,
    pub is_error: bool,
    pub item_index: u64,
    pub iteration: u64,
    pub step_id: String,
    pub timing_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorMessageCompletedEvent {
    pub request_id: String,
    pub stop_reason: StopReason,
    pub usage: TalosUsage,
    pub total_output_items: u64,
    pub iteration: u64,
    pub step_id: String,
    pub task_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorMessageErrorEvent {
    pub request_id: String,
    pub error: String,
}

#[async_trait]
pub trait AgentOrchestrator: Send + Sync {
    fn id(&self) -> &str;
    async fn run(&self, request: OrchestratorRequest) -> TalosResult<OrchestratorExecution>;
    async fn run_stream(
        &self,
        request: OrchestratorRequest,
    ) -> TalosResult<mpsc::Receiver<OrchestratorEvent>>;
    async fn compact_session(&self, session: TalosSession) -> TalosResult<TalosUsage>;
    async fn clear_session(&self, session: TalosSession) -> TalosResult<()>;
    async fn close_session(&self, session: TalosSession) -> TalosResult<()>;
    async fn drain_memory_flushes(&self, timeout: Duration) -> TalosResult<()>;
    fn pending_memory_flushes(&self) -> usize;
    async fn has_pending_memory_flush(&self, session_id: &str) -> bool;
    fn has_active_turn(&self, session_id: &str) -> bool;
    async fn cancel_active_turn(&self, session_id: &str) -> TalosResult<()>;
    async fn drain_active_turns(&self, timeout: Duration) -> TalosResult<()>;
    async fn submit_tool_approval(
        &self,
        session_id: &str,
        call_id: &str,
        tool_name: &str,
        approved: bool,
        reason: Option<String>,
    ) -> TalosResult<()>;
    async fn submit_dynamic_tool_result(
        &self,
        session_id: &str,
        call_id: &str,
        tool_name: &str,
        output: String,
        success: bool,
    ) -> TalosResult<()>;
}
