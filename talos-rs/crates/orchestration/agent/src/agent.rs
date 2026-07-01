use async_trait::async_trait;
use dashmap::DashMap;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, Mutex};

mod kernel;
mod mock;
mod request;
mod runtime;
mod session;
#[cfg(test)]
#[path = "agent/agent_tests.rs"]
mod tests;

pub use mock::MockAgent;

use crate::{
    empty_skill_roots_resolver, flush_manager::MemoryFlushManager, AgentDescriptor, AgentHarness,
    AgentInput, AgentInputParams, AgentInputResult, AgentInterruptParams, AgentInterruptResult,
    AgentRunParams, AgentRunResult, AgentStatus, SessionTitleUpdate, SkillRootsResolver,
};
#[cfg(test)]
use runtime::{emit_kernel_timeline_events, KernelTimelineState};
use runtime::{AgentRuntime, AgentRuntimeAbortHandle, KernelAgent, KernelAgentParams};
use talos_agent_kernel::TimelineBuilder;
use talos_config::{AgentKernelConfig, PlannerMode};
use talos_error::{TalosError, TalosResult};
use talos_llm_provider::{ChatMessage, ChatRequest, LlmProvider};
use talos_memory::MemoryStore;
use talos_tools::ToolRegistry;
use talos_types::normalize_session_title;
use talos_types::talos::{AgentEvent, StopReason as TalosStopReason, TokenUsage};
use tracing::warn;

const SESSION_TITLE_PROMPT: &str = r#"Generate a concise session title for the conversation.

Rules:
- Return only the title text.
- One line only.
- No markdown, no quotes, no trailing punctuation.
- Prefer 3-8 words.
- Focus on the core task topic.
"#;

/// Harness-owned in-memory state for an active Agent run.
#[derive(Clone)]
pub(crate) enum ActiveTurnHandle {
    Reserved,
    Running {
        turn_id: Option<String>,
        execution_task: AgentRuntimeAbortHandle,
    },
}

/// Queued Agent run waiting for the session's active run to finish.
pub(crate) enum QueuedTurn {
    Unary {
        request: AgentRunParams,
        response_tx: oneshot::Sender<TalosResult<AgentRunResult>>,
    },
    Stream {
        request: AgentRunParams,
        event_tx: mpsc::Sender<AgentEvent>,
    },
}

/// Default AgentHarness implementation used by Talos today.
///
/// The Harness owns shared Agent framework behavior and delegates backend
/// execution to an AgentRuntime implementation. It should not expose Kernel,
/// Codex, Claude, or other runtime-private objects through its public API.
#[derive(Clone)]
pub struct AgentHarnessImpl {
    /// Stable Agent id registered in Coordinator.
    pub id: String,
    /// Human-readable Agent name.
    pub name: String,
    /// Human-readable Agent description.
    pub description: String,
    /// LLM provider shared with the built-in runtime.
    pub llm_provider: Arc<dyn LlmProvider>,
    /// Default model for this Harness.
    pub model: String,
    /// Memory store used by current Kernel-backed behavior.
    pub memory_store: Arc<MemoryStore>,
    /// Tool registry shared by Harness and runtime execution.
    pub tool_registry: Arc<ToolRegistry>,
    kernel_config: AgentKernelConfig,
    active_turns: Arc<DashMap<String, ActiveTurnHandle>>,
    completed_title_updates: Arc<DashMap<String, SessionTitleUpdate>>,
    session_backlogs: Arc<Mutex<HashMap<String, VecDeque<QueuedTurn>>>>,
    memory_flush_manager: Arc<MemoryFlushManager>,
    runtime: KernelAgent,
}

impl AgentHarnessImpl {
    /// Construct the default Kernel-backed Harness implementation.
    pub fn new(
        id: String,
        name: String,
        description: String,
        llm_provider: Arc<dyn LlmProvider>,
        model: String,
        tool_registry: Arc<ToolRegistry>,
        kernel_config: AgentKernelConfig,
    ) -> Self {
        Self::new_with_skill_roots_resolver(
            id,
            name,
            description,
            llm_provider,
            model,
            tool_registry,
            kernel_config,
            empty_skill_roots_resolver(),
        )
    }

    /// Construct the default Harness with an explicit skill roots resolver.
    pub fn new_with_skill_roots_resolver(
        id: String,
        name: String,
        description: String,
        llm_provider: Arc<dyn LlmProvider>,
        model: String,
        tool_registry: Arc<ToolRegistry>,
        kernel_config: AgentKernelConfig,
        skill_roots_resolver: Arc<dyn SkillRootsResolver>,
    ) -> Self {
        let kernel_config = Self::normalized_kernel_config(kernel_config);
        let memory_store = Arc::new(Self::build_memory_store_from_kernel_config(
            &kernel_config,
            llm_provider.clone(),
        ));

        Self {
            id: id.clone(),
            name: name.clone(),
            description: description.clone(),
            llm_provider: llm_provider.clone(),
            model: model.clone(),
            memory_store,
            tool_registry: tool_registry.clone(),
            kernel_config: kernel_config.clone(),
            active_turns: Arc::new(DashMap::new()),
            completed_title_updates: Arc::new(DashMap::new()),
            session_backlogs: Arc::new(Mutex::new(HashMap::new())),
            memory_flush_manager: Arc::new(MemoryFlushManager::new()),
            runtime: KernelAgent::new(KernelAgentParams {
                id,
                name,
                description,
                llm_provider,
                model: model.clone(),
                tool_registry,
                kernel_config,
                skill_roots_resolver,
            }),
        }
    }

    /// Add a tool to this agent's registry
    pub fn add_tool(&self, tool: Arc<dyn talos_tools::Tool>) {
        self.tool_registry.register(tool);
    }

    /// Set the tool registry for this agent (allows external tool injection)
    pub fn set_tool_registry(&mut self, tool_registry: Arc<ToolRegistry>) {
        self.tool_registry = tool_registry.clone();
        self.runtime.set_tool_registry(tool_registry);
    }

    /// Set the default planner mode for the Kernel-backed runtime.
    pub fn set_default_planner_mode(&mut self, planner_mode: &str) -> TalosResult<()> {
        let mode = PlannerMode::parse(planner_mode.trim())?;
        self.kernel_config.planner_mode = mode;
        self.runtime.set_kernel_config(self.kernel_config.clone());
        Ok(())
    }

    /// Update kernel configuration with a new config object.
    pub fn update_kernel_config(&mut self, config: AgentKernelConfig) {
        self.kernel_config = Self::normalized_kernel_config(config);
        self.runtime.set_kernel_config(self.kernel_config.clone());
        self.rebuild_memory_store();
    }

    /// Generate a system prompt that includes tool information
    pub fn generate_system_prompt(&self) -> String {
        let available_tools = self.tool_registry.list_tool_names();
        format!(
            r#"You are {}, {}

You have access to the following tools:
{}

Use native function calling when a tool is needed.
After receiving tool outputs, continue reasoning and provide the final response."#,
            self.name,
            self.description,
            available_tools.join(", ")
        )
    }

    async fn generate_session_title_update(
        &self,
        session: &talos_types::TalosSession,
        first_user_text: &str,
        first_assistant_text: &str,
    ) -> Option<SessionTitleUpdate> {
        if session.has_real_title()
            || session.state.message_count != 0
            || first_user_text.trim().is_empty()
            || first_assistant_text.trim().is_empty()
        {
            return None;
        }

        let request = ChatRequest::builder()
            .model(session.config.llm.model.clone())
            .messages(vec![
                ChatMessage::system(SESSION_TITLE_PROMPT),
                ChatMessage::user(format!(
                    "First user message:\n{}\n\nFirst assistant response:\n{}",
                    first_user_text.trim(),
                    first_assistant_text.trim()
                )),
            ])
            .build();

        match self.llm_provider.chat(request).await {
            Ok(response) => {
                normalize_session_title(&response.content).map(|title| SessionTitleUpdate { title })
            }
            Err(error) => {
                warn!(
                    session_id = %session.context.session_id,
                    ?error,
                    "failed to generate session title"
                );
                None
            }
        }
    }

    fn title_update_key(session_id: &str, turn_id: &str) -> String {
        format!("{session_id}:{turn_id}")
    }

    async fn reserve_or_enqueue_unary_turn(
        &self,
        request: AgentRunParams,
    ) -> TalosResult<Option<oneshot::Receiver<TalosResult<AgentRunResult>>>> {
        let session_id = request.session.context.session_id.clone();
        if self.has_queued_turns(&session_id).await {
            let (response_tx, response_rx) = oneshot::channel();
            self.enqueue_turn(
                &session_id,
                QueuedTurn::Unary {
                    request,
                    response_tx,
                },
            )
            .await;
            return Ok(Some(response_rx));
        }

        match self.try_reserve_active_turn(&session_id) {
            Ok(()) => Ok(None),
            Err(_) => {
                let (response_tx, response_rx) = oneshot::channel();
                self.enqueue_turn(
                    &session_id,
                    QueuedTurn::Unary {
                        request,
                        response_tx,
                    },
                )
                .await;
                Ok(Some(response_rx))
            }
        }
    }

    async fn reserve_or_enqueue_stream_turn(
        &self,
        request: AgentRunParams,
        event_tx: mpsc::Sender<AgentEvent>,
    ) -> TalosResult<bool> {
        let session_id = request.session.context.session_id.clone();
        if self.has_queued_turns(&session_id).await {
            self.enqueue_turn(&session_id, QueuedTurn::Stream { request, event_tx })
                .await;
            return Ok(false);
        }

        match self.try_reserve_active_turn(&session_id) {
            Ok(()) => Ok(true),
            Err(_) => {
                self.enqueue_turn(&session_id, QueuedTurn::Stream { request, event_tx })
                    .await;
                Ok(false)
            }
        }
    }

    async fn execute_reserved_run(&self, request: AgentRunParams) -> TalosResult<AgentRunResult> {
        let turn_id = request.turn_id.clone();
        let session = request.session.clone();
        let session_id = session.context.session_id.clone();
        let title_input = request::AgentKernelParams::extract_text_input(&request.input);
        let runtime = self.runtime.clone();
        let run_handle = tokio::spawn(async move { runtime.run(request).await });
        let abort_handle = AgentRuntimeAbortHandle::new(run_handle.abort_handle());
        if let Err(err) = self.activate_reserved_turn(&session_id, turn_id, abort_handle) {
            run_handle.abort();
            self.finish_active_turn_and_resume(&session_id).await;
            return Err(err);
        }

        let result = match run_handle.await {
            Ok(Ok(mut result)) => {
                result.session_title_update = self
                    .generate_session_title_update(&session, &title_input, &result.output_text)
                    .await;
                Ok(result)
            }
            Ok(Err(err)) => Err(err),
            Err(err) if err.is_cancelled() => Ok(AgentRunResult {
                output_text: String::new(),
                output_items: Vec::new(),
                reasoning: Vec::new(),
                stop_reason: TalosStopReason::Cancelled,
                usage: TokenUsage::default(),
                task_count: 0,
                session_id: session_id.clone(),
                session_title_update: None,
            }),
            Err(err) => Err(TalosError::InternalError(format!(
                "kernel task join failed: {}",
                err
            ))),
        };
        self.finish_active_turn_and_resume(&session_id).await;
        result
    }

    fn spawn_reserved_stream_turn(&self, request: AgentRunParams, tx: mpsc::Sender<AgentEvent>) {
        let this = self.clone();
        let stream_task = tokio::spawn(async move {
            let request_turn_id = request.turn_id.clone();
            let session = request.session.clone();
            let session_id = session.context.session_id.clone();
            let title_input = request::AgentKernelParams::extract_text_input(&request.input);
            let title_turn_id = request_turn_id.clone().unwrap_or_default();
            let runtime = this.runtime.clone();
            let runtime_stream = match runtime.run_streamed(request).await {
                Ok(runtime_stream) => runtime_stream,
                Err(err) => {
                    let mut timeline = TimelineBuilder::new(
                        "req_failed".to_string(),
                        title_turn_id.clone(),
                        session.context.message_id.clone(),
                        session_id.clone(),
                    );
                    let _ = tx.send(timeline.fail_turn(err.to_string())).await;
                    this.finish_active_turn_and_resume(&session_id).await;
                    return;
                }
            };

            let abort_handle = runtime_stream.abort_handle.clone();
            if let Err(err) =
                this.activate_reserved_turn(&session_id, request_turn_id, abort_handle)
            {
                runtime_stream.abort_handle.abort();
                runtime_stream.completion.abort();
                let mut timeline = TimelineBuilder::new(
                    "req_failed".to_string(),
                    title_turn_id.clone(),
                    session.context.message_id.clone(),
                    session_id.clone(),
                );
                let _ = tx.send(timeline.fail_turn(err.to_string())).await;
                this.finish_active_turn_and_resume(&session_id).await;
                return;
            }

            let mut runtime_events = runtime_stream.events;
            while let Some(event) = runtime_events.recv().await {
                if tx.send(event).await.is_err() {
                    runtime_stream.abort_handle.abort();
                    runtime_stream.completion.abort();
                    this.finish_active_turn_and_resume(&session_id).await;
                    return;
                }
            }

            match runtime_stream.completion.await {
                Ok(Ok(result)) => {
                    this.finish_active_turn_and_resume(&session_id).await;
                    if !title_turn_id.is_empty() {
                        if let Some(update) = this
                            .generate_session_title_update(
                                &session,
                                &title_input,
                                &result.output_text,
                            )
                            .await
                        {
                            this.completed_title_updates.insert(
                                Self::title_update_key(&session_id, &title_turn_id),
                                update,
                            );
                        }
                    }
                }
                Ok(Err(_)) | Err(_) => {
                    this.finish_active_turn_and_resume(&session_id).await;
                }
            }
        });
        drop(stream_task);
    }
    async fn finish_active_turn_and_resume(&self, session_id: &str) {
        if let Some(queued_turn) = self.dequeue_next_turn(session_id).await {
            self.active_turns
                .insert(session_id.to_string(), ActiveTurnHandle::Reserved);
            self.dispatch_queued_turn(queued_turn);
        } else {
            self.clear_active_turn(session_id);
        }
    }

    fn dispatch_queued_turn(&self, queued_turn: QueuedTurn) {
        match queued_turn {
            QueuedTurn::Unary {
                request,
                response_tx,
            } => {
                let this = self.clone();
                let task = tokio::spawn(async move {
                    let result = this.execute_reserved_run(request).await;
                    let _ = response_tx.send(result);
                });
                drop(task);
            }
            QueuedTurn::Stream { request, event_tx } => {
                self.spawn_reserved_stream_turn(request, event_tx);
            }
        }
    }
}

#[async_trait]
impl AgentHarness for AgentHarnessImpl {
    fn id(&self) -> &str {
        &self.id
    }

    fn descriptor(&self) -> AgentDescriptor {
        AgentDescriptor {
            agent_id: self.id.clone(),
            name: self.name.clone(),
            kind: self.runtime.kind(),
            description: self.description.clone(),
            capabilities: self.runtime.capabilities(),
            status: AgentStatus::Enabled,
        }
    }

    fn take_session_title_update(
        &self,
        session_id: &str,
        turn_id: &str,
    ) -> Option<SessionTitleUpdate> {
        self.completed_title_updates
            .remove(&Self::title_update_key(session_id, turn_id))
            .map(|(_, update)| update)
    }

    async fn run(&self, request: AgentRunParams) -> TalosResult<AgentRunResult> {
        if let Some(response_rx) = self.reserve_or_enqueue_unary_turn(request.clone()).await? {
            return response_rx.await.map_err(|err| {
                TalosError::InternalError(format!("queued agent execution channel closed: {}", err))
            })?;
        }

        self.execute_reserved_run(request).await
    }

    async fn run_streamed(
        &self,
        request: AgentRunParams,
    ) -> TalosResult<mpsc::Receiver<AgentEvent>> {
        let (tx, rx) = mpsc::channel(256);
        let should_start_now = self
            .reserve_or_enqueue_stream_turn(request.clone(), tx.clone())
            .await?;
        if should_start_now {
            self.spawn_reserved_stream_turn(request, tx);
        }
        Ok(rx)
    }

    async fn close_session(&self, session: talos_types::TalosSession) -> TalosResult<()> {
        self.close_session_impl(session).await
    }

    async fn compact_session(&self, session: talos_types::TalosSession) -> TalosResult<TokenUsage> {
        self.compact_session_impl(session).await
    }

    async fn clear_session(&self, session: talos_types::TalosSession) -> TalosResult<()> {
        self.clear_session_impl(session).await
    }

    async fn drain_memory_flushes(&self, timeout: Duration) -> TalosResult<()> {
        self.drain_pending_memory_flushes(timeout).await
    }

    fn pending_memory_flushes(&self) -> usize {
        self.pending_memory_flush_count()
    }

    async fn has_pending_memory_flush(&self, session_id: &str) -> bool {
        AgentHarnessImpl::has_pending_memory_flush(self, session_id).await
    }

    fn has_active_turn(&self, session_id: &str) -> bool {
        AgentHarnessImpl::has_active_turn(self, session_id)
    }

    async fn interrupt(&self, params: AgentInterruptParams) -> TalosResult<AgentInterruptResult> {
        let Some(handle) = self.active_turns.get(&params.session_id) else {
            return Err(talos_error::TalosError::ResourceNotFound(format!(
                "active turn for session {}",
                params.session_id
            )));
        };
        if let ActiveTurnHandle::Running {
            turn_id,
            execution_task,
        } = handle.value()
        {
            if let Some(expected_turn_id) = params.turn_id.as_deref() {
                if turn_id.as_deref() != Some(expected_turn_id) {
                    return Err(talos_error::TalosError::ResourceNotFound(format!(
                        "active turn `{}` for session {}",
                        expected_turn_id, params.session_id
                    )));
                }
            }
            execution_task.abort();
            return Ok(AgentInterruptResult { interrupted: true });
        }
        Err(talos_error::TalosError::ResourceNotFound(format!(
            "active turn for session {} is not running",
            params.session_id
        )))
    }

    async fn drain_active_turns(&self, timeout: Duration) -> TalosResult<()> {
        AgentHarnessImpl::drain_active_turns(self, timeout).await
    }

    async fn submit_input(&self, params: AgentInputParams) -> TalosResult<AgentInputResult> {
        match params.input {
            AgentInput::DynamicToolResult {
                call_id,
                tool_name,
                output,
                success,
            } => {
                self.submit_dynamic_tool_result(
                    params.session,
                    &call_id,
                    &tool_name,
                    output,
                    success,
                )
                .await?;
                Ok(AgentInputResult { accepted: true })
            }
        }
    }
}
