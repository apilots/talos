use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use talos_agent::{
    AgentAdmission, AgentContext, AgentHarness, AgentInputParams, AgentInterruptParams,
    AgentRunExecution, AgentRunParams, AgentSessionContext,
};
use talos_agent_api::AgentInterruptOutcome;
use talos_error::{TalosError, TalosResult};
use talos_router::Router;

use crate::{AgentCoordinator, AgentRegistry, AgentRouter, AgentSelectionParams};

/// Default Coordinator that routes stable Agent bindings to registered Harnesses.
pub struct CoordinatorImpl {
    registry: AgentRegistry,
    agent_router: AgentRouter,
}

impl CoordinatorImpl {
    /// Create an empty Coordinator using the configured new-Session router.
    pub fn new(router: Arc<dyn Router>) -> Self {
        let registry = AgentRegistry::new();
        let agent_router = AgentRouter::new(router, registry.clone());
        Self {
            registry,
            agent_router,
        }
    }

    /// Register one already-constructed Harness at application composition.
    pub async fn register_agent(
        &self,
        agent_id: String,
        harness: Arc<dyn AgentHarness>,
    ) -> TalosResult<()> {
        self.registry.register(agent_id, harness).await
    }

    async fn harness(&self, agent_id: &str) -> TalosResult<Arc<dyn AgentHarness>> {
        if agent_id.trim().is_empty() {
            return Err(TalosError::internal("Agent binding cannot be empty"));
        }
        self.registry.get(agent_id).await
    }

    async fn context_harness(&self, context: &AgentContext) -> TalosResult<Arc<dyn AgentHarness>> {
        self.harness(&context.session.agent_id).await
    }
}

#[async_trait]
impl AgentCoordinator for CoordinatorImpl {
    async fn resolve_agent(&self, params: AgentSelectionParams) -> TalosResult<String> {
        self.agent_router.resolve_agent(params).await
    }

    async fn ensure_agent(&self, agent_id: &str) -> TalosResult<()> {
        self.harness(agent_id).await.map(|_| ())
    }

    async fn start(&self, params: AgentRunParams) -> TalosResult<AgentRunExecution> {
        let harness = self.context_harness(&params.context).await?;
        harness.start(params).await
    }

    async fn submit_input(&self, params: AgentInputParams) -> TalosResult<AgentAdmission> {
        let harness = self.harness(&params.agent_id).await?;
        harness.submit_input(params).await
    }

    async fn interrupt(&self, params: AgentInterruptParams) -> TalosResult<AgentInterruptOutcome> {
        let harness = self.harness(&params.agent_id).await?;
        harness.interrupt(params).await
    }

    async fn compact_session(&self, context: AgentContext) -> TalosResult<AgentRunExecution> {
        let harness = self.context_harness(&context).await?;
        harness.compact_session(context).await
    }

    async fn clear_session(&self, params: AgentSessionContext) -> TalosResult<()> {
        let harness = self.harness(&params.agent_id).await?;
        harness.clear_session(params).await
    }

    async fn close_session(&self, params: AgentSessionContext) -> TalosResult<()> {
        let harness = self.harness(&params.agent_id).await?;
        harness.close_session(params).await
    }

    async fn shutdown(&self, timeout: Duration) -> TalosResult<()> {
        let deadline = Instant::now() + timeout;
        for harness in self.registry.list().await {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or_else(|| {
                    TalosError::new(
                        talos_error::ErrorCode::Timeout,
                        "Agent Coordinator shutdown timed out",
                    )
                })?;
            harness.shutdown(remaining).await?;
        }
        Ok(())
    }
}
