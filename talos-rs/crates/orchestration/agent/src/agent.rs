use async_trait::async_trait;
use dashmap::DashMap;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex};

mod kernel;
mod lifecycle;
mod mock;
mod request;
mod runtime;
mod runtime_compaction;
mod session;
#[cfg(test)]
#[path = "agent/agent_tests.rs"]
mod tests;

pub use mock::MockAgent;

use lifecycle::{ActiveTurnHandle, AgentRunMode, AgentRuntimeAbortHandle, QueuedTurn};

use crate::{
    AgentDescriptor, AgentError, AgentErrorKind, AgentHarness, AgentInput, AgentInputParams,
    AgentInputResult, AgentInterruptParams, AgentInterruptResult, AgentResult, AgentResumeParams,
    AgentRunConfig, AgentRunParams, AgentRunResult, AgentRunSessionState,
    AgentSessionCompactionParams, AgentStatus, SkillRootsResolver,
};
#[cfg(test)]
use runtime::{emit_kernel_timeline_events, KernelTimelineState};
use runtime::{AgentRuntime, KernelAgent, KernelAgentParams};
use talos_agent_kernel::{AttachmentReader, TimelineBuilder};
use talos_config::{AgentKernelConfig, PlannerMode};
use talos_error::{TalosError, TalosResult};
use talos_llm_provider::{ChatMessage, ChatRequest, LlmProviderRouter};
use talos_memory::MemoryWriter;
use talos_store::session::{SessionArtifactStore, SessionStore};
use talos_tools::ToolRegistry;
use talos_types::talos::{AgentEvent, StopReason as TalosStopReason, TokenUsage};
use talos_types::{normalize_session_title, ModelSelection};
use tracing::warn;

const ACTIVE_TURN_MESSAGE_CAPACITY: usize = 16;
const SESSION_TITLE_PROMPT: &str = r#"Generate a concise session title for the conversation.

Rules:
- Return only the title text.
- One line only.
- No markdown, no quotes, no trailing punctuation.
- Prefer 3-8 words.
- Focus on the core task topic.
"#;

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
    /// LLM provider router shared with the built-in runtime.
    pub llm_provider_router: Arc<dyn LlmProviderRouter>,
    /// Default model and reasoning policy for this Harness.
    pub model_selection: ModelSelection,
    /// Long-term memory persistence injected by the composition root.
    memory_writer: Arc<dyn MemoryWriter>,
    /// Recoverable session artifact persistence injected by the composition root.
    session_artifact_store: Arc<dyn SessionArtifactStore>,
    /// Session state persistence used for memory formation cursors.
    session_store: Arc<dyn SessionStore>,
    /// Tool registry shared by Harness and runtime execution.
    pub tool_registry: Arc<ToolRegistry>,
    kernel_config: AgentKernelConfig,
    active_turns: Arc<DashMap<String, ActiveTurnHandle>>,
    generated_session_titles: Arc<DashMap<String, String>>,
    session_backlogs: Arc<Mutex<HashMap<String, VecDeque<QueuedTurn>>>>,
    runtime: KernelAgent,
}

/// Constructor inputs for the default Harness implementation.
///
/// The Agent module owns runtime construction. Callers provide already-merged
/// Agent definition data and shared services, but must not pass runtime-private
/// state through this boundary.
pub struct AgentHarnessParams {
    /// Stable routing identifier.
    pub id: String,
    /// User-visible Agent name.
    pub name: String,
    /// User-visible Agent description.
    pub description: String,
    /// Provider selection boundary.
    pub llm_provider_router: Arc<dyn LlmProviderRouter>,
    /// Default model selection.
    pub model_selection: ModelSelection,
    /// Tools available to this Harness.
    pub tool_registry: Arc<ToolRegistry>,
    /// Resolved Kernel runtime configuration.
    pub kernel_config: AgentKernelConfig,
    /// Resolver for configured skill roots.
    pub skill_roots_resolver: Arc<dyn SkillRootsResolver>,
    /// Recoverable session artifact persistence.
    pub session_artifact_store: Arc<dyn SessionArtifactStore>,
    /// Session state persistence.
    pub session_store: Arc<dyn SessionStore>,
    /// Long-term memory persistence.
    pub memory_writer: Arc<dyn MemoryWriter>,
    /// Authorized file content reader used only for the active model turn.
    pub attachment_reader: Arc<dyn AttachmentReader>,
}

impl AgentHarnessImpl {
    /// Construct the default Kernel-backed Harness implementation.
    pub fn new(params: AgentHarnessParams) -> TalosResult<Self> {
        let AgentHarnessParams {
            id,
            name,
            description,
            llm_provider_router,
            model_selection,
            tool_registry,
            kernel_config,
            skill_roots_resolver,
            session_artifact_store,
            session_store,
            memory_writer,
            attachment_reader,
        } = params;
        let kernel_config = Self::normalized_kernel_config(kernel_config);
        llm_provider_router.provider_for(&model_selection.model_ref)?;

        Ok(Self {
            id: id.clone(),
            name: name.clone(),
            description: description.clone(),
            llm_provider_router: llm_provider_router.clone(),
            model_selection: model_selection.clone(),
            memory_writer: memory_writer.clone(),
            session_artifact_store: session_artifact_store.clone(),
            session_store: session_store.clone(),
            tool_registry: tool_registry.clone(),
            kernel_config: kernel_config.clone(),
            active_turns: Arc::new(DashMap::new()),
            generated_session_titles: Arc::new(DashMap::new()),
            session_backlogs: Arc::new(Mutex::new(HashMap::new())),
            runtime: KernelAgent::new(KernelAgentParams {
                id,
                name,
                description,
                llm_provider_router,
                tool_registry,
                kernel_config,
                skill_roots_resolver,
                session_artifact_store,
                session_store,
                memory_writer,
                attachment_reader,
            }),
        })
    }

    /// Register a tool in the Harness-owned tool registry.
    pub fn add_tool(&self, tool: Arc<dyn talos_tools::Tool>) {
        self.tool_registry.register(tool);
    }

    /// Replace the Harness tool registry and propagate it to the runtime.
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

    /// Replace Kernel runtime configuration.
    pub fn update_kernel_config(&mut self, config: AgentKernelConfig) -> TalosResult<()> {
        let kernel_config = Self::normalized_kernel_config(config);
        self.kernel_config = kernel_config;
        self.runtime.set_kernel_config(self.kernel_config.clone());
        Ok(())
    }

    /// Generate the default Kernel system prompt from Harness metadata and tools.
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

    async fn generate_session_title(
        &self,
        session_state: &AgentRunSessionState,
        run_config: &AgentRunConfig,
        first_user_text: &str,
        first_assistant_text: &str,
    ) -> Option<String> {
        if session_state.has_real_title()
            || session_state.message_count != 0
            || first_user_text.trim().is_empty()
            || first_assistant_text.trim().is_empty()
        {
            return None;
        }

        let request = ChatRequest::builder()
            .model(run_config.model_selection.model_ref.id.clone())
            .messages(vec![
                ChatMessage::system(SESSION_TITLE_PROMPT),
                ChatMessage::user(format!(
                    "First user message:\n{}\n\nFirst assistant response:\n{}",
                    first_user_text.trim(),
                    first_assistant_text.trim()
                )),
            ])
            .build();

        let llm_provider = match self
            .llm_provider_router
            .provider_for(&run_config.model_selection.model_ref)
        {
            Ok(llm_provider) => llm_provider,
            Err(error) => {
                warn!(
                    ?error,
                    "failed to resolve provider for session title generation"
                );
                return None;
            }
        };

        match llm_provider.chat(request).await {
            Ok(response) => normalize_session_title(&response.content),
            Err(error) => {
                warn!(?error, "failed to generate session title");
                None
            }
        }
    }

    fn generated_session_title_key(session_id: &str, turn_id: &str) -> String {
        format!("{session_id}:{turn_id}")
    }

    fn validate_run_identity(request: &AgentRunParams) -> AgentResult<()> {
        if request.context.turn_id.trim().is_empty() {
            return Err(AgentError::invalid_execution_context(
                "agent run context turn_id is required",
            ));
        }
        Ok(())
    }

    fn map_dynamic_input_error(error: TalosError) -> AgentError {
        match error {
            TalosError::Timeout(message) => AgentError::new(AgentErrorKind::Timeout, message),
            TalosError::ToolError(message) | TalosError::ValidationError(message) => {
                AgentError::input_rejected(message)
            }
            other => other.into(),
        }
    }

    async fn execute_reserved_run(
        &self,
        mode: AgentRunMode,
        request: AgentRunParams,
    ) -> AgentResult<AgentRunResult> {
        let turn_id = request.turn_id().to_string();
        let session_id = request.context.session_id.clone();
        let session_state = request.session_state.clone();
        let run_config = request.config.clone();
        let title_input = request::AgentKernelParams::extract_text_input(&request.input);
        let runtime = self.runtime.clone();
        let (message_tx, message_rx) = mpsc::channel(ACTIVE_TURN_MESSAGE_CAPACITY);
        let run_handle = tokio::spawn(async move {
            match mode {
                AgentRunMode::Start => runtime.run(request, message_rx).await,
                AgentRunMode::Resume => {
                    runtime
                        .resume(AgentResumeParams::new(request), message_rx)
                        .await
                }
            }
        });
        let abort_handle = AgentRuntimeAbortHandle::new(run_handle.abort_handle());
        if let Err(err) =
            self.activate_reserved_turn(&session_id, turn_id, abort_handle, Some(message_tx))
        {
            run_handle.abort();
            self.finish_active_turn_and_dispatch_next(&session_id).await;
            return Err(err.into());
        }

        let result = match run_handle.await {
            Ok(Ok(mut result)) => {
                result.generated_session_title = self
                    .generate_session_title(
                        &session_state,
                        &run_config,
                        &title_input,
                        &result.output_text,
                    )
                    .await;
                Ok(result)
            }
            Ok(Err(err)) => Err(err),
            Err(err) if err.is_cancelled() => Ok(AgentRunResult {
                output_text: String::new(),
                output_items: Vec::new(),
                reasoning: Default::default(),
                stop_reason: TalosStopReason::Cancelled,
                usage: TokenUsage::default(),
                task_count: 0,
                session_id: session_id.clone(),
                generated_session_title: None,
            }),
            Err(err) => Err(AgentError::runtime_error(format!(
                "kernel task join failed: {}",
                err
            ))),
        };
        self.finish_active_turn_and_dispatch_next(&session_id).await;
        result
    }

    fn spawn_reserved_stream_turn(
        &self,
        mode: AgentRunMode,
        request: AgentRunParams,
        tx: mpsc::Sender<AgentEvent>,
    ) {
        let this = self.clone();
        let stream_task = tokio::spawn(async move {
            let request_turn_id = request.turn_id().to_string();
            let session_id = request.context.session_id.clone();
            let message_id = request.context.message_id.clone();
            let session_state = request.session_state.clone();
            let run_config = request.config.clone();
            let title_input = request::AgentKernelParams::extract_text_input(&request.input);
            let title_turn_id = request_turn_id.clone();
            let runtime = this.runtime.clone();
            let (message_tx, message_rx) = mpsc::channel(ACTIVE_TURN_MESSAGE_CAPACITY);
            let runtime_stream = match mode {
                AgentRunMode::Start => runtime.run_streamed(request, message_rx).await,
                AgentRunMode::Resume => {
                    runtime
                        .resume_streamed(AgentResumeParams::new(request), message_rx)
                        .await
                }
            };
            let runtime_stream = match runtime_stream {
                Ok(runtime_stream) => runtime_stream,
                Err(err) => {
                    let mut timeline = TimelineBuilder::new(
                        "req_failed".to_string(),
                        title_turn_id.clone(),
                        message_id.clone(),
                        session_id.clone(),
                    );
                    let talos_error = err.clone().into_talos_error();
                    let _ = tx.send(timeline.fail_turn(&talos_error)).await;
                    this.finish_active_turn_and_dispatch_next(&session_id).await;
                    return;
                }
            };

            let abort_handle = runtime_stream.abort_handle.clone();
            if let Err(err) = this.activate_reserved_turn(
                &session_id,
                request_turn_id,
                abort_handle,
                Some(message_tx),
            ) {
                runtime_stream.abort_handle.abort();
                runtime_stream.completion.abort();
                let mut timeline = TimelineBuilder::new(
                    "req_failed".to_string(),
                    title_turn_id.clone(),
                    message_id.clone(),
                    session_id.clone(),
                );
                let _ = tx.send(timeline.fail_turn(&err)).await;
                this.finish_active_turn_and_dispatch_next(&session_id).await;
                return;
            }

            let mut runtime_events = runtime_stream.events;
            while let Some(event) = runtime_events.recv().await {
                if tx.send(event).await.is_err() {
                    runtime_stream.abort_handle.abort();
                    runtime_stream.completion.abort();
                    this.finish_active_turn_and_dispatch_next(&session_id).await;
                    return;
                }
            }

            match runtime_stream.completion.await {
                Ok(Ok(result)) => {
                    this.finish_active_turn_and_dispatch_next(&session_id).await;
                    if let Some(title) = this
                        .generate_session_title(
                            &session_state,
                            &run_config,
                            &title_input,
                            &result.output_text,
                        )
                        .await
                    {
                        this.generated_session_titles.insert(
                            Self::generated_session_title_key(&session_id, &title_turn_id),
                            title,
                        );
                    }
                }
                Ok(Err(_)) | Err(_) => {
                    this.finish_active_turn_and_dispatch_next(&session_id).await;
                }
            }
        });
        drop(stream_task);
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

    fn take_generated_session_title(&self, session_id: &str, turn_id: &str) -> Option<String> {
        self.generated_session_titles
            .remove(&Self::generated_session_title_key(session_id, turn_id))
            .map(|(_, title)| title)
    }

    async fn run(&self, request: AgentRunParams) -> AgentResult<AgentRunResult> {
        Self::validate_run_identity(&request)?;
        self.validate_run_request(&request)?;
        if let Some(response_rx) = self
            .reserve_or_enqueue_unary_turn(AgentRunMode::Start, request.clone())
            .await?
        {
            return response_rx.await.map_err(|err| {
                AgentError::runtime_error(format!("queued agent execution channel closed: {}", err))
            })?;
        }

        self.execute_reserved_run(AgentRunMode::Start, request)
            .await
    }

    async fn run_streamed(
        &self,
        request: AgentRunParams,
    ) -> AgentResult<mpsc::Receiver<AgentEvent>> {
        Self::validate_run_identity(&request)?;
        self.validate_streaming_run_request(&request)?;
        let (tx, rx) = mpsc::channel(256);
        let should_start_now = self
            .reserve_or_enqueue_stream_turn(AgentRunMode::Start, request.clone(), tx.clone())
            .await?;
        if should_start_now {
            self.spawn_reserved_stream_turn(AgentRunMode::Start, request, tx);
        }
        Ok(rx)
    }

    async fn resume(&self, params: AgentResumeParams) -> AgentResult<AgentRunResult> {
        Self::validate_run_identity(&params.run)?;
        self.validate_resume_request(&params)?;
        let request = params.run;
        if let Some(response_rx) = self
            .reserve_or_enqueue_unary_turn(AgentRunMode::Resume, request.clone())
            .await?
        {
            return response_rx.await.map_err(|err| {
                AgentError::runtime_error(format!("queued agent execution channel closed: {}", err))
            })?;
        }

        self.execute_reserved_run(AgentRunMode::Resume, request)
            .await
    }

    async fn resume_streamed(
        &self,
        params: AgentResumeParams,
    ) -> AgentResult<mpsc::Receiver<AgentEvent>> {
        Self::validate_run_identity(&params.run)?;
        self.validate_streaming_resume_request(&params)?;
        let request = params.run;
        let (tx, rx) = mpsc::channel(256);
        let should_start_now = self
            .reserve_or_enqueue_stream_turn(AgentRunMode::Resume, request.clone(), tx.clone())
            .await?;
        if should_start_now {
            self.spawn_reserved_stream_turn(AgentRunMode::Resume, request, tx);
        }
        Ok(rx)
    }

    async fn close_session(&self, session: talos_types::TalosSession) -> TalosResult<()> {
        self.close_session_impl(session).await
    }

    async fn compact_session(
        &self,
        params: AgentSessionCompactionParams,
    ) -> TalosResult<TokenUsage> {
        self.compact_session_impl(params).await
    }

    async fn clear_session(&self, session: talos_types::TalosSession) -> TalosResult<()> {
        self.clear_session_impl(session).await
    }

    fn has_active_turn(&self, session_id: &str) -> bool {
        AgentHarnessImpl::has_active_turn(self, session_id)
    }

    async fn interrupt(&self, params: AgentInterruptParams) -> AgentResult<AgentInterruptResult> {
        self.validate_interrupt_request(&params)?;
        let Some(handle) = self.active_turns.get(&params.session_id) else {
            return Err(AgentError::run_not_found(format!(
                "active turn for session {}",
                params.session_id
            )));
        };
        if let ActiveTurnHandle::Running {
            turn_id,
            execution_task,
            ..
        } = handle.value()
        {
            if turn_id != &params.turn_id {
                return Err(AgentError::run_not_found(format!(
                    "active turn `{}` for session {}",
                    params.turn_id, params.session_id
                )));
            }
            execution_task.abort();
            return Ok(AgentInterruptResult { interrupted: true });
        }
        Err(AgentError::run_not_found(format!(
            "active turn for session {} is not running",
            params.session_id
        )))
    }

    async fn drain_active_turns(&self, timeout: Duration) -> TalosResult<()> {
        AgentHarnessImpl::drain_active_turns(self, timeout).await
    }

    async fn submit_input(&self, params: AgentInputParams) -> AgentResult<AgentInputResult> {
        self.validate_input_request(&params)?;
        match params.input {
            AgentInput::DynamicToolResult {
                call_id,
                tool_name,
                output,
                success,
            } => {
                self.submit_dynamic_tool_result(
                    params.session,
                    &params.turn_id,
                    &call_id,
                    &tool_name,
                    output,
                    success,
                )
                .await
                .map_err(Self::map_dynamic_input_error)?;
                Ok(AgentInputResult { accepted: true })
            }
            AgentInput::Message { message } => {
                let session_id = params.session.context.session_id;
                let handle = self.active_turns.get(&session_id).ok_or_else(|| {
                    AgentError::run_not_found(format!(
                        "active turn for session {session_id} is not running"
                    ))
                })?;
                let ActiveTurnHandle::Running {
                    turn_id,
                    message_tx,
                    ..
                } = handle.value()
                else {
                    return Err(AgentError::run_not_found(format!(
                        "active turn for session {session_id} is not running"
                    )));
                };
                if turn_id != &params.turn_id {
                    return Err(AgentError::run_not_found(format!(
                        "active turn `{}` for session {session_id}",
                        params.turn_id
                    )));
                }
                let sender = message_tx.as_ref().ok_or_else(|| {
                    AgentError::input_rejected(format!(
                        "active turn `{}` does not accept messages",
                        params.turn_id
                    ))
                })?;
                sender.try_send(message).map_err(|error| match error {
                    mpsc::error::TrySendError::Full(_) => AgentError::input_rejected(format!(
                        "active turn `{}` message input is full",
                        params.turn_id
                    )),
                    mpsc::error::TrySendError::Closed(_) => AgentError::run_not_found(format!(
                        "active turn `{}` message input is closed",
                        params.turn_id
                    )),
                })?;
                Ok(AgentInputResult { accepted: true })
            }
        }
    }
}
