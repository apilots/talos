use crate::types::CoordinatorResponse;
use async_trait::async_trait;
use std::sync::Arc;
use talos_agent::AgentOrchestrator;
use talos_error::TalosResult;
use talos_types::{RequestHandler, SessionService};

#[async_trait]
pub trait Coordinator: RequestHandler + SessionService + Send + Sync {
    async fn handle_message(
        &self,
        channel: &str,
        user_id: &str,
        content: &str,
        model: Option<&str>,
    ) -> TalosResult<CoordinatorResponse>;
    async fn register_agent(
        &self,
        agent_id: &str,
        agent: Arc<dyn AgentOrchestrator>,
    ) -> TalosResult<()>;
}
