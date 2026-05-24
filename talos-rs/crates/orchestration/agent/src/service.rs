use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use talos_agent_kernel::TurnPhase;
use talos_error::TalosResult;
use talos_types::gateway::GatewayResumeContext;
use talos_types::talos::{
    StopReason, TalosInputItem, TalosOutputItem, TalosReasoning, TalosTimelineEvent, TalosUsage,
};
use talos_types::TalosSession;
use tokio::sync::mpsc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorRequest {
    pub session: TalosSession,
    #[serde(default)]
    pub turn_id: Option<String>,
    #[serde(default)]
    pub resume_context: Option<GatewayResumeContext>,
    pub input: Vec<TalosInputItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTitleUpdate {
    pub title: String,
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
    #[serde(default)]
    pub session_title_update: Option<SessionTitleUpdate>,
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

#[async_trait]
pub trait AgentOrchestrator: Send + Sync {
    fn id(&self) -> &str;
    async fn run(&self, request: OrchestratorRequest) -> TalosResult<OrchestratorExecution>;
    async fn run_stream(
        &self,
        request: OrchestratorRequest,
    ) -> TalosResult<mpsc::Receiver<TalosTimelineEvent>>;
    fn take_session_title_update(
        &self,
        _session_id: &str,
        _turn_id: &str,
    ) -> Option<SessionTitleUpdate> {
        None
    }
    async fn compact_session(&self, session: TalosSession) -> TalosResult<TalosUsage>;
    async fn clear_session(&self, session: TalosSession) -> TalosResult<()>;
    async fn close_session(&self, session: TalosSession) -> TalosResult<()>;
    async fn drain_memory_flushes(&self, timeout: Duration) -> TalosResult<()>;
    fn pending_memory_flushes(&self) -> usize;
    async fn has_pending_memory_flush(&self, session_id: &str) -> bool;
    fn has_active_turn(&self, session_id: &str) -> bool;
    async fn cancel_active_turn(&self, session_id: &str) -> TalosResult<()>;
    async fn drain_active_turns(&self, timeout: Duration) -> TalosResult<()>;
    async fn submit_dynamic_tool_result(
        &self,
        session_id: &str,
        call_id: &str,
        tool_name: &str,
        output: String,
        success: bool,
    ) -> TalosResult<()>;
}
