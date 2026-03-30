use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use talos_agent_kernel::TurnPhase;
use talos_error::TalosResult;
use talos_types::talos::{
    TalosInputItem, TalosOutputItem, TalosReasoning, TalosUsage, TalosThinkingSource,
    TalosThinkingStage,
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
    pub stop_reason: String,
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
    MessageToolCallCompleted(OrchestratorMessageToolCallCompletedEvent),
    MessageCompleted(OrchestratorMessageCompletedEvent),
    MessageError(OrchestratorMessageErrorEvent),
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
    pub arguments: serde_json::Value,
    pub item_index: u64,
    pub iteration: u64,
    pub step_id: String,
    pub timing_ms: Option<u64>,
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
    pub stop_reason: String,
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
    async fn close_session(&self, session: TalosSession) -> TalosResult<()>;
}
