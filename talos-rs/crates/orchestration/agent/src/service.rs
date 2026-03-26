use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use talos_agent_kernel::TurnPhase;
use talos_error::TalosResult;
use talos_types::talos::{TalosInputItem, TalosOutputItem, TalosReasoning, TalosUsage};
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
pub struct OrchestratorEvent {
    pub event_type: String,
    pub data: Value,
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
