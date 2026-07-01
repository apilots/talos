use std::sync::Arc;

use async_trait::async_trait;
use talos_error::TalosResult;
use talos_types::agent_dispatch::{AgentDispatchInputParams, AgentDispatchParams};
use talos_types::talos::{AgentEvent, TalosInputItem};
use talos_types::AgentDispatchRuntime;
use tokio::sync::mpsc;

#[async_trait]
pub trait TurnExecutor: Send + Sync {
    async fn start_turn_stream(
        &self,
        request: AgentDispatchParams,
    ) -> TalosResult<mpsc::Receiver<AgentEvent>>;

    async fn submit_input(&self, params: AgentDispatchInputParams) -> TalosResult<()>;

    async fn take_generated_session_title(
        &self,
        _session_id: &str,
        _turn_id: &str,
    ) -> Option<String> {
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

    async fn submit_input(&self, params: AgentDispatchInputParams) -> TalosResult<()> {
        self.handler.submit_input(params).await
    }

    async fn take_generated_session_title(
        &self,
        session_id: &str,
        turn_id: &str,
    ) -> Option<String> {
        self.handler
            .take_generated_session_title(session_id, turn_id)
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
