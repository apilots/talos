use crate::Coordinator;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use talos_agent::{AgentOrchestrator, OrchestratorEvent, OrchestratorRequest};
use talos_error::TalosResult;
use talos_router::Router;
use talos_session_store::SessionStore;
use talos_types::gateway::{GatewayRequest, GatewayResponse};
use talos_types::talos::{
    TalosContext, TalosEvent, TalosEventPayload, TalosEventType, TalosMessageCompletedEvent,
    TalosMessageErrorEvent, TalosMessageExecutionSummaryEvent, TalosMessageOutputDeltaEvent,
    TalosMessageReasoningDeltaEvent, TalosMessageStartedEvent, TalosMessageToolCallCompletedEvent,
    TalosMessageToolCallPendingApprovalEvent, TalosMessageToolCallStartedEvent, TalosProvider,
    TalosUsage,
};
use talos_types::{
    RequestHandler, SessionPatch, SessionPatchAck, SessionQuery, SessionRecord, SessionService,
    TalosConfig, TalosSession,
};
use tokio::sync::{mpsc, RwLock};
use tracing::info;
use uuid::Uuid;

#[derive(Clone)]
struct SessionManager {
    session_store: Arc<dyn SessionStore>,
    session_cache: Arc<RwLock<HashMap<String, TalosSession>>>,
    base_config: TalosConfig,
    default_workspace_id: String,
    channel_dm_scopes: HashMap<String, String>,
}

impl SessionManager {
    fn new(session_store: Arc<dyn SessionStore>, base_config: TalosConfig) -> Self {
        Self {
            session_store,
            session_cache: Arc::new(RwLock::new(HashMap::new())),
            base_config,
            default_workspace_id: "default".to_string(),
            channel_dm_scopes: HashMap::new(),
        }
    }

    fn with_default_workspace_id(mut self, workspace_id: impl Into<String>) -> Self {
        self.default_workspace_id = workspace_id.into();
        self
    }

    fn with_channel_dm_scope(
        mut self,
        channel_id: impl Into<String>,
        dm_scope: impl Into<String>,
    ) -> Self {
        self.channel_dm_scopes
            .insert(channel_id.into(), dm_scope.into());
        self
    }

    fn base_model(&self) -> &str {
        self.base_config.llm.model.as_str()
    }

    fn resolve_dm_scope(&self, provider: &TalosProvider) -> Option<String> {
        self.channel_dm_scopes.get(provider.as_str()).cloned()
    }

    fn build_session(
        &self,
        record: &SessionRecord,
        workspace_id: Option<String>,
    ) -> TalosResult<TalosSession> {
        TalosSession::restore(record, &self.base_config, workspace_id)
    }

    async fn cache_session(&self, session: TalosSession) -> TalosSession {
        let session_id = session.context.session_id.clone();
        self.session_cache
            .write()
            .await
            .insert(session_id, session.clone());
        session
    }

    async fn get_cached_session(&self, session_id: &str) -> Option<TalosSession> {
        self.session_cache.read().await.get(session_id).cloned()
    }

    async fn invalidate_cached_session(&self, session_id: &str) {
        self.session_cache.write().await.remove(session_id);
    }

    async fn resolve_session(&self, request: &GatewayRequest) -> TalosResult<TalosSession> {
        let workspace_id = request
            .workspace_id
            .clone()
            .unwrap_or_else(|| self.default_workspace_id.clone());
        let session_id = request.session_id.as_str();
        if let Some(session) = self.get_cached_session(session_id).await {
            return Ok(session);
        }

        let record = self.session_store.get_session_record(session_id).await?;
        let record = record.ok_or_else(|| {
            talos_error::TalosError::ResourceNotFound(format!("session {}", session_id))
        })?;
        let session = self.build_session(&record, Some(workspace_id))?;
        Ok(self.cache_session(session).await)
    }

    async fn record_turn_completion(
        &self,
        session_id: &str,
        assistant_model: &str,
        message_delta: u64,
        usage: &TalosUsage,
    ) -> TalosResult<()> {
        self.session_store
            .record_session_progress(session_id, message_delta, usage, Some(assistant_model))
            .await?;
        self.update_cached_session_after_turn(session_id, assistant_model, message_delta, usage)
            .await;
        Ok(())
    }

    async fn record_stream_completion(
        &self,
        session_id: &str,
        assistant_model: &str,
        message_delta: u64,
        usage: &TalosUsage,
    ) {
        let _ = self
            .session_store
            .record_session_progress(session_id, message_delta, usage, Some(assistant_model))
            .await;
        self.update_cached_session_after_turn(session_id, assistant_model, message_delta, usage)
            .await;
    }

    async fn update_cached_session_after_turn(
        &self,
        session_id: &str,
        assistant_model: &str,
        message_delta: u64,
        usage: &TalosUsage,
    ) {
        let mut cache = self.session_cache.write().await;
        if let Some(session) = cache.get_mut(session_id) {
            let now = talos_types::now();
            session.state.last_activity = now;
            session.state.message_count = session.state.message_count.saturating_add(message_delta);
            session.state.usage.input_tokens = session
                .state
                .usage
                .input_tokens
                .saturating_add(usage.input_tokens);
            session.state.usage.output_tokens = session
                .state
                .usage
                .output_tokens
                .saturating_add(usage.output_tokens);
            session.state.usage.total_tokens = session
                .state
                .usage
                .total_tokens
                .saturating_add(usage.total_tokens);
            session.config.agent.model = assistant_model.to_string();
            session.config.llm.model = assistant_model.to_string();
        }
    }

    async fn create_session(
        &self,
        context: TalosContext,
        target_agent: &str,
        model: Option<String>,
    ) -> TalosResult<TalosSession> {
        let record = self
            .session_store
            .create_session(
                &context,
                target_agent,
                self.base_model(),
                model.as_deref(),
                self.resolve_dm_scope(&context.provider),
            )
            .await?;
        let session = self.build_session(&record, Some(self.default_workspace_id.clone()))?;
        Ok(self.cache_session(session).await)
    }

    async fn list_sessions(&self, query: SessionQuery) -> TalosResult<Vec<TalosSession>> {
        let records = self.session_store.list_session_records(&query).await?;
        records
            .iter()
            .map(|record| self.build_session(record, Some(self.default_workspace_id.clone())))
            .collect()
    }

    async fn get_session(&self, session_id: &str) -> TalosResult<Option<TalosSession>> {
        if let Some(session) = self.get_cached_session(session_id).await {
            return Ok(Some(session));
        }
        self.load_session(session_id, Some(self.default_workspace_id.clone()))
            .await
    }

    async fn load_session(
        &self,
        session_id: &str,
        workspace_id: Option<String>,
    ) -> TalosResult<Option<TalosSession>> {
        if let Some(session) = self.get_cached_session(session_id).await {
            return Ok(Some(session));
        }
        let Some(record) = self.session_store.get_session_record(session_id).await? else {
            return Ok(None);
        };
        let session = self.build_session(
            &record,
            workspace_id.or_else(|| Some(self.default_workspace_id.clone())),
        )?;
        Ok(Some(self.cache_session(session).await))
    }

    async fn patch_session(
        &self,
        session_id: &str,
        patch: SessionPatch,
    ) -> TalosResult<SessionPatchAck> {
        let ack = self
            .session_store
            .patch_session_record(session_id, &patch)
            .await?;
        if ack.success {
            self.invalidate_cached_session(session_id).await;
        }
        Ok(ack)
    }

    async fn close_session(&self, session_id: &str) -> TalosResult<Option<TalosSession>> {
        let session = self
            .load_session(session_id, Some(self.default_workspace_id.clone()))
            .await?;
        Ok(session)
    }
}

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

    pub fn with_default_workspace_id(mut self, workspace_id: impl Into<String>) -> Self {
        self.sessions = Arc::new(
            self.sessions
                .as_ref()
                .clone()
                .with_default_workspace_id(workspace_id),
        );
        self
    }

    pub fn with_channel_dm_scope(
        mut self,
        channel_id: impl Into<String>,
        dm_scope: impl Into<String>,
    ) -> Self {
        self.sessions = Arc::new(
            self.sessions
                .as_ref()
                .clone()
                .with_channel_dm_scope(channel_id, dm_scope),
        );
        self
    }

    fn route_channel(context: &TalosContext) -> String {
        match context.channel_id.as_deref() {
            Some(channel_id) => format!("{}:{}", context.provider, channel_id),
            None => context.provider.to_string(),
        }
    }

    async fn resolve_session_agent(
        &self,
        session: &TalosSession,
    ) -> TalosResult<(String, Arc<dyn AgentOrchestrator>)> {
        let route_channel = Self::route_channel(&TalosContext {
            source_type: session.context.source_type.clone(),
            provider: session.context.provider.clone(),
            user_id: session.context.user_id.clone(),
            message_id: session.context.message_id.clone(),
            trace_id: session.context.trace_id.clone(),
            channel_id: session.context.channel_id.clone(),
            thread_id: session.context.thread_id.clone(),
        });
        let target_agent = self
            .router
            .route(&route_channel, &session.context.user_id)
            .await?
            .unwrap_or_else(|| "default".to_string());
        let agents = self.agents.read().await;
        let agent = agents
            .get(&target_agent)
            .cloned()
            .ok_or_else(|| talos_error::TalosError::AgentNotFound(target_agent.clone()))?;
        Ok((target_agent, agent))
    }

    /// Get reference to registered agents
    pub fn agents(&self) -> Arc<RwLock<HashMap<String, Arc<dyn AgentOrchestrator>>>> {
        self.agents.clone()
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

        let agent_response = agent
            .run(OrchestratorRequest {
                session: session.clone(),
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
    ) -> TalosResult<mpsc::Receiver<TalosEvent>> {
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

        let mut orchestrator_stream = agent
            .run_stream(OrchestratorRequest {
                session: session.clone(),
                input: request.input.clone(),
            })
            .await?;

        let sessions = self.sessions.clone();
        let session_id = session.context.session_id.clone();
        let message_id = format!("msg_{}", Uuid::new_v4().simple());
        let stream_session_id = session.context.session_id.clone();
        let response_model = model_for_turn.clone();
        let (tx, rx) = mpsc::channel(256);
        tokio::spawn(async move {
            let mut sequence: u64 = 0;
            let mut final_usage = TalosUsage::default();
            let mut final_message_delta = 2;

            while let Some(event) = orchestrator_stream.recv().await {
                let (event_type, payload) = map_orchestrator_event(event.clone());
                if let OrchestratorEvent::MessageCompleted(completed) = &event {
                    final_usage = completed.usage.clone();
                    final_message_delta = completed.total_output_items.max(1);
                }

                if tx
                    .send(TalosEvent {
                        event_type,
                        message_id: message_id.clone(),
                        session_id: stream_session_id.clone(),
                        sequence,
                        payload,
                    })
                    .await
                    .is_err()
                {
                    return;
                }
                sequence = sequence.saturating_add(1);
            }

            sessions
                .record_stream_completion(
                    &session_id,
                    &response_model,
                    final_message_delta,
                    &final_usage,
                )
                .await;
        });

        Ok(rx)
    }
}

fn map_orchestrator_event(event: OrchestratorEvent) -> (TalosEventType, TalosEventPayload) {
    match event {
        OrchestratorEvent::MessageStarted(payload) => (
            TalosEventType::MessageStarted,
            TalosEventPayload::MessageStarted(TalosMessageStartedEvent {
                request_id: payload.request_id,
                model: payload.model,
                usage: payload.usage,
                iteration: payload.iteration,
                step_id: payload.step_id,
            }),
        ),
        OrchestratorEvent::MessageReasoningDelta(payload) => (
            TalosEventType::MessageReasoningDelta,
            TalosEventPayload::MessageReasoningDelta(TalosMessageReasoningDeltaEvent {
                request_id: payload.request_id,
                delta: payload.delta,
                source: payload.source,
                stage: payload.stage,
                item_index: payload.item_index,
                iteration: payload.iteration,
                step_id: payload.step_id,
            }),
        ),
        OrchestratorEvent::MessageExecutionSummary(payload) => (
            TalosEventType::MessageExecutionSummary,
            TalosEventPayload::MessageExecutionSummary(TalosMessageExecutionSummaryEvent {
                request_id: payload.request_id,
                text: payload.text,
                item_index: payload.item_index,
                iteration: payload.iteration,
                step_id: payload.step_id,
            }),
        ),
        OrchestratorEvent::MessageOutputDelta(payload) => (
            TalosEventType::MessageOutputDelta,
            TalosEventPayload::MessageOutputDelta(TalosMessageOutputDeltaEvent {
                request_id: payload.request_id,
                delta: payload.delta,
                item_index: payload.item_index,
                iteration: payload.iteration,
                step_id: payload.step_id,
            }),
        ),
        OrchestratorEvent::MessageToolCallStarted(payload) => (
            TalosEventType::MessageToolCallStarted,
            TalosEventPayload::MessageToolCallStarted(TalosMessageToolCallStartedEvent {
                request_id: payload.request_id,
                call_id: payload.call_id,
                name: payload.name,
                arguments: payload.arguments,
                item_index: payload.item_index,
                iteration: payload.iteration,
                step_id: payload.step_id,
                timing_ms: payload.timing_ms,
            }),
        ),
        OrchestratorEvent::MessageToolCallCompleted(payload) => (
            TalosEventType::MessageToolCallCompleted,
            TalosEventPayload::MessageToolCallCompleted(TalosMessageToolCallCompletedEvent {
                request_id: payload.request_id,
                call_id: payload.call_id,
                tool: payload.tool,
                content: payload.content,
                is_error: payload.is_error,
                item_index: payload.item_index,
                iteration: payload.iteration,
                step_id: payload.step_id,
                timing_ms: payload.timing_ms,
            }),
        ),
        OrchestratorEvent::MessageToolCallPendingApproval(payload) => (
            TalosEventType::MessageToolCallPendingApproval,
            TalosEventPayload::MessageToolCallPendingApproval(
                TalosMessageToolCallPendingApprovalEvent {
                    request_id: payload.request_id,
                    call_id: payload.call_id,
                    tool_name: payload.tool_name,
                    prompt: payload.prompt,
                    arguments: payload.arguments,
                    step_id: payload.step_id,
                },
            ),
        ),
        OrchestratorEvent::MessageCompleted(payload) => (
            TalosEventType::MessageCompleted,
            TalosEventPayload::MessageCompleted(TalosMessageCompletedEvent {
                request_id: payload.request_id,
                stop_reason: payload.stop_reason,
                usage: payload.usage,
                total_output_items: payload.total_output_items,
                iteration: payload.iteration,
                step_id: payload.step_id,
                task_count: payload.task_count,
            }),
        ),
        OrchestratorEvent::MessageError(payload) => (
            TalosEventType::MessageError,
            TalosEventPayload::MessageError(TalosMessageErrorEvent {
                request_id: payload.request_id,
                error: payload.error,
            }),
        ),
    }
}

#[async_trait]
impl SessionService for CoordinatorImpl {
    async fn create_session(
        &self,
        context: TalosContext,
        model: Option<String>,
    ) -> TalosResult<TalosSession> {
        let channel = Self::route_channel(&context);
        let target_agent = self
            .router
            .route(&channel, &context.user_id)
            .await?
            .unwrap_or_else(|| "default".to_string());

        let record = self
            .sessions
            .create_session(context, &target_agent, model)
            .await?;
        Ok(record)
    }

    async fn list_sessions(&self, query: SessionQuery) -> TalosResult<Vec<TalosSession>> {
        self.sessions.list_sessions(query).await
    }

    async fn get_session(&self, session_id: &str) -> TalosResult<Option<TalosSession>> {
        self.sessions.get_session(session_id).await
    }

    async fn patch_session(
        &self,
        session_id: &str,
        patch: SessionPatch,
    ) -> TalosResult<SessionPatchAck> {
        self.sessions.patch_session(session_id, patch).await
    }

    async fn close_session(&self, session_id: &str) -> TalosResult<()> {
        let Some(session) = self.sessions.close_session(session_id).await? else {
            return Err(talos_error::TalosError::ResourceNotFound(format!(
                "session {}",
                session_id
            )));
        };

        let (_target_agent, agent) = self.resolve_session_agent(&session).await?;
        self.sessions.invalidate_cached_session(session_id).await;
        tokio::spawn(async move {
            if let Err(e) = agent.close_session(session).await {
                tracing::warn!("background memory flush failed: {}", e);
            }
        });
        Ok(())
    }

    async fn submit_tool_approval(
        &self,
        session_id: &str,
        call_id: &str,
        tool_name: &str,
        approved: bool,
        reason: Option<String>,
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
            .submit_tool_approval(session_id, call_id, tool_name, approved, reason)
            .await?;

        let response = agent
            .run(OrchestratorRequest {
                session: session.clone(),
                input: Vec::new(),
            })
            .await?;
        let model_for_turn = session.config.llm.model.clone();
        self.sessions
            .record_turn_completion(
                &session.context.session_id,
                &model_for_turn,
                1,
                &response.usage,
            )
            .await?;
        Ok(())
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

        let response = agent
            .run(OrchestratorRequest {
                session: session.clone(),
                input: Vec::new(),
            })
            .await?;
        let model_for_turn = session.config.llm.model.clone();
        self.sessions
            .record_turn_completion(
                &session.context.session_id,
                &model_for_turn,
                1,
                &response.usage,
            )
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use talos_agent::{OrchestratorEvent, OrchestratorExecution, OrchestratorRequest};
    use talos_types::talos::{TalosInputItem, TalosSourceType};
    use tokio;

    // Define mock implementations for testing
    struct MockRouter;

    impl MockRouter {
        fn new() -> Self {
            Self
        }
    }

    struct RecordingApprovalAgent {
        approvals: Arc<Mutex<Vec<(String, String, String, bool, Option<String>)>>>,
        resumed_inputs: Arc<Mutex<Vec<Vec<TalosInputItem>>>>,
    }

    impl RecordingApprovalAgent {
        fn new() -> Self {
            Self {
                approvals: Arc::new(Mutex::new(Vec::new())),
                resumed_inputs: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    #[async_trait]
    impl talos_router::Router for MockRouter {
        async fn route(&self, _channel: &str, _user_id: &str) -> TalosResult<Option<String>> {
            Ok(Some("test_agent".to_string()))
        }

        async fn add_binding(&self, _binding: talos_router::Binding) -> TalosResult<()> {
            Ok(())
        }

        async fn remove_binding(&self, _id: &str) -> TalosResult<()> {
            Ok(())
        }

        async fn list_bindings(&self) -> TalosResult<Vec<talos_router::Binding>> {
            Ok(vec![])
        }
    }

    #[async_trait]
    impl AgentOrchestrator for RecordingApprovalAgent {
        fn id(&self) -> &str {
            "recording-approval"
        }

        async fn run(&self, request: OrchestratorRequest) -> TalosResult<OrchestratorExecution> {
            self.resumed_inputs
                .lock()
                .unwrap()
                .push(request.input.clone());
            Ok(OrchestratorExecution {
                output_text: "ok".to_string(),
                output_items: Vec::new(),
                reasoning: Vec::new(),
                stop_reason: "end_turn".to_string(),
                usage: TalosUsage::default(),
                task_count: 1,
                session_id: request.session.context.session_id,
            })
        }

        async fn run_stream(
            &self,
            _request: OrchestratorRequest,
        ) -> TalosResult<mpsc::Receiver<OrchestratorEvent>> {
            let (_tx, rx) = mpsc::channel(1);
            Ok(rx)
        }

        async fn close_session(&self, _session: TalosSession) -> TalosResult<()> {
            Ok(())
        }

        async fn submit_tool_approval(
            &self,
            session_id: &str,
            call_id: &str,
            tool_name: &str,
            approved: bool,
            reason: Option<String>,
        ) -> TalosResult<()> {
            self.approvals.lock().unwrap().push((
                session_id.to_string(),
                call_id.to_string(),
                tool_name.to_string(),
                approved,
                reason,
            ));
            Ok(())
        }

        async fn submit_dynamic_tool_result(
            &self,
            _session_id: &str,
            _call_id: &str,
            _tool_name: &str,
            _output: String,
            _success: bool,
        ) -> TalosResult<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_coordinator_creation() {
        let mock_router = Arc::new(MockRouter::new());
        let mock_session_store = Arc::new(talos_session_store::MockSessionStore::new());

        let coordinator =
            CoordinatorImpl::new(mock_router, mock_session_store, TalosConfig::default());

        assert_eq!(coordinator.agents.read().await.len(), 0);
    }

    #[tokio::test]
    async fn test_register_agent() {
        let mock_router = Arc::new(MockRouter::new());
        let mock_session_store = Arc::new(talos_session_store::MockSessionStore::new());

        let coordinator =
            CoordinatorImpl::new(mock_router, mock_session_store, TalosConfig::default());

        let mock_agent = Arc::new(talos_agent::MockAgent::new());
        let result = coordinator.register_agent("test_agent", mock_agent).await;

        assert!(result.is_ok());
        assert_eq!(coordinator.agents.read().await.len(), 1);
    }

    #[tokio::test]
    async fn test_handle_stream_returns_orchestrator_events() {
        let mock_router = Arc::new(MockRouter::new());
        let mock_session_store = Arc::new(talos_session_store::MockSessionStore::new());
        let coordinator =
            CoordinatorImpl::new(mock_router, mock_session_store, TalosConfig::default());
        coordinator
            .register_agent("test_agent", Arc::new(talos_agent::MockAgent::new()))
            .await
            .expect("agent registration should succeed");

        let mut stream = coordinator
            .handle_stream(GatewayRequest {
                session_id: "test-session".to_string(),
                workspace_id: None,
                input: vec![TalosInputItem::Text {
                    text: "hello".to_string(),
                }],
                context: TalosContext {
                    source_type: TalosSourceType::Im,
                    provider: TalosProvider::Telegram,
                    user_id: "user-1".to_string(),
                    message_id: "message-1".to_string(),
                    trace_id: "trace-1".to_string(),
                    channel_id: Some("test-channel".to_string()),
                    thread_id: None,
                },
            })
            .await
            .expect("stream request should succeed");

        let mut event_types = Vec::new();
        while let Some(event) = stream.recv().await {
            event_types.push(event.event_type.clone());
            if event.event_type == TalosEventType::MessageCompleted {
                break;
            }
        }

        assert!(event_types.contains(&TalosEventType::MessageStarted));
        assert!(event_types.contains(&TalosEventType::MessageOutputDelta));
        assert!(event_types.contains(&TalosEventType::MessageCompleted));
    }

    #[tokio::test]
    async fn test_submit_tool_approval_routes_to_session_agent() {
        let mock_router = Arc::new(MockRouter::new());
        let mock_session_store = Arc::new(talos_session_store::MockSessionStore::new());
        let coordinator =
            CoordinatorImpl::new(mock_router, mock_session_store, TalosConfig::default());
        let agent = Arc::new(RecordingApprovalAgent::new());
        let approvals = agent.approvals.clone();
        let resumed_inputs = agent.resumed_inputs.clone();
        coordinator
            .register_agent("test_agent", agent)
            .await
            .expect("agent registration should succeed");

        coordinator
            .submit_tool_approval(
                "session-1",
                "call-1",
                "exec_command",
                false,
                Some("deny".to_string()),
            )
            .await
            .expect("tool approval should route successfully");

        let approvals = approvals.lock().unwrap();
        assert_eq!(approvals.len(), 1);
        assert_eq!(
            approvals[0],
            (
                "session-1".to_string(),
                "call-1".to_string(),
                "exec_command".to_string(),
                false,
                Some("deny".to_string()),
            )
        );
        drop(approvals);

        let resumed_inputs = resumed_inputs.lock().unwrap();
        assert_eq!(resumed_inputs.len(), 1);
        assert!(resumed_inputs[0].is_empty());
    }
}
