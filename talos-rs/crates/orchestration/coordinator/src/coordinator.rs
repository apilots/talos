#[path = "maintenance.rs"]
mod maintenance;
#[cfg(test)]
#[path = "coordinator_tests.rs"]
mod tests;

use crate::Coordinator;
use async_trait::async_trait;
use std::sync::Arc;
use talos_agent::{
    AgentHarness, AgentInput, AgentInputParams, AgentInterruptParams, AgentResult,
    AgentResumeParams, AgentRunConfig, AgentRunContext, AgentRunOptions, AgentRunParams,
    AgentRunSessionState,
};
use talos_error::TalosResult;
use talos_router::Router;
use talos_types::agent_dispatch::{
    AgentDispatchInput, AgentDispatchInputParams, AgentDispatchParams, AgentDispatchResult,
    DispatchBinding,
};
use talos_types::talos::{AgentEvent, TalosContext};
use talos_types::{AgentDispatchRuntime, CancelActiveTurnParams, TalosSession};

use crate::{AgentRegistry, AgentRouter, AgentSelectionParams, SessionRuntime};
use tokio::sync::mpsc;
use tracing::info;
use uuid::Uuid;

/// Default Coordinator implementation for Agent routing and dispatch.
///
/// CoordinatorImpl owns registry lookup and routing decisions while delegating
/// execution to AgentHarness. It must not depend on concrete AgentRuntime
/// implementations or Service store records.
pub struct CoordinatorImpl {
    /// Registered agent harnesses and descriptors.
    registry: AgentRegistry,

    /// Coordinator-owned Agent selection framework.
    agent_router: AgentRouter,
}

impl CoordinatorImpl {
    /// Create a Coordinator with an external routing policy implementation.
    pub fn new(router: Arc<dyn Router>) -> Self {
        let registry = AgentRegistry::new();
        let agent_router =
            AgentRouter::new(router, registry.clone(), crate::router::DEFAULT_AGENT_ID);
        Self {
            registry,
            agent_router,
        }
    }

    fn map_agent_result<T>(result: AgentResult<T>) -> TalosResult<T> {
        result.map_err(|error| error.into_talos_error())
    }

    async fn resolve_target_agent_id(&self, context: &TalosContext) -> TalosResult<String> {
        Ok(self
            .agent_router
            .resolve_agent(AgentSelectionParams::for_context(context.clone()))
            .await?
            .agent_id)
    }

    fn message_id_for_dispatch(request: &AgentDispatchParams) -> String {
        if request.resume_context.is_some() {
            request.session.context.message_id.clone()
        } else {
            format!("msg_{}", Uuid::new_v4().simple())
        }
    }

    fn agent_run_params_for_dispatch(
        request: &AgentDispatchParams,
        message_id: String,
        agent_id: String,
    ) -> TalosResult<AgentRunParams> {
        let turn_id = request.turn_id.clone();
        let mut turn_session = request.session.clone();
        turn_session.context.message_id = message_id;

        let context = AgentRunContext::from_session(&turn_session, turn_id, agent_id);
        let config = AgentRunConfig::from_session(&turn_session);
        let session_state = AgentRunSessionState::from_session(&turn_session);
        let options = AgentRunOptions::from_config(&config);
        let mut params = AgentRunParams::new(
            context,
            config,
            session_state,
            options,
            request.input.clone(),
        );
        if let Some(resume_context) = request.resume_context.clone() {
            params = params.with_resume_context(resume_context);
        }
        Ok(params)
    }

    /// Return the Coordinator-owned Agent registry handle.
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
        let selection = self
            .agent_router
            .resolve_agent(AgentSelectionParams::for_context(route_context))
            .await?;
        Ok((selection.agent_id, selection.registration.harness))
    }

    async fn resolve_bound_agent(
        &self,
        binding: &DispatchBinding,
        session: &TalosSession,
    ) -> TalosResult<(String, Arc<dyn AgentHarness>)> {
        match binding {
            DispatchBinding::Exact { agent_id } => {
                let registration = Self::map_agent_result(self.registry.get(agent_id).await)?;
                Ok((agent_id.clone(), registration.harness))
            }
            DispatchBinding::Routed => self.resolve_session_agent(session).await,
        }
    }

    async fn prepare_delete_session_runtime(&self, session: TalosSession) -> TalosResult<()> {
        let session_id = session.context.session_id.clone();
        let (_target_agent, agent) = self.resolve_session_agent(&session).await?;
        if agent.has_active_turn(&session_id) {
            return Err(talos_error::TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                format!(
                    "cannot delete session `{}` while a turn is active",
                    session_id
                ),
            ));
        }
        agent.clear_session(session).await
    }

    async fn compact_session_runtime(
        &self,
        params: talos_agent::AgentSessionCompactionParams,
    ) -> TalosResult<talos_types::TokenUsage> {
        let (_target_agent, agent) = self.resolve_session_agent(&params.session).await?;
        agent.compact_session(params).await
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
        let turn_id = params.turn_id;
        let (_agent_id, agent) = self
            .resolve_bound_agent(&params.binding, &params.session)
            .await?;
        let interrupt_params = AgentInterruptParams {
            session_id,
            turn_id,
        };
        Self::map_agent_result(agent.validate_interrupt_request(&interrupt_params))?;
        Self::map_agent_result(agent.interrupt(interrupt_params).await).map(|_| ())
    }

    async fn submit_input_for_session(&self, params: AgentDispatchInputParams) -> TalosResult<()> {
        let (_agent_id, agent) = self
            .resolve_bound_agent(&params.binding, &params.session)
            .await?;
        let input = match params.input {
            AgentDispatchInput::UserActionResponse {
                call_id,
                tool_name,
                response,
            } => AgentInput::UserActionResponse {
                call_id,
                tool_name,
                response,
            },
            AgentDispatchInput::Message { input } => AgentInput::Message { input },
        };
        let input_params = AgentInputParams {
            session: params.session,
            turn_id: params.turn_id,
            input,
        };
        Self::map_agent_result(agent.validate_input_request(&input_params))?;
        Self::map_agent_result(agent.submit_input(input_params).await).map(|_| ())
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

        Self::map_agent_result(self.registry.register(agent_id, agent).await)
    }
}

#[async_trait]
impl AgentDispatchRuntime for CoordinatorImpl {
    async fn validate_agent_id(&self, agent_id: &str) -> TalosResult<()> {
        Self::map_agent_result(self.registry.get(agent_id).await).map(|_| ())
    }

    async fn handle(&self, request: AgentDispatchParams) -> TalosResult<AgentDispatchResult> {
        let session = request.session.clone();
        let model_for_turn = session.config.llm.model.clone();
        let (target_agent, agent) = self
            .resolve_bound_agent(&request.binding, &request.session)
            .await?;

        let message_id = Self::message_id_for_dispatch(&request);
        let agent_run_params =
            Self::agent_run_params_for_dispatch(&request, message_id, target_agent)?;
        let agent_response = if agent_run_params.resume_context.is_some() {
            let resume_params = AgentResumeParams::new(agent_run_params);
            Self::map_agent_result(agent.validate_resume_request(&resume_params))?;
            Self::map_agent_result(agent.resume(resume_params).await)?
        } else {
            Self::map_agent_result(agent.validate_run_request(&agent_run_params))?;
            Self::map_agent_result(agent.run(agent_run_params).await)?
        };

        Ok(AgentDispatchResult {
            output_text: agent_response.output_text,
            output_items: agent_response.output_items,
            reasoning: agent_response.reasoning,
            stop_reason: agent_response.stop_reason,
            usage: agent_response.usage,
            model: model_for_turn,
            session_id: session.context.session_id,
        })
    }

    async fn handle_stream(
        &self,
        request: AgentDispatchParams,
    ) -> TalosResult<mpsc::Receiver<AgentEvent>> {
        let (target_agent, agent) = self
            .resolve_bound_agent(&request.binding, &request.session)
            .await?;

        let message_id = Self::message_id_for_dispatch(&request);
        let agent_run_params =
            Self::agent_run_params_for_dispatch(&request, message_id, target_agent)?;
        let mut agent_event_stream = if agent_run_params.resume_context.is_some() {
            let resume_params = AgentResumeParams::new(agent_run_params);
            Self::map_agent_result(agent.validate_streaming_resume_request(&resume_params))?;
            Self::map_agent_result(agent.resume_streamed(resume_params).await)?
        } else {
            Self::map_agent_result(agent.validate_streaming_run_request(&agent_run_params))?;
            Self::map_agent_result(agent.run_streamed(agent_run_params).await)?
        };

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

    async fn submit_input(&self, params: AgentDispatchInputParams) -> TalosResult<()> {
        self.submit_input_for_session(params).await
    }

    async fn take_generated_session_title(
        &self,
        session_id: &str,
        turn_id: &str,
    ) -> Option<String> {
        let registrations = self.registry.list().await;
        registrations.into_iter().find_map(|registration| {
            registration
                .harness
                .take_generated_session_title(session_id, turn_id)
        })
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

    async fn compact_session(
        &self,
        params: talos_agent::AgentSessionCompactionParams,
    ) -> TalosResult<talos_types::TokenUsage> {
        self.compact_session_runtime(params).await
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

    async fn has_active_turn(&self, session: &TalosSession) -> TalosResult<bool> {
        let session_id = session.context.session_id.as_str();
        let (_agent_id, agent) = self.resolve_session_agent(session).await?;
        Ok(agent.has_active_turn(session_id))
    }
}
