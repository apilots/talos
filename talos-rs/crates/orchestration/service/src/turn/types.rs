use std::sync::Arc;

use async_trait::async_trait;
use talos_error::TalosResult;
use talos_types::agent_dispatch::AgentDispatchParams;
use talos_types::talos::{AgentEvent, TalosInputItem};
use talos_types::{AgentDispatchRuntime, CancelActiveTurnParams, TalosSession};
use tokio::sync::mpsc;

#[async_trait]
pub trait TurnExecutor: Send + Sync {
    async fn start_turn_stream(
        &self,
        request: AgentDispatchParams,
    ) -> TalosResult<mpsc::Receiver<AgentEvent>>;

    async fn cancel_active_turn(&self, params: CancelActiveTurnParams) -> TalosResult<()>;

    async fn submit_dynamic_tool_result(
        &self,
        session: TalosSession,
        turn_id: &str,
        call_id: &str,
        tool_name: &str,
        output: String,
        success: bool,
    ) -> TalosResult<()>;

    async fn take_session_title_update(&self, _session_id: &str, _turn_id: &str) -> Option<String> {
        None
    }
}

#[derive(Clone)]
pub struct AgentDispatchTurnExecutor {
    handler: Arc<dyn AgentDispatchRuntime>,
}

impl AgentDispatchTurnExecutor {
    pub fn new(handler: Arc<dyn AgentDispatchRuntime>) -> Self {
        Self { handler }
    }
}

#[async_trait]
impl TurnExecutor for AgentDispatchTurnExecutor {
    async fn start_turn_stream(
        &self,
        request: AgentDispatchParams,
    ) -> TalosResult<mpsc::Receiver<AgentEvent>> {
        self.handler.handle_stream(request).await
    }

    async fn cancel_active_turn(&self, params: CancelActiveTurnParams) -> TalosResult<()> {
        self.handler.cancel_active_turn(params).await
    }

    async fn submit_dynamic_tool_result(
        &self,
        session: TalosSession,
        turn_id: &str,
        call_id: &str,
        tool_name: &str,
        output: String,
        success: bool,
    ) -> TalosResult<()> {
        self.handler
            .submit_dynamic_tool_result(session, turn_id, call_id, tool_name, output, success)
            .await
    }

    async fn take_session_title_update(&self, session_id: &str, turn_id: &str) -> Option<String> {
        self.handler
            .take_session_title_update(session_id, turn_id)
            .await
    }
}

#[derive(Debug, Clone)]
pub struct TurnRetryParams {
    pub input: Vec<TalosInputItem>,
    pub workspace_id: Option<String>,
    pub client_message_id: String,
    pub retry_of_message_id: String,
}
