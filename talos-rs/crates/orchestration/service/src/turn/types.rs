use std::sync::Arc;

use async_trait::async_trait;
use talos_error::TalosResult;
use talos_types::agent_dispatch::AgentDispatchRequest;
use talos_types::talos::{TalosInputItem, TalosTimelineEvent};
use talos_types::{AgentDispatchRuntime, TalosSession};
use tokio::sync::mpsc;

#[async_trait]
pub trait TurnExecutor: Send + Sync {
    async fn start_turn_stream(
        &self,
        request: AgentDispatchRequest,
    ) -> TalosResult<mpsc::Receiver<TalosTimelineEvent>>;

    async fn cancel_active_turn(&self, session: TalosSession) -> TalosResult<()>;

    async fn submit_dynamic_tool_result(
        &self,
        session: TalosSession,
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
        request: AgentDispatchRequest,
    ) -> TalosResult<mpsc::Receiver<TalosTimelineEvent>> {
        self.handler.handle_stream(request).await
    }

    async fn cancel_active_turn(&self, session: TalosSession) -> TalosResult<()> {
        self.handler.cancel_active_turn(session).await
    }

    async fn submit_dynamic_tool_result(
        &self,
        session: TalosSession,
        call_id: &str,
        tool_name: &str,
        output: String,
        success: bool,
    ) -> TalosResult<()> {
        self.handler
            .submit_dynamic_tool_result(session, call_id, tool_name, output, success)
            .await
    }

    async fn take_session_title_update(&self, session_id: &str, turn_id: &str) -> Option<String> {
        self.handler
            .take_session_title_update(session_id, turn_id)
            .await
    }
}

#[derive(Debug, Clone)]
pub struct TurnRetryRequest {
    pub input: Vec<TalosInputItem>,
    pub workspace_id: Option<String>,
    pub client_message_id: String,
    pub retry_of_message_id: String,
}
