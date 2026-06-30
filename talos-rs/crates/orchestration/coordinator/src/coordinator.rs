#[path = "maintenance.rs"]
mod maintenance;
#[path = "stream.rs"]
mod stream;
#[cfg(test)]
#[path = "coordinator_tests.rs"]
mod tests;

use crate::Coordinator;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use talos_agent::{Agent, AgentRequest};
use talos_error::{TalosError, TalosResult};
use talos_router::Router;
use talos_types::agent_dispatch::{AgentDispatchRequest, AgentDispatchResponse};
use talos_types::talos::{TalosContext, TalosTimelineEvent};
use talos_types::{AgentDispatchRuntime, TalosSession};

use crate::SessionRuntime;
use tokio::sync::{mpsc, RwLock};
use tracing::info;
use uuid::Uuid;

/// Implementation of the Coordinator trait
pub struct CoordinatorImpl {
    /// Router instance for routing messages to appropriate agents
    router: Arc<dyn Router>,

    /// Registered agents map
    agents: Arc<RwLock<HashMap<String, Arc<dyn Agent>>>>,
}

impl CoordinatorImpl {
    /// Create a new instance of CoordinatorImpl
    pub fn new(router: Arc<dyn Router>) -> Self {
        Self {
            router,
            agents: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    fn route_channel(context: &TalosContext) -> String {
        stream::route_channel(context)
    }

    /// Get reference to registered agents
    pub fn agents(&self) -> Arc<RwLock<HashMap<String, Arc<dyn Agent>>>> {
        self.agents.clone()
    }

    async fn resolve_session_agent(
        &self,
        session: &TalosSession,
    ) -> TalosResult<(String, Arc<dyn Agent>)> {
        let route_context = TalosContext {
            source_type: session.context.source_type.clone(),
            provider: session.context.provider.clone(),
            user_id: session.context.user_id.clone(),
            message_id: session.context.message_id.clone(),
            trace_id: session.context.trace_id.clone(),
            channel_id: session.context.channel_id.clone(),
            thread_id: session.context.thread_id.clone(),
            project_root: session.context.project_root.clone(),
            environment: session.context.environment.clone(),
        };
        let route_channel = Self::route_channel(&route_context);
        let target_agent = self
            .router
            .route(&route_channel, &route_context.user_id)
            .await?
            .unwrap_or_else(|| "default".to_string());
        let agents = self.agents.read().await;
        let agent = agents
            .get(&target_agent)
            .cloned()
            .ok_or_else(|| talos_error::TalosError::AgentNotFound(target_agent.clone()))?;
        Ok((target_agent, agent))
    }

    async fn prepare_delete_session_runtime(&self, session: TalosSession) -> TalosResult<()> {
        let session_id = session.context.session_id.clone();
        let (_target_agent, agent) = self.resolve_session_agent(&session).await?;
        if agent.has_active_turn(&session_id) {
            return Err(talos_error::TalosError::ValidationError(format!(
                "cannot delete session `{}` while a turn is active",
                session_id
            )));
        }
        if agent.has_pending_memory_flush(&session_id).await {
            return Err(talos_error::TalosError::ValidationError(format!(
                "cannot delete session `{}` while memory flush is pending",
                session_id
            )));
        }
        agent.clear_session(session).await
    }

    async fn compact_session_runtime(
        &self,
        session: TalosSession,
    ) -> TalosResult<talos_types::TalosUsage> {
        let (_target_agent, agent) = self.resolve_session_agent(&session).await?;
        agent.compact_session(session).await
    }

    async fn clear_session_runtime(&self, session: TalosSession) -> TalosResult<()> {
        let (_target_agent, agent) = self.resolve_session_agent(&session).await?;
        agent.clear_session(session).await
    }

    async fn cancel_active_turn_for_session(&self, session: TalosSession) -> TalosResult<()> {
        let session_id = session.context.session_id.clone();
        let (_target_agent, agent) = self.resolve_session_agent(&session).await?;
        agent.cancel_active_turn(&session_id).await
    }

    async fn submit_dynamic_tool_result_for_session(
        &self,
        session: TalosSession,
        call_id: &str,
        tool_name: &str,
        output: String,
        success: bool,
    ) -> TalosResult<()> {
        let (_target_agent, agent) = self.resolve_session_agent(&session).await?;
        agent
            .submit_dynamic_tool_result(session, call_id, tool_name, output, success)
            .await
    }
}

#[async_trait]
impl Coordinator for CoordinatorImpl {
    async fn register_agent(&self, agent_id: &str, agent: Arc<dyn Agent>) -> TalosResult<()> {
        info!("Registering agent: {}", agent_id);

        let mut agents = self.agents.write().await;
        agents.insert(agent_id.to_string(), agent);

        Ok(())
    }
}

#[async_trait]
impl AgentDispatchRuntime for CoordinatorImpl {
    async fn handle(&self, request: AgentDispatchRequest) -> TalosResult<AgentDispatchResponse> {
        let context = request.context.clone();
        let route_channel = Self::route_channel(&context);

        let target_agent = self
            .router
            .route(&route_channel, &context.user_id)
            .await?
            .unwrap_or_else(|| "default".to_string());

        let session = request
            .session
            .as_ref()
            .map(|session| session.as_ref().clone())
            .ok_or_else(|| {
                TalosError::ValidationError(
                    "AgentDispatchRequest.session must be prepared by application service"
                        .to_string(),
                )
            })?;
        let model_for_turn = session.config.llm.model.clone();
        let agents = self.agents.read().await;
        let agent = agents
            .get(&target_agent)
            .cloned()
            .ok_or_else(|| talos_error::TalosError::AgentNotFound(target_agent.clone()))?;
        drop(agents);

        let message_id = if request.resume_context.is_some() {
            request.context.message_id.clone()
        } else {
            format!("msg_{}", Uuid::new_v4().simple())
        };
        let mut turn_session = session.clone();
        turn_session.context.message_id = message_id;
        if let Some(workspace_id) = request.workspace_id.clone() {
            turn_session.context.workspace_id = Some(workspace_id);
        }
        let agent_response = agent
            .run(AgentRequest {
                session: turn_session,
                turn_id: request.turn_id.clone(),
                resume_context: request.resume_context.clone(),
                input: request.input.clone(),
            })
            .await?;

        Ok(AgentDispatchResponse {
            content: agent_response.output_text,
            output_items: agent_response.output_items,
            reasoning: agent_response.reasoning,
            stop_reason: agent_response.stop_reason,
            usage: agent_response.usage,
            success: true,
            model: model_for_turn,
            session_id: session.context.session_id,
        })
    }

    async fn handle_stream(
        &self,
        request: AgentDispatchRequest,
    ) -> TalosResult<mpsc::Receiver<TalosTimelineEvent>> {
        let context = request.context.clone();
        let route_channel = Self::route_channel(&context);

        let target_agent = self
            .router
            .route(&route_channel, &context.user_id)
            .await?
            .unwrap_or_else(|| "default".to_string());

        let session = request
            .session
            .as_ref()
            .map(|session| session.as_ref().clone())
            .ok_or_else(|| {
                TalosError::ValidationError(
                    "AgentDispatchRequest.session must be prepared by application service"
                        .to_string(),
                )
            })?;

        let agents = self.agents.read().await;
        let agent = agents
            .get(&target_agent)
            .cloned()
            .ok_or_else(|| talos_error::TalosError::AgentNotFound(target_agent.clone()))?;
        drop(agents);

        let message_id = if request.resume_context.is_some() {
            request.context.message_id.clone()
        } else {
            format!("msg_{}", Uuid::new_v4().simple())
        };
        let mut turn_session = session.clone();
        turn_session.context.message_id = message_id.clone();
        if let Some(workspace_id) = request.workspace_id.clone() {
            turn_session.context.workspace_id = Some(workspace_id);
        }
        let mut agent_event_stream = agent
            .run_stream(AgentRequest {
                session: turn_session,
                turn_id: request.turn_id.clone(),
                resume_context: request.resume_context.clone(),
                input: request.input.clone(),
            })
            .await?;

        let (tx, rx) = mpsc::channel(256);
        tokio::spawn(async move {
            while let Some(event) = agent_event_stream.recv().await {
                if tx.send(event).await.is_err() {
                    return;
                }
            }
        });

        Ok(rx)
    }

    async fn cancel_active_turn(&self, session: TalosSession) -> TalosResult<()> {
        self.cancel_active_turn_for_session(session).await
    }

    async fn submit_dynamic_tool_result(
        &self,
        session: TalosSession,
        call_id: &str,
        tool_name: &str,
        output: String,
        success: bool,
    ) -> TalosResult<()> {
        self.submit_dynamic_tool_result_for_session(session, call_id, tool_name, output, success)
            .await
    }

    async fn take_session_title_update(&self, session_id: &str, turn_id: &str) -> Option<String> {
        let agents = self.agents.read().await;
        agents
            .values()
            .find_map(|agent| agent.take_session_title_update(session_id, turn_id))
            .map(|update| update.title)
    }
}

#[async_trait]
impl SessionRuntime for CoordinatorImpl {
    async fn resolve_target_agent(&self, context: &TalosContext) -> TalosResult<String> {
        let channel = Self::route_channel(context);
        Ok(self
            .router
            .route(&channel, &context.user_id)
            .await?
            .unwrap_or_else(|| "default".to_string()))
    }

    async fn invalidate_cached_session(&self, session_id: &str) -> TalosResult<()> {
        let _ = session_id;
        Ok(())
    }

    async fn compact_session(&self, session: TalosSession) -> TalosResult<talos_types::TalosUsage> {
        self.compact_session_runtime(session).await
    }

    async fn clear_session(&self, session: TalosSession) -> TalosResult<()> {
        self.clear_session_runtime(session).await
    }

    async fn close_session(&self, session: TalosSession) -> TalosResult<()> {
        let (_target_agent, agent) = self.resolve_session_agent(&session).await?;
        agent.close_session(session).await
    }

    async fn prepare_delete_session(&self, session: TalosSession) -> TalosResult<()> {
        self.prepare_delete_session_runtime(session).await
    }

    async fn cancel_active_turn(&self, session: TalosSession) -> TalosResult<()> {
        self.cancel_active_turn_for_session(session).await
    }

    async fn submit_dynamic_tool_result(
        &self,
        session: TalosSession,
        call_id: &str,
        tool_name: &str,
        output: String,
        success: bool,
    ) -> TalosResult<()> {
        self.submit_dynamic_tool_result_for_session(session, call_id, tool_name, output, success)
            .await
    }
}
