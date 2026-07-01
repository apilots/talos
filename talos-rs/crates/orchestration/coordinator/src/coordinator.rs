#[path = "maintenance.rs"]
mod maintenance;
#[path = "stream.rs"]
mod stream;
#[cfg(test)]
#[path = "coordinator_tests.rs"]
mod tests;

use crate::Coordinator;
use async_trait::async_trait;
use std::sync::Arc;
use talos_agent::{
    AgentHarness, AgentInput, AgentInputParams, AgentInterruptParams, AgentRunParams,
};
use talos_error::TalosResult;
use talos_router::Router;
use talos_types::agent_dispatch::{AgentDispatchParams, AgentDispatchResult};
use talos_types::talos::{AgentEvent, TalosContext};
use talos_types::{AgentDispatchRuntime, CancelActiveTurnParams, TalosSession};

use crate::{AgentRegistry, SessionRuntime};
use tokio::sync::mpsc;
use tracing::info;
use uuid::Uuid;

const DEFAULT_AGENT_ID: &str = "default";

/// Implementation of the Coordinator trait
pub struct CoordinatorImpl {
    /// Router instance for routing messages to appropriate agents
    router: Arc<dyn Router>,

    /// Registered agent harnesses and descriptors.
    registry: AgentRegistry,
}

impl CoordinatorImpl {
    /// Create a new instance of CoordinatorImpl
    pub fn new(router: Arc<dyn Router>) -> Self {
        Self {
            router,
            registry: AgentRegistry::new(),
        }
    }

    fn route_channel(context: &TalosContext) -> String {
        stream::route_channel(context)
    }

    async fn resolve_target_agent_id(&self, context: &TalosContext) -> TalosResult<String> {
        let route_channel = Self::route_channel(context);
        Ok(self
            .router
            .route(&route_channel, &context.user_id)
            .await?
            .unwrap_or_else(|| DEFAULT_AGENT_ID.to_string()))
    }

    fn message_id_for_dispatch(request: &AgentDispatchParams) -> String {
        if request.resume_context.is_some() {
            request.context.message_id.clone()
        } else {
            format!("msg_{}", Uuid::new_v4().simple())
        }
    }

    fn agent_run_params_for_dispatch(
        request: &AgentDispatchParams,
        message_id: String,
    ) -> AgentRunParams {
        let mut turn_session = request.session.clone();
        turn_session.context.message_id = message_id;
        if let Some(workspace_id) = request.workspace_id.clone() {
            turn_session.context.workspace_id = Some(workspace_id);
        }

        AgentRunParams {
            session: turn_session,
            turn_id: request.turn_id.clone(),
            resume_context: request.resume_context.clone(),
            input: request.input.clone(),
        }
    }

    /// Get the agent registry.
    pub fn registry(&self) -> AgentRegistry {
        self.registry.clone()
    }

    async fn resolve_session_agent(
        &self,
        session: &TalosSession,
    ) -> TalosResult<(String, Arc<dyn AgentHarness>)> {
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
        let target_agent = self.resolve_target_agent_id(&route_context).await?;
        let registration = self.registry.get(&target_agent).await?;
        Ok((target_agent, registration.harness))
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
    ) -> TalosResult<talos_types::TokenUsage> {
        let (_target_agent, agent) = self.resolve_session_agent(&session).await?;
        agent.compact_session(session).await
    }

    async fn clear_session_runtime(&self, session: TalosSession) -> TalosResult<()> {
        let (_target_agent, agent) = self.resolve_session_agent(&session).await?;
        agent.clear_session(session).await
    }

    async fn cancel_active_turn_for_session(
        &self,
        params: CancelActiveTurnParams,
    ) -> TalosResult<()> {
        let session_id = params.session.context.session_id.clone();
        let (_target_agent, agent) = self.resolve_session_agent(&params.session).await?;
        agent
            .interrupt(AgentInterruptParams {
                session_id,
                turn_id: params.turn_id,
            })
            .await
            .map(|_| ())
    }

    async fn submit_dynamic_tool_result_for_session(
        &self,
        session: TalosSession,
        turn_id: Option<&str>,
        call_id: &str,
        tool_name: &str,
        output: String,
        success: bool,
    ) -> TalosResult<()> {
        let (_target_agent, agent) = self.resolve_session_agent(&session).await?;
        agent
            .submit_input(AgentInputParams {
                session,
                turn_id: turn_id.map(str::to_string),
                input: AgentInput::DynamicToolResult {
                    call_id: call_id.to_string(),
                    tool_name: tool_name.to_string(),
                    output,
                    success,
                },
            })
            .await
            .map(|_| ())
    }
}

#[async_trait]
impl Coordinator for CoordinatorImpl {
    async fn register_agent(
        &self,
        agent_id: &str,
        agent: Arc<dyn AgentHarness>,
    ) -> TalosResult<()> {
        info!("Registering agent: {}", agent_id);

        self.registry.register(agent_id, agent).await
    }
}

#[async_trait]
impl AgentDispatchRuntime for CoordinatorImpl {
    async fn handle(&self, request: AgentDispatchParams) -> TalosResult<AgentDispatchResult> {
        let context = request.context.clone();
        let target_agent = self.resolve_target_agent_id(&context).await?;

        let session = request.session.clone();
        let model_for_turn = session.config.llm.model.clone();
        let agent = self.registry.get(&target_agent).await?.harness;

        let message_id = Self::message_id_for_dispatch(&request);
        let agent_response = agent
            .run(Self::agent_run_params_for_dispatch(&request, message_id))
            .await?;

        Ok(AgentDispatchResult {
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
        request: AgentDispatchParams,
    ) -> TalosResult<mpsc::Receiver<AgentEvent>> {
        let context = request.context.clone();
        let target_agent = self.resolve_target_agent_id(&context).await?;

        let agent = self.registry.get(&target_agent).await?.harness;

        let message_id = Self::message_id_for_dispatch(&request);
        let mut agent_event_stream = agent
            .run_streamed(Self::agent_run_params_for_dispatch(&request, message_id))
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

    async fn cancel_active_turn(&self, params: CancelActiveTurnParams) -> TalosResult<()> {
        self.cancel_active_turn_for_session(params).await
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
        self.submit_dynamic_tool_result_for_session(
            session,
            Some(turn_id),
            call_id,
            tool_name,
            output,
            success,
        )
        .await
    }

    async fn take_session_title_update(&self, session_id: &str, turn_id: &str) -> Option<String> {
        let registrations = self.registry.list().await;
        registrations
            .into_iter()
            .find_map(|registration| {
                registration
                    .harness
                    .take_session_title_update(session_id, turn_id)
            })
            .map(|update| update.title)
    }
}

#[async_trait]
impl SessionRuntime for CoordinatorImpl {
    async fn resolve_target_agent(&self, context: &TalosContext) -> TalosResult<String> {
        self.resolve_target_agent_id(context).await
    }

    async fn invalidate_cached_session(&self, session_id: &str) -> TalosResult<()> {
        let _ = session_id;
        Ok(())
    }

    async fn compact_session(&self, session: TalosSession) -> TalosResult<talos_types::TokenUsage> {
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

    async fn cancel_active_turn(&self, params: CancelActiveTurnParams) -> TalosResult<()> {
        self.cancel_active_turn_for_session(params).await
    }

    async fn submit_dynamic_tool_result(
        &self,
        session: TalosSession,
        call_id: &str,
        tool_name: &str,
        output: String,
        success: bool,
    ) -> TalosResult<()> {
        self.submit_dynamic_tool_result_for_session(
            session, None, call_id, tool_name, output, success,
        )
        .await
    }
}
