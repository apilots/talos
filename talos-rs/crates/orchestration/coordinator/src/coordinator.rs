#[path = "maintenance.rs"]
mod maintenance;
#[path = "session.rs"]
mod session;
#[path = "stream.rs"]
mod stream;
#[cfg(test)]
#[path = "tests.rs"]
mod tests;

use crate::Coordinator;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use talos_agent::{AgentOrchestrator, OrchestratorRequest};
use talos_error::TalosResult;
use talos_router::Router;
use talos_session_store::SessionStore;
use talos_types::gateway::{GatewayRequest, GatewayResponse};
use talos_types::talos::{TalosContext, TalosTimelineEvent, TalosTimelineEventPayload, TalosUsage};
use talos_types::{
    ProjectPatch, ProjectRecord, RequestHandler, SessionConf, SessionPatch, SessionPatchAck,
    SessionQuery, SessionRuntime, SessionService, TalosConfig, TalosSession,
};
use tokio::sync::{mpsc, RwLock};
use tracing::{info, warn};
use uuid::Uuid;

use self::session::SessionManager;

/// Implementation of the Coordinator trait
pub struct CoordinatorImpl {
    /// Router instance for routing messages to appropriate agents
    router: Arc<dyn Router>,

    /// Session lifecycle owner
    sessions: Arc<SessionManager>,

    /// Registered agents map
    agents: Arc<RwLock<HashMap<String, Arc<dyn AgentOrchestrator>>>>,
}

impl CoordinatorImpl {
    /// Create a new instance of CoordinatorImpl
    pub fn new(
        router: Arc<dyn Router>,
        session_store: Arc<dyn SessionStore>,
        base_config: TalosConfig,
    ) -> Self {
        Self {
            router,
            sessions: Arc::new(SessionManager::new(session_store, base_config)),
            agents: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    fn route_channel(context: &TalosContext) -> String {
        stream::route_channel(context)
    }

    /// Get reference to registered agents
    pub fn agents(&self) -> Arc<RwLock<HashMap<String, Arc<dyn AgentOrchestrator>>>> {
        self.agents.clone()
    }

    async fn resolve_session_agent(
        &self,
        session: &TalosSession,
    ) -> TalosResult<(String, Arc<dyn AgentOrchestrator>)> {
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
}

#[async_trait]
impl Coordinator for CoordinatorImpl {
    async fn register_agent(
        &self,
        agent_id: &str,
        agent: Arc<dyn AgentOrchestrator>,
    ) -> TalosResult<()> {
        info!("Registering agent: {}", agent_id);

        let mut agents = self.agents.write().await;
        agents.insert(agent_id.to_string(), agent);

        Ok(())
    }
}

#[async_trait]
impl RequestHandler for CoordinatorImpl {
    async fn handle(&self, request: GatewayRequest) -> TalosResult<GatewayResponse> {
        let context = request.context.clone();
        let route_channel = Self::route_channel(&context);

        let target_agent = self
            .router
            .route(&route_channel, &context.user_id)
            .await?
            .unwrap_or_else(|| "default".to_string());

        let session = self.sessions.resolve_session(&request).await?;
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
        let agent_response = agent
            .run(OrchestratorRequest {
                session: turn_session,
                turn_id: request.turn_id.clone(),
                resume_context: request.resume_context.clone(),
                input: request.input.clone(),
            })
            .await?;

        self.sessions
            .record_turn_completion(
                &session.context.session_id,
                &model_for_turn,
                2,
                &agent_response.usage,
            )
            .await?;
        if let Some(title_update) = agent_response.session_title_update.as_ref() {
            if let Err(error) = self
                .sessions
                .update_session_title(&session.context.session_id, &title_update.title)
                .await
            {
                warn!(
                    session_id = %session.context.session_id,
                    ?error,
                    "failed to persist session title after non-stream turn"
                );
            }
        }

        Ok(GatewayResponse {
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
        request: GatewayRequest,
    ) -> TalosResult<mpsc::Receiver<TalosTimelineEvent>> {
        let context = request.context.clone();
        let route_channel = Self::route_channel(&context);

        let target_agent = self
            .router
            .route(&route_channel, &context.user_id)
            .await?
            .unwrap_or_else(|| "default".to_string());

        let session = self.sessions.resolve_session(&request).await?;
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
        turn_session.context.message_id = message_id.clone();
        let mut orchestrator_stream = agent
            .run_stream(OrchestratorRequest {
                session: turn_session,
                turn_id: request.turn_id.clone(),
                resume_context: request.resume_context.clone(),
                input: request.input.clone(),
            })
            .await?;

        let sessions = self.sessions.clone();
        let session_id = session.context.session_id.clone();
        let response_model = model_for_turn.clone();
        let (tx, rx) = mpsc::channel(256);
        tokio::spawn(async move {
            let mut final_usage = TalosUsage::default();
            let mut final_message_delta = 2;
            let mut final_turn_id = None;

            while let Some(event) = orchestrator_stream.recv().await {
                if let TalosTimelineEventPayload::TurnCompleted(completed) = &event.payload {
                    final_usage = completed.usage.clone();
                    final_message_delta = 2;
                    final_turn_id = Some(completed.turn_id.clone());
                }

                if tx.send(event).await.is_err() {
                    return;
                }
            }

            sessions
                .record_stream_completion(
                    &session_id,
                    &response_model,
                    final_message_delta,
                    &final_usage,
                )
                .await;
            if let Some(turn_id) = final_turn_id.as_deref() {
                if let Some(title_update) = agent.take_session_title_update(&session_id, turn_id) {
                    if let Err(error) = sessions
                        .update_session_title(&session_id, &title_update.title)
                        .await
                    {
                        warn!(
                            session_id = %session_id,
                            ?error,
                            "failed to persist session title after stream turn"
                        );
                    }
                }
            }
        });

        Ok(rx)
    }

    async fn cancel_active_turn(&self, session_id: &str) -> TalosResult<()> {
        let Some(session) = self
            .sessions
            .load_session(
                session_id,
                Some(self.sessions.default_workspace_id().to_string()),
            )
            .await?
        else {
            return Err(talos_error::TalosError::ResourceNotFound(format!(
                "session {}",
                session_id
            )));
        };
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
        let Some(agent) = agents.get(&target_agent).cloned() else {
            return Ok(());
        };
        drop(agents);
        agent.cancel_active_turn(session_id).await
    }

    async fn submit_dynamic_tool_result(
        &self,
        session_id: &str,
        call_id: &str,
        tool_name: &str,
        output: String,
        success: bool,
    ) -> TalosResult<()> {
        let session = self
            .sessions
            .get_session(session_id)
            .await?
            .ok_or_else(|| {
                talos_error::TalosError::ResourceNotFound(format!("session {}", session_id))
            })?;
        let (_target_agent, agent) = self.resolve_session_agent(&session).await?;
        agent
            .submit_dynamic_tool_result(session_id, call_id, tool_name, output, success)
            .await
    }
}

#[async_trait]
impl SessionRuntime for CoordinatorImpl {
    async fn resolve_target_agent(&self, context: &TalosContext) -> TalosResult<String> {
        SessionService::resolve_target_agent(self, context).await
    }

    async fn invalidate_cached_session(&self, session_id: &str) -> TalosResult<()> {
        SessionService::invalidate_cached_session(self, session_id).await
    }

    async fn compact_session(&self, session_id: &str) -> TalosResult<TalosSession> {
        SessionService::compact_session(self, session_id).await
    }

    async fn clear_session(&self, session_id: &str) -> TalosResult<TalosSession> {
        SessionService::clear_session(self, session_id).await
    }

    async fn close_session(&self, session_id: &str) -> TalosResult<()> {
        SessionService::close_session(self, session_id).await
    }

    async fn delete_session(&self, session_id: &str) -> TalosResult<SessionPatchAck> {
        SessionService::delete_session(self, session_id).await
    }

    async fn cancel_active_turn(&self, session_id: &str) -> TalosResult<()> {
        SessionService::cancel_active_turn(self, session_id).await
    }

    async fn submit_dynamic_tool_result(
        &self,
        session_id: &str,
        call_id: &str,
        tool_name: &str,
        output: String,
        success: bool,
    ) -> TalosResult<()> {
        SessionService::submit_dynamic_tool_result(
            self, session_id, call_id, tool_name, output, success,
        )
        .await
    }
}

#[async_trait]
impl SessionService for CoordinatorImpl {
    async fn resolve_target_agent(&self, context: &TalosContext) -> TalosResult<String> {
        let channel = Self::route_channel(context);
        Ok(self
            .router
            .route(&channel, &context.user_id)
            .await?
            .unwrap_or_else(|| "default".to_string()))
    }

    async fn create_session(
        &self,
        context: TalosContext,
        model: Option<String>,
        conf: Option<SessionConf>,
        project_id: Option<String>,
        current_directory: std::option::Option<std::path::PathBuf>,
    ) -> TalosResult<TalosSession> {
        let target_agent = SessionService::resolve_target_agent(self, &context).await?;

        let record = self
            .sessions
            .create_session(
                context,
                &target_agent,
                model,
                conf,
                project_id,
                current_directory,
            )
            .await?;
        Ok(record)
    }

    async fn list_sessions(&self, query: SessionQuery) -> TalosResult<Vec<TalosSession>> {
        self.sessions.list_sessions(query).await
    }

    async fn count_sessions(&self, query: SessionQuery) -> TalosResult<usize> {
        self.sessions.count_sessions(query).await
    }

    async fn get_session(&self, session_id: &str) -> TalosResult<Option<TalosSession>> {
        self.sessions.get_session(session_id).await
    }

    async fn invalidate_cached_session(&self, session_id: &str) -> TalosResult<()> {
        self.sessions.invalidate_cached_session(session_id).await;
        Ok(())
    }

    async fn patch_session(
        &self,
        session_id: &str,
        patch: SessionPatch,
    ) -> TalosResult<SessionPatchAck> {
        self.sessions.patch_session(session_id, patch).await
    }

    async fn compact_session(&self, session_id: &str) -> TalosResult<TalosSession> {
        let session = self
            .sessions
            .get_session(session_id)
            .await?
            .ok_or_else(|| {
                talos_error::TalosError::ResourceNotFound(format!("session {}", session_id))
            })?;
        let (_target_agent, agent) = self.resolve_session_agent(&session).await?;
        let usage = agent.compact_session(session.clone()).await?;
        self.sessions
            .record_session_usage(
                &session.context.session_id,
                &session.config.llm.model,
                &usage,
            )
            .await?;
        self.sessions.get_session(session_id).await?.ok_or_else(|| {
            talos_error::TalosError::ResourceNotFound(format!("session {}", session_id))
        })
    }

    async fn clear_session(&self, session_id: &str) -> TalosResult<TalosSession> {
        let session = self
            .sessions
            .get_session(session_id)
            .await?
            .ok_or_else(|| {
                talos_error::TalosError::ResourceNotFound(format!("session {}", session_id))
            })?;
        let (_target_agent, agent) = self.resolve_session_agent(&session).await?;
        agent.clear_session(session.clone()).await?;
        let ack = self.sessions.reset_session(session_id).await?;
        if !ack.success {
            return Err(talos_error::TalosError::ResourceNotFound(format!(
                "session {}",
                session_id
            )));
        }
        self.sessions.get_session(session_id).await?.ok_or_else(|| {
            talos_error::TalosError::ResourceNotFound(format!("session {}", session_id))
        })
    }

    async fn close_session(&self, session_id: &str) -> TalosResult<()> {
        let Some(session) = self.sessions.load_session_for_close(session_id).await? else {
            return Err(talos_error::TalosError::ResourceNotFound(format!(
                "session {}",
                session_id
            )));
        };
        let (_target_agent, agent) = self.resolve_session_agent(&session).await?;
        self.sessions.invalidate_cached_session(session_id).await;
        agent.close_session(session).await?;
        Ok(())
    }

    async fn delete_session(&self, session_id: &str) -> TalosResult<SessionPatchAck> {
        let session = self
            .sessions
            .get_session(session_id)
            .await?
            .ok_or_else(|| {
                talos_error::TalosError::ResourceNotFound(format!("session {}", session_id))
            })?;
        let (_target_agent, agent) = self.resolve_session_agent(&session).await?;
        if agent.has_active_turn(session_id) {
            return Err(talos_error::TalosError::ValidationError(format!(
                "cannot delete session `{}` while a turn is active",
                session_id
            )));
        }
        if agent.has_pending_memory_flush(session_id).await {
            return Err(talos_error::TalosError::ValidationError(format!(
                "cannot delete session `{}` while memory flush is pending",
                session_id
            )));
        }
        agent.clear_session(session).await?;
        let ack = self.sessions.delete_session(session_id).await?;
        if !ack.success {
            return Err(talos_error::TalosError::ResourceNotFound(format!(
                "session {}",
                session_id
            )));
        }
        Ok(ack)
    }

    async fn cancel_active_turn(&self, session_id: &str) -> TalosResult<()> {
        RequestHandler::cancel_active_turn(self, session_id).await
    }

    async fn submit_dynamic_tool_result(
        &self,
        session_id: &str,
        call_id: &str,
        tool_name: &str,
        output: String,
        success: bool,
    ) -> TalosResult<()> {
        let session = self
            .sessions
            .get_session(session_id)
            .await?
            .ok_or_else(|| {
                talos_error::TalosError::ResourceNotFound(format!("session {}", session_id))
            })?;
        let (_target_agent, agent) = self.resolve_session_agent(&session).await?;
        agent
            .submit_dynamic_tool_result(session_id, call_id, tool_name, output, success)
            .await?;
        Ok(())
    }

    async fn list_projects(&self) -> TalosResult<Vec<ProjectRecord>> {
        self.sessions.list_projects().await
    }

    async fn get_project(&self, project_id: &str) -> TalosResult<Option<ProjectRecord>> {
        self.sessions.get_project(project_id).await
    }

    async fn create_project(
        &self,
        name: String,
        directory: std::path::PathBuf,
        user_id: String,
    ) -> TalosResult<ProjectRecord> {
        self.sessions.create_project(name, directory, user_id).await
    }

    async fn patch_project(
        &self,
        project_id: &str,
        patch: ProjectPatch,
    ) -> TalosResult<Option<ProjectRecord>> {
        self.sessions.patch_project(project_id, patch).await
    }

    async fn delete_project(&self, project_id: &str) -> TalosResult<()> {
        self.sessions.delete_project(project_id).await
    }
}
