use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;

use tokio::sync::mpsc;
use tokio::task::{AbortHandle, JoinHandle};
use uuid::Uuid;

use super::kernel;
use super::request::{agent_run_params_to_kernel_params, AgentKernelParams};
use crate::{
    AgentCapabilities, AgentError, AgentEvent, AgentKind, AgentResult, AgentResumeParams,
    AgentRunConfig, AgentRunContext, AgentRunParams, AgentRunResult, SkillRootsResolver,
};
use talos_agent_kernel::{
    run_general_turn_with_resume, ContextStateBags, DynamicToolResultResume, GeneralTurnOutcome,
    GeneralTurnResult, GeneralTurnResume, GeneralTurnSkillOptions, InvocationContext,
    InvocationExecutionState, InvocationLimits, InvocationSession, KernelAssistantTextPhase,
    KernelEvent, KernelEventData, KernelEventLabel, KernelEventType, KernelStreamEvent,
    KernelStreamEventData, KernelStreamEventType, KernelStreamItemKey, ResumeToolCall,
    StopReason as KernelStopReason, TimelineBuilder, ToolProgressKind, ToolStreamKey,
    UserInputEventData,
};
use talos_config::{
    resolve_effective_kernel_workspace, validate_session_workspace_user, AgentKernelConfig,
};
use talos_error::{TalosError, TalosResult};
use talos_llm_provider::{LlmProvider, LlmProviderRouter};
use talos_memory::{Memory, MemoryStore, SessionTranscriptBranch, SessionTranscriptPayload};
use talos_tools::ToolRegistry;
use talos_types::agent_dispatch::AgentDispatchResumeContext;
use talos_types::talos::{
    AgentEventPayload, ContextCompactionStage, RunItem, StopReason as TalosStopReason,
    TalosContentItem, TalosInputItem, TalosTimelineContent, TalosTimelineDelta,
    TalosTimelineItemKind, TalosTimelineItemStatus, TalosTimelinePhase, TalosToolProgressKind,
    TokenUsage, TurnExecutionMetrics,
};
use talos_types::ModelInfo;

/// Internal SPI implemented by concrete agent backends attached to AgentHarness.
///
/// The SPI is crate-private and uses the shared Agent domain params/results/events.
/// Runtime-private SDK or kernel request builders must stay inside concrete
/// runtime implementations and must not become Coordinator, Service, Gateway,
/// or SDK DTOs.
pub(crate) trait AgentRuntime: Clone + Send + Sync + 'static {
    /// Return the runtime family implemented by this backend.
    fn kind(&self) -> AgentKind;

    /// Return capabilities supported through the shared Harness API.
    fn capabilities(&self) -> AgentCapabilities;

    /// Execute a non-streaming backend run using shared Agent parameters.
    fn run(
        &self,
        params: AgentRunParams,
    ) -> impl Future<Output = AgentResult<AgentRunResult>> + Send;

    /// Execute a backend run and stream standardized Agent events.
    fn run_streamed(
        &self,
        params: AgentRunParams,
    ) -> impl Future<Output = AgentResult<AgentRuntimeStream>> + Send;

    /// Continue a non-streaming backend run using same-turn Agent parameters.
    fn resume(
        &self,
        params: AgentResumeParams,
    ) -> impl Future<Output = AgentResult<AgentRunResult>> + Send;

    /// Continue a backend run and stream standardized same-turn Agent events.
    fn resume_streamed(
        &self,
        params: AgentResumeParams,
    ) -> impl Future<Output = AgentResult<AgentRuntimeStream>> + Send;
}

/// Stream resources returned by a concrete AgentRuntime.
///
/// This is Harness-internal plumbing. The event receiver is standardized, while
/// abort handles and completion tasks must not escape to Coordinator or Service.
pub(crate) struct AgentRuntimeStream {
    /// Standardized Agent events emitted by the runtime.
    pub(crate) events: mpsc::Receiver<AgentEvent>,
    /// Harness-owned abort handle for interrupting the backend task.
    pub(crate) abort_handle: AgentRuntimeAbortHandle,
    /// Completion task that resolves to the final AgentRunResult.
    pub(crate) completion: JoinHandle<AgentResult<AgentRunResult>>,
}

/// Harness-owned abort handle for a runtime execution.
///
/// Runtime implementations can expose backend abort mechanics through this
/// wrapper, but callers outside Harness must use AgentInterruptParams instead.
#[derive(Clone)]
pub(crate) struct AgentRuntimeAbortHandle {
    outer: Option<AbortHandle>,
    inner: Arc<StdMutex<Option<AbortHandle>>>,
    cancelled: Arc<AtomicBool>,
}

impl AgentRuntimeAbortHandle {
    /// Create an abort handle for a newly spawned runtime task.
    pub(crate) fn new(outer: AbortHandle) -> Self {
        Self {
            outer: Some(outer),
            inner: Arc::new(StdMutex::new(None)),
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    fn inner_only(inner: Arc<StdMutex<Option<AbortHandle>>>) -> Self {
        Self {
            outer: None,
            inner,
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Abort all tracked runtime tasks for the active Harness run.
    pub(crate) fn abort(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        if let Some(outer) = &self.outer {
            outer.abort();
        }
        if let Some(inner) = self.inner.lock().unwrap().as_ref() {
            inner.abort();
        }
    }
}

/// Built-in Talos runtime implementation for AgentRuntime.
///
/// KernelAgent contains Kernel-specific execution details and converts them to
/// shared Agent events/results before returning to AgentHarness.
#[derive(Clone)]
pub(crate) struct KernelAgent {
    id: String,
    name: String,
    description: String,
    llm_provider_router: Arc<dyn LlmProviderRouter>,
    tool_registry: Arc<ToolRegistry>,
    kernel_config: AgentKernelConfig,
    skill_roots_resolver: Arc<dyn SkillRootsResolver>,
}

impl KernelAgent {
    /// Create the built-in Kernel runtime from Harness factory inputs.
    pub(crate) fn new(params: KernelAgentParams) -> Self {
        Self {
            id: params.id,
            name: params.name,
            description: params.description,
            llm_provider_router: params.llm_provider_router,
            tool_registry: params.tool_registry,
            kernel_config: params.kernel_config,
            skill_roots_resolver: params.skill_roots_resolver,
        }
    }

    /// Replace runtime-visible tools after Harness registry changes.
    pub(crate) fn set_tool_registry(&mut self, tool_registry: Arc<ToolRegistry>) {
        self.tool_registry = tool_registry;
    }

    /// Replace Kernel runtime configuration after Harness config changes.
    pub(crate) fn set_kernel_config(&mut self, kernel_config: AgentKernelConfig) {
        self.kernel_config = kernel_config;
    }

    fn kernel_config_for_run(&self, context: &AgentRunContext) -> TalosResult<AgentKernelConfig> {
        let execution_user_id = context
            .user_id
            .as_deref()
            .unwrap_or(context.owner_user_id.as_str());
        validate_session_workspace_user(&context.owner_user_id, execution_user_id)?;
        let kernel_config = kernel::normalized_kernel_config(self.kernel_config.clone());
        Ok(
            resolve_effective_kernel_workspace(&kernel_config, &context.owner_user_id)?
                .kernel_config,
        )
    }

    fn llm_provider_for_run(&self, config: &AgentRunConfig) -> TalosResult<Arc<dyn LlmProvider>> {
        self.llm_provider_router
            .provider_for(&talos_types::ModelRef {
                provider: config.llm.provider.clone(),
                id: config.llm.model.clone(),
            })
    }

    async fn memory_store_for_run(
        &self,
        context: &AgentRunContext,
        llm_provider: Arc<dyn LlmProvider>,
    ) -> TalosResult<(AgentKernelConfig, Arc<MemoryStore>)> {
        let kernel_config = self.kernel_config_for_run(context)?;
        let memory_store = Arc::new(MemoryStore::from_config_with_llm(
            &kernel_config,
            llm_provider,
        ));
        Ok((kernel_config, memory_store))
    }

    fn resolve_selected_model_info(&self, config: &AgentRunConfig) -> ModelInfo {
        let selected_model = config.llm.model.clone();
        let provider_name = config.llm.provider.clone();

        if let Some(provider_config) = config.current_provider_config() {
            if let Some(model) = provider_config
                .models
                .iter()
                .find(|model| model.id == selected_model)
            {
                return ModelInfo::from_provider_model(provider_name, model.clone());
            }
        }

        ModelInfo {
            id: selected_model.clone(),
            name: selected_model,
            provider: provider_name,
            context_window: 0,
            reasoning: false,
            reasoning_effort: Default::default(),
            speed: Default::default(),
            input: Vec::new(),
            cost: Default::default(),
            compat: Default::default(),
        }
    }

    fn extract_text_input(input: &[TalosInputItem]) -> String {
        input
            .iter()
            .filter_map(|item| match item {
                TalosInputItem::Text { text } => {
                    let normalized = text.trim();
                    if normalized.is_empty() {
                        None
                    } else {
                        Some(normalized.to_string())
                    }
                }
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    fn build_invocation_context(
        &self,
        agent_request: &AgentKernelParams,
        input: String,
        invocation_id: String,
        stream_tx: Option<mpsc::Sender<KernelStreamEvent>>,
        kernel_config: &AgentKernelConfig,
    ) -> InvocationContext {
        let session_id = agent_request.context.session_id.clone();
        let message_id = agent_request.context.message_id.clone();
        let workspace_id = agent_request
            .context
            .workspace_id
            .clone()
            .unwrap_or_default();

        InvocationContext {
            session_id: session_id.clone(),
            message_id,
            invocation_id,
            agent_id: self.id.clone(),
            workspace_id,
            project_root: agent_request.context.project_root.clone(),
            input,
            selected_model_info: self.resolve_selected_model_info(&agent_request.config),
            phase: agent_request.phase.clone(),
            planning_instruction: agent_request.planning_instruction.clone(),
            transfer_to: agent_request.transfer_to.clone(),
            execution_state: InvocationExecutionState::default(),
            session: InvocationSession {
                session_id: session_id.clone(),
                ..InvocationSession::default()
            },
            session_environment: agent_request.context.environment.clone(),
            state: HashMap::new(),
            state_bags: ContextStateBags::default(),
            limits: InvocationLimits::from(kernel_config),
            events: vec![KernelEvent::new(
                KernelEventType::UserInput,
                KernelEventLabel::UserInputReceived,
                KernelEventData::UserInput(UserInputEventData {
                    session_id,
                    agent_id: self.id.clone(),
                }),
            )],
            transcript_service: None,
            stream_tx,
        }
    }

    async fn build_general_turn_resume(
        memory_store: &MemoryStore,
        session_id: &str,
        turn_id: &str,
        resume_context: Option<AgentDispatchResumeContext>,
    ) -> TalosResult<Option<GeneralTurnResume>> {
        let Some(resume_context) = resume_context else {
            return Ok(None);
        };
        let Some(tool_call) = resume_context.tool_call else {
            return Err(TalosError::ValidationError(format!(
                "dynamic resume context for session `{session_id}` turn `{turn_id}` is missing tool call metadata"
            )));
        };

        let records = memory_store
            .load_transcript_records(session_id, None, SessionTranscriptBranch::ReactiveLoop)
            .await?;
        let mut request = None;
        let mut response = None;
        for record in records {
            match record.payload {
                SessionTranscriptPayload::DynamicToolRequest { payload }
                    if record.header.invocation_id.as_str() == turn_id
                        && payload.call_id.as_str() == tool_call.call_id.as_str()
                        && payload.tool_name.as_str() == tool_call.tool_name.as_str() =>
                {
                    if request
                        .as_ref()
                        .is_none_or(|(timestamp, _)| record.header.timestamp > *timestamp)
                    {
                        request = Some((record.header.timestamp, payload));
                    }
                }
                SessionTranscriptPayload::DynamicToolResponse { payload }
                    if record.header.invocation_id.as_str() == turn_id
                        && payload.call_id.as_str() == tool_call.call_id.as_str()
                        && payload.tool_name.as_str() == tool_call.tool_name.as_str()
                        && response
                            .as_ref()
                            .is_none_or(|(timestamp, _)| record.header.timestamp > *timestamp) =>
                {
                    response = Some((record.header.timestamp, payload));
                }
                _ => {}
            }
        }
        let request = request.map(|(_, payload)| payload).ok_or_else(|| {
            TalosError::ToolError(format!(
                "dynamic tool call `{}` has no pending request for session `{}` turn `{}`",
                tool_call.call_id, session_id, turn_id
            ))
        })?;
        let response = response.map(|(_, payload)| payload).ok_or_else(|| {
            TalosError::ToolError(format!(
                "dynamic tool call `{}` has no submitted result for session `{}` turn `{}`",
                tool_call.call_id, session_id, turn_id
            ))
        })?;
        let original_arguments = request.arguments;

        Ok(Some(GeneralTurnResume::DynamicToolResult(
            DynamicToolResultResume {
                call_id: tool_call.call_id.clone(),
                tool_name: tool_call.tool_name.clone(),
                output: response.output,
                success: response.success,
                original_call: Some(ResumeToolCall {
                    item_id: Some(tool_call.item_id),
                    call_id: tool_call.call_id,
                    tool_name: tool_call.tool_name,
                    arguments: original_arguments,
                }),
                replay_dynamic_tool_results: HashMap::new(),
            },
        )))
    }

    fn build_skill_options(
        &self,
        input: &[TalosInputItem],
        kernel_config: &AgentKernelConfig,
    ) -> TalosResult<GeneralTurnSkillOptions> {
        let roots = self.skill_roots_resolver.skill_roots(kernel_config)?;
        Ok(GeneralTurnSkillOptions {
            selected: AgentKernelParams::extract_skill_selections(input),
            roots: Some(roots),
        })
    }

    fn agent_run_result_from_general_turn_result(
        result: &talos_agent_kernel::GeneralTurnResult,
        session_id: String,
    ) -> AgentRunResult {
        let (output_text, output_items, stop_reason, usage) =
            execution_from_general_turn_result(result);
        let reasoning = AgentKernelParams::collect_reasoning(&output_items);
        AgentRunResult {
            output_text,
            output_items,
            reasoning,
            stop_reason,
            usage,
            task_count: 1,
            session_id,
            generated_session_title: None,
        }
    }

    async fn execute_unary_run(&self, request: AgentRunParams) -> AgentResult<AgentRunResult> {
        let turn_id = request.turn_id().to_string();
        let resume_context = request.resume_context.clone();
        let agent_request = agent_run_params_to_kernel_params(request);
        let llm_provider = self.llm_provider_for_run(&agent_request.config)?;
        let input = Self::extract_text_input(&agent_request.input);
        let invocation_id = turn_id.clone();
        let session_id = agent_request.context.session_id.clone();
        let (kernel_config, memory_store) = self
            .memory_store_for_run(&agent_request.context, llm_provider.clone())
            .await?;
        let skill_options = self.build_skill_options(&agent_request.input, &kernel_config)?;
        let resume = Self::build_general_turn_resume(
            memory_store.as_ref(),
            &session_id,
            &turn_id,
            resume_context,
        )
        .await?;
        let mut context = self.build_invocation_context(
            &agent_request,
            input.clone(),
            invocation_id,
            None,
            &kernel_config,
        );

        let result = run_general_turn_with_resume(
            &mut context,
            &self.name,
            &self.description,
            &input,
            llm_provider,
            self.tool_registry.clone(),
            self.tool_registry.clone(),
            memory_store,
            &kernel_config,
            resume,
            Some(skill_options),
        )
        .await?;

        Ok(Self::agent_run_result_from_general_turn_result(
            &result, session_id,
        ))
    }

    async fn start_streamed_run(&self, request: AgentRunParams) -> AgentResult<AgentRuntimeStream> {
        let turn_id = request.turn_id().to_string();
        let resume_context = request.resume_context.clone();
        let agent_request = agent_run_params_to_kernel_params(request);
        let input = Self::extract_text_input(&agent_request.input);
        let request_id = format!("req_{}", Uuid::new_v4().simple());
        let invocation_id = turn_id.clone();
        let session_id = agent_request.context.session_id.clone();
        let message_id = agent_request.context.message_id.clone();
        let mut timeline = TimelineBuilder::new(
            request_id.clone(),
            turn_id.clone(),
            message_id.clone(),
            session_id.clone(),
        );
        let mut timeline_state = KernelTimelineState::default();
        seed_resume_context(&mut timeline, &mut timeline_state, resume_context.clone());

        let (tx, rx) = mpsc::channel(256);
        let this = self.clone();
        let inner_abort_handle = Arc::new(StdMutex::new(None));
        let abort_handle = AgentRuntimeAbortHandle::inner_only(inner_abort_handle.clone());
        let task_abort_handle = abort_handle.clone();
        let task_inner_abort_handle = inner_abort_handle.clone();
        let task: JoinHandle<AgentResult<AgentRunResult>> = tokio::spawn(async move {
            let llm_provider = match this.llm_provider_for_run(&agent_request.config) {
                Ok(llm_provider) => llm_provider,
                Err(err) => {
                    let _ = tx.send(timeline.fail_turn(err.to_string())).await;
                    return Err(err.into());
                }
            };
            let (kernel_config, memory_store) = match this
                .memory_store_for_run(&agent_request.context, llm_provider.clone())
                .await
            {
                Ok(value) => value,
                Err(err) => {
                    let _ = tx.send(timeline.fail_turn(err.to_string())).await;
                    return Err(err.into());
                }
            };
            let (kernel_tx, mut kernel_rx) = mpsc::channel(256);
            let kernel_ctx = this.build_invocation_context(
                &agent_request,
                input.clone(),
                invocation_id,
                Some(kernel_tx),
                &kernel_config,
            );
            let resume = match Self::build_general_turn_resume(
                memory_store.as_ref(),
                &session_id,
                &turn_id,
                resume_context.clone(),
            )
            .await
            {
                Ok(resume) => resume,
                Err(err) => {
                    let _ = tx.send(timeline.fail_turn(err.to_string())).await;
                    return Err(err.into());
                }
            };

            let _ = tx.send(timeline.start_turn()).await;
            let _ = tx
                .send(timeline.turn_metrics(TurnExecutionMetrics::default()))
                .await;
            if !input.trim().is_empty() {
                match emit_user_input_item(&mut timeline, &input) {
                    Ok(events) => {
                        for event in events {
                            let _ = tx.send(event).await;
                        }
                    }
                    Err(err) => {
                        let _ = tx.send(timeline.fail_turn(err.to_string())).await;
                        return Err(err.into());
                    }
                }
            }

            let skill_options = match this.build_skill_options(&agent_request.input, &kernel_config)
            {
                Ok(skill_options) => skill_options,
                Err(err) => {
                    let _ = tx.send(timeline.fail_turn(err.to_string())).await;
                    return Err(err.into());
                }
            };

            let mut context = kernel_ctx;
            let run_handle = tokio::spawn({
                let this = this.clone();
                let input = input.clone();
                async move {
                    run_general_turn_with_resume(
                        &mut context,
                        &this.name,
                        &this.description,
                        &input,
                        llm_provider,
                        this.tool_registry.clone(),
                        this.tool_registry.clone(),
                        memory_store,
                        &kernel_config,
                        resume,
                        Some(skill_options),
                    )
                    .await
                }
            });
            task_inner_abort_handle
                .lock()
                .unwrap()
                .replace(run_handle.abort_handle());
            if task_abort_handle.cancelled.load(Ordering::SeqCst) {
                run_handle.abort();
            }

            let mut stream_failed = false;
            while let Some(kernel_event) = kernel_rx.recv().await {
                match emit_kernel_timeline_events(&mut timeline, &mut timeline_state, kernel_event)
                {
                    Ok(events) => {
                        for event in events {
                            let _ = tx.send(event).await;
                        }
                    }
                    Err(err) => {
                        emit_terminal_item_closures(
                            &mut timeline,
                            &tx,
                            TalosTimelineItemStatus::Failed,
                        )
                        .await;
                        let _ = tx.send(timeline.fail_turn(err.to_string())).await;
                        stream_failed = true;
                        run_handle.abort();
                        break;
                    }
                }
            }
            if stream_failed {
                return Err(AgentError::runtime_error(
                    "kernel stream event mapping failed".to_string(),
                ));
            }

            match run_handle.await {
                Ok(Ok(result)) => {
                    match emit_missing_tool_items_from_result(
                        &mut timeline,
                        &mut timeline_state,
                        &result.output_items,
                    ) {
                        Ok(events) => {
                            for event in events {
                                let _ = tx.send(event).await;
                            }
                        }
                        Err(err) => {
                            let _ = tx.send(timeline.fail_turn(err.to_string())).await;
                            return Err(err.into());
                        }
                    }
                    if matches!(
                        result.outcome,
                        talos_agent_kernel::GeneralTurnOutcome::Completed { .. }
                    ) {
                        emit_terminal_item_closures(
                            &mut timeline,
                            &tx,
                            TalosTimelineItemStatus::Completed,
                        )
                        .await;
                    }
                    let _ = tx
                        .send(timeline.turn_metrics(outcome_metrics(&result.outcome)))
                        .await;
                    let _ = tx
                        .send(timeline.complete_turn(
                            kernel::map_stop_reason(outcome_stop_reason(&result.outcome)),
                            outcome_usage(&result.outcome),
                            Some(outcome_metrics(&result.outcome)),
                        ))
                        .await;
                    Ok(Self::agent_run_result_from_general_turn_result(
                        &result, session_id,
                    ))
                }
                Err(err) if err.is_cancelled() => {
                    emit_terminal_item_closures(
                        &mut timeline,
                        &tx,
                        TalosTimelineItemStatus::Cancelled,
                    )
                    .await;
                    let _ = tx
                        .send(timeline.complete_turn(
                            TalosStopReason::Cancelled,
                            TokenUsage::default(),
                            None,
                        ))
                        .await;
                    Ok(AgentRunResult {
                        output_text: String::new(),
                        output_items: Vec::new(),
                        reasoning: Vec::new(),
                        stop_reason: TalosStopReason::Cancelled,
                        usage: TokenUsage::default(),
                        task_count: 0,
                        session_id,
                        generated_session_title: None,
                    })
                }
                Ok(Err(err)) => {
                    emit_terminal_item_closures(
                        &mut timeline,
                        &tx,
                        TalosTimelineItemStatus::Failed,
                    )
                    .await;
                    let _ = tx.send(timeline.fail_turn(err.to_string())).await;
                    Err(err.into())
                }
                Err(err) => {
                    emit_terminal_item_closures(
                        &mut timeline,
                        &tx,
                        TalosTimelineItemStatus::Failed,
                    )
                    .await;
                    let error = AgentError::runtime_error(format!(
                        "kernel stream task join failed: {}",
                        err
                    ));
                    let _ = tx.send(timeline.fail_turn(error.to_string())).await;
                    Err(error)
                }
            }
        });

        Ok(AgentRuntimeStream {
            events: rx,
            abort_handle,
            completion: task,
        })
    }
}

fn emit_user_input_item(
    timeline: &mut TimelineBuilder,
    input: &str,
) -> TalosResult<Vec<AgentEvent>> {
    let started = timeline.start_user_input(input.to_string());
    let item_id = item_id_from_started(&started)?;
    let completed = timeline.complete_item(&item_id, TalosTimelineItemStatus::Completed, None)?;
    Ok(vec![started, completed])
}

async fn emit_terminal_item_closures(
    timeline: &mut TimelineBuilder,
    tx: &mpsc::Sender<AgentEvent>,
    status: TalosTimelineItemStatus,
) {
    match timeline.close_active_items(status) {
        Ok(events) => {
            for event in events {
                let _ = tx.send(event).await;
            }
        }
        Err(err) => {
            tracing::warn!(error = %err, "failed to close active timeline items");
        }
    }
}

/// State used while normalizing Kernel stream events into Agent events.
///
/// This is runtime-private bookkeeping for event ordering and item identity.
/// Coordinator and Service must only receive the resulting AgentEvent values.
#[derive(Default)]
pub(crate) struct KernelTimelineState {
    pub(crate) reasoning_item_id: Option<String>,
    pub(crate) final_answer_item_id: Option<String>,
    pub(crate) assistant_text_items: HashMap<KernelStreamItemKey, String>,
    pub(crate) completed_assistant_text_items: HashMap<KernelStreamItemKey, String>,
    pub(crate) active_tool_items: HashMap<String, String>,
    pub(crate) tool_item_by_call_id: HashMap<String, String>,
    pub(crate) completed_tool_call_ids: HashSet<String>,
    pub(crate) context_compaction_item_id: Option<String>,
}

fn execution_from_general_turn_result(
    result: &GeneralTurnResult,
) -> (String, Vec<RunItem>, TalosStopReason, TokenUsage) {
    match &result.outcome {
        GeneralTurnOutcome::Completed {
            output_text,
            output_items,
            stop_reason,
            usage,
            ..
        } => (
            output_text.clone(),
            output_items.clone(),
            kernel::map_stop_reason(*stop_reason),
            usage.clone(),
        ),
        GeneralTurnOutcome::Waiting {
            output_items,
            stop_reason,
            usage,
            ..
        } => (
            String::new(),
            output_items.clone(),
            kernel::map_stop_reason(*stop_reason),
            usage.clone(),
        ),
    }
}

fn outcome_stop_reason(outcome: &GeneralTurnOutcome) -> KernelStopReason {
    match outcome {
        GeneralTurnOutcome::Completed { stop_reason, .. }
        | GeneralTurnOutcome::Waiting { stop_reason, .. } => *stop_reason,
    }
}

fn outcome_usage(outcome: &GeneralTurnOutcome) -> TokenUsage {
    match outcome {
        GeneralTurnOutcome::Completed { usage, .. } | GeneralTurnOutcome::Waiting { usage, .. } => {
            usage.clone()
        }
    }
}

fn outcome_metrics(outcome: &GeneralTurnOutcome) -> TurnExecutionMetrics {
    match outcome {
        GeneralTurnOutcome::Completed { metrics, .. }
        | GeneralTurnOutcome::Waiting { metrics, .. } => metrics.clone(),
    }
}

fn seed_resume_context(
    timeline: &mut TimelineBuilder,
    state: &mut KernelTimelineState,
    resume_context: Option<AgentDispatchResumeContext>,
) {
    let Some(tool_call) = resume_context.and_then(|context| context.tool_call) else {
        return;
    };
    timeline.track_existing_tool_call(
        tool_call.item_id.clone(),
        tool_call.call_id.clone(),
        tool_call.tool_name,
        String::new(),
    );
    state
        .tool_item_by_call_id
        .insert(tool_call.call_id.clone(), tool_call.item_id.clone());
    state
        .active_tool_items
        .insert(tool_call.call_id, tool_call.item_id);
}

/// Convert one Kernel stream event into standardized Agent timeline events.
///
/// This function is the Kernel runtime's event normalization boundary. It must
/// reject invalid Kernel event ordering instead of leaking KernelStreamEvent
/// values or backend item ids past the AgentRuntime SPI.
pub(crate) fn emit_kernel_timeline_events(
    timeline: &mut TimelineBuilder,
    state: &mut KernelTimelineState,
    event: KernelStreamEvent,
) -> TalosResult<Vec<AgentEvent>> {
    match (event.event_type, event.data) {
        (KernelStreamEventType::TurnMetrics, KernelStreamEventData::TurnMetrics(data)) => {
            Ok(vec![timeline.turn_metrics(data.metrics)])
        }
        (KernelStreamEventType::ThinkingDelta, KernelStreamEventData::ThinkingDelta(data)) => {
            reject_after_final_answer(state, "thinking delta")?;
            append_text_item_delta(timeline, &mut state.reasoning_item_id, data.delta)
        }
        (
            KernelStreamEventType::AssistantTextStarted,
            KernelStreamEventData::AssistantTextStarted(data),
        ) => {
            if state.assistant_text_items.contains_key(&data.item_key) {
                return Err(TalosError::ValidationError(format!(
                    "timeline assistant text item already started: {:?}",
                    data.item_key
                )));
            }
            state.completed_assistant_text_items.remove(&data.item_key);
            if matches!(data.phase, KernelAssistantTextPhase::FinalAnswer)
                && state.final_answer_item_id.is_some()
            {
                return Err(TalosError::ValidationError(
                    "timeline final answer item already started".to_string(),
                ));
            }
            if matches!(data.phase, KernelAssistantTextPhase::Commentary) {
                reject_after_final_answer(state, "commentary assistant text")?;
            }
            let started = match data.phase {
                KernelAssistantTextPhase::Unknown => {
                    reject_after_final_answer(state, "unknown assistant text")?;
                    timeline.start_unknown_assistant_text()
                }
                KernelAssistantTextPhase::Commentary => timeline.start_commentary(),
                KernelAssistantTextPhase::FinalAnswer => timeline.start_final_answer(),
            };
            let item_id = item_id_from_started(&started)?;
            if matches!(data.phase, KernelAssistantTextPhase::FinalAnswer) {
                state.final_answer_item_id = Some(item_id.clone());
            }
            state.assistant_text_items.insert(data.item_key, item_id);
            Ok(vec![started])
        }
        (
            KernelStreamEventType::AssistantTextDelta,
            KernelStreamEventData::AssistantTextDelta(data),
        ) => {
            let item_id = state
                .assistant_text_items
                .get(&data.item_key)
                .cloned()
                .ok_or_else(|| {
                    TalosError::ValidationError(format!(
                        "timeline assistant text item not found for delta: {:?}",
                        data.item_key
                    ))
                })?;
            Ok(vec![timeline.append_delta(
                &item_id,
                TalosTimelineDelta::Text { text: data.delta },
            )?])
        }
        (
            KernelStreamEventType::AssistantTextCompleted,
            KernelStreamEventData::AssistantTextCompleted(data),
        ) => {
            let item_id = state
                .assistant_text_items
                .remove(&data.item_key)
                .ok_or_else(|| {
                    TalosError::ValidationError(format!(
                        "timeline assistant text item not found for completion: {:?}",
                        data.item_key
                    ))
                })?;
            Ok(vec![timeline.complete_item(
                &item_id,
                TalosTimelineItemStatus::Completed,
                None,
            )?])
            .inspect(|_events| {
                state
                    .completed_assistant_text_items
                    .insert(data.item_key, item_id);
            })
        }
        (
            KernelStreamEventType::AssistantTextCommitted,
            KernelStreamEventData::AssistantTextCommitted(data),
        ) => {
            if matches!(data.phase, KernelAssistantTextPhase::Unknown) {
                return Err(TalosError::ValidationError(
                    "timeline assistant text commit phase cannot be unknown".to_string(),
                ));
            }
            let item_id = state
                .assistant_text_items
                .get(&data.item_key)
                .or_else(|| state.completed_assistant_text_items.get(&data.item_key))
                .cloned()
                .ok_or_else(|| {
                    TalosError::ValidationError(format!(
                        "timeline assistant text item not found for commit: {:?}",
                        data.item_key
                    ))
                })?;
            let phase = match data.phase {
                KernelAssistantTextPhase::Unknown => unreachable!(),
                KernelAssistantTextPhase::Commentary => {
                    reject_after_final_answer(state, "commentary assistant text commit")?;
                    TalosTimelinePhase::Commentary
                }
                KernelAssistantTextPhase::FinalAnswer => {
                    if state
                        .final_answer_item_id
                        .as_ref()
                        .is_some_and(|existing| existing != &item_id)
                    {
                        return Err(TalosError::ValidationError(
                            "timeline final answer item already started".to_string(),
                        ));
                    }
                    state.final_answer_item_id = Some(item_id.clone());
                    TalosTimelinePhase::FinalAnswer
                }
            };
            Ok(vec![timeline.commit_assistant_text(&item_id, phase)?])
        }
        (
            KernelStreamEventType::ExecutionSummary,
            KernelStreamEventData::ExecutionSummary(data),
        ) => {
            reject_after_final_answer(state, "execution summary")?;
            let mut events = close_active_reasoning_item(timeline, state)?;
            let started = timeline.start_item(
                TalosTimelineItemKind::ExecutionSummary,
                TalosTimelinePhase::System,
                Some("Execution summary".to_string()),
                TalosTimelineContent::ExecutionSummary { summary: data.text },
            );
            let item_id = item_id_from_started(&started)?;
            let completed =
                timeline.complete_item(&item_id, TalosTimelineItemStatus::Completed, None)?;
            events.push(started);
            events.push(completed);
            Ok(events)
        }
        (
            KernelStreamEventType::ContextCompaction,
            KernelStreamEventData::ContextCompaction(data),
        ) => emit_context_compaction_timeline_events(timeline, state, data),
        (KernelStreamEventType::ToolCallStart, KernelStreamEventData::ToolCallStart(data)) => {
            reject_after_final_answer(state, "tool call start")?;
            let mut events = close_active_reasoning_item(timeline, state)?;
            let stream_key = ToolStreamKey {
                round: data.round,
                index: data.index,
            };
            let call_id = data
                .call_id
                .unwrap_or_else(|| stream_key_fallback(&stream_key));
            let tool_name = data.tool_name.unwrap_or_else(|| "unknown".to_string());
            let (_, started) = ensure_tool_item(
                timeline,
                state,
                stream_key,
                Some(&call_id),
                Some(&tool_name),
                data.arguments.to_string(),
            )?;
            events.extend(started);
            Ok(events)
        }
        (KernelStreamEventType::ToolArgsDelta, KernelStreamEventData::ToolArgsDelta(data)) => {
            reject_after_final_answer(state, "tool arguments delta")?;
            let mut events = close_active_reasoning_item(timeline, state)?;
            let (item_id, started) = ensure_tool_item(
                timeline,
                state,
                data.stream_key,
                data.call_id.as_deref(),
                data.tool_name.as_deref(),
                String::new(),
            )?;
            let delta = timeline.append_delta(
                &item_id,
                TalosTimelineDelta::ToolArguments {
                    arguments_delta: data.delta,
                },
            )?;
            events.extend(optional_started_with_delta(started, delta));
            Ok(events)
        }
        (KernelStreamEventType::ToolQueued, KernelStreamEventData::ToolQueued(data)) => {
            reject_after_final_answer(state, "tool queued")?;
            let mut events = close_active_reasoning_item(timeline, state)?;
            let (item_id, started) = ensure_tool_item(
                timeline,
                state,
                data.stream_key,
                Some(&data.call_id),
                Some(&data.tool_name),
                String::new(),
            )?;
            let delta = timeline.append_delta(
                &item_id,
                TalosTimelineDelta::Status {
                    status: TalosTimelineItemStatus::Running,
                },
            )?;
            events.extend(optional_started_with_delta(started, delta));
            Ok(events)
        }
        (KernelStreamEventType::ToolExecuting, KernelStreamEventData::ToolExecuting(data)) => {
            reject_after_final_answer(state, "tool executing")?;
            let mut events = close_active_reasoning_item(timeline, state)?;
            let (item_id, started) = ensure_tool_item(
                timeline,
                state,
                data.stream_key,
                Some(&data.call_id),
                Some(&data.tool_name),
                String::new(),
            )?;
            let delta = timeline.append_delta(
                &item_id,
                TalosTimelineDelta::ToolProgress {
                    progress_kind: TalosToolProgressKind::Status,
                    text_delta: format!("Executing {}", data.tool_name),
                },
            )?;
            events.extend(optional_started_with_delta(started, delta));
            Ok(events)
        }
        (
            KernelStreamEventType::ToolProgressDelta,
            KernelStreamEventData::ToolProgressDelta(data),
        ) => {
            reject_after_final_answer(state, "tool progress delta")?;
            let mut events = close_active_reasoning_item(timeline, state)?;
            let (item_id, started) = ensure_tool_item(
                timeline,
                state,
                data.stream_key,
                Some(&data.call_id),
                Some(&data.tool_name),
                String::new(),
            )?;
            let delta = timeline.append_delta(
                &item_id,
                TalosTimelineDelta::ToolProgress {
                    progress_kind: map_timeline_tool_progress_kind(data.progress_kind),
                    text_delta: data.message,
                },
            )?;
            events.extend(optional_started_with_delta(started, delta));
            Ok(events)
        }
        (KernelStreamEventType::ToolResult, KernelStreamEventData::ToolResult(data)) => {
            reject_after_final_answer(state, "tool result")?;
            let mut events = close_active_reasoning_item(timeline, state)?;
            let (item_id, started) =
                ensure_tool_item_for_call(timeline, state, &data.call_id, &data.tool_name);
            let result = timeline.append_delta(
                &item_id,
                TalosTimelineDelta::ToolResult {
                    content: if data.content.is_empty() {
                        vec![TalosContentItem::Json {
                            value: serde_json::Value::Array(Vec::new()).into(),
                        }]
                    } else {
                        data.content
                    },
                    is_error: data.is_error,
                },
            )?;
            if data.next_turn_failure.is_some() {
                events.extend(optional_started_with_delta(started, result));
                return Ok(events);
            }
            let completed = timeline.complete_item(
                &item_id,
                if data.is_error {
                    TalosTimelineItemStatus::Failed
                } else {
                    TalosTimelineItemStatus::Completed
                },
                None,
            )?;
            state.completed_tool_call_ids.insert(data.call_id);
            events.extend(optional_started_with_delta(started, result));
            events.push(completed);
            Ok(events)
        }
        (
            KernelStreamEventType::DynamicToolRequest,
            KernelStreamEventData::DynamicToolRequest(data),
        ) => {
            reject_after_final_answer(state, "dynamic tool request")?;
            let mut events = close_active_reasoning_item(timeline, state)?;
            let item_id = state
                .tool_item_by_call_id
                .get(&data.call_id)
                .cloned()
                .ok_or_else(|| {
                    TalosError::ValidationError(format!(
                        "timeline tool item not found for dynamic result: {}",
                        data.call_id
                    ))
                })?;
            events.push(timeline.append_delta(
                &item_id,
                TalosTimelineDelta::Status {
                    status: TalosTimelineItemStatus::WaitingDynamicInput,
                },
            )?);
            events.push(timeline.append_delta(
                &item_id,
                TalosTimelineDelta::ToolDynamicRequest {
                    call_id: data.call_id,
                    tool_name: data.tool_name,
                    arguments: data.arguments.into(),
                },
            )?);
            Ok(events)
        }
        (KernelStreamEventType::PromptWarning, KernelStreamEventData::PromptWarning(_)) => {
            Ok(Vec::new())
        }
        _ => Ok(Vec::new()),
    }
}

fn reject_after_final_answer(state: &KernelTimelineState, event_name: &str) -> TalosResult<()> {
    if state.final_answer_item_id.is_some() {
        return Err(TalosError::ValidationError(format!(
            "timeline event after final answer is not allowed: {event_name}"
        )));
    }
    Ok(())
}

fn close_active_reasoning_item(
    timeline: &mut TimelineBuilder,
    state: &mut KernelTimelineState,
) -> TalosResult<Vec<AgentEvent>> {
    let Some(item_id) = state.reasoning_item_id.clone() else {
        return Ok(Vec::new());
    };
    let completed = timeline.complete_item(&item_id, TalosTimelineItemStatus::Completed, None)?;
    state.reasoning_item_id = None;
    Ok(vec![completed])
}

fn emit_context_compaction_timeline_events(
    timeline: &mut TimelineBuilder,
    state: &mut KernelTimelineState,
    data: talos_agent_kernel::ContextCompactionStreamData,
) -> TalosResult<Vec<AgentEvent>> {
    reject_after_final_answer(state, "context compaction")?;
    let mut events = close_active_reasoning_item(timeline, state)?;
    let item_id = match state.context_compaction_item_id.clone() {
        Some(item_id) => item_id,
        None => {
            let started =
                timeline.start_context_compaction(data.trigger, data.stage, data.message.clone());
            let item_id = item_id_from_started(&started)?;
            state.context_compaction_item_id = Some(item_id.clone());
            events.push(started);
            item_id
        }
    };
    events.push(timeline.append_delta(
        &item_id,
        TalosTimelineDelta::ContextCompaction {
            stage: Some(data.stage),
            outcome: data.outcome,
            metrics: data.metrics.clone(),
            message: data.message.clone(),
        },
    )?);
    let status = match data.stage {
        ContextCompactionStage::Completed => Some(TalosTimelineItemStatus::Completed),
        ContextCompactionStage::Failed => Some(TalosTimelineItemStatus::Failed),
        ContextCompactionStage::Preparing
        | ContextCompactionStage::Summarizing
        | ContextCompactionStage::RebuildingPrompt
        | ContextCompactionStage::Trimming => None,
    };
    if let Some(status) = status {
        let final_content = timeline.item(&item_id).map(|item| item.content.clone());
        events.push(timeline.complete_item(&item_id, status, final_content)?);
        state.context_compaction_item_id = None;
    }
    Ok(events)
}

fn append_text_item_delta(
    timeline: &mut TimelineBuilder,
    item_id_slot: &mut Option<String>,
    text: String,
) -> TalosResult<Vec<AgentEvent>> {
    let mut events = Vec::new();
    let item_id = match item_id_slot.clone() {
        Some(item_id) => item_id,
        None => {
            let started = timeline.start_reasoning();
            let item_id = item_id_from_started(&started)?;
            *item_id_slot = Some(item_id.clone());
            events.push(started);
            item_id
        }
    };
    events.push(timeline.append_delta(&item_id, TalosTimelineDelta::Text { text })?);
    Ok(events)
}

fn emit_missing_tool_items_from_result(
    timeline: &mut TimelineBuilder,
    state: &mut KernelTimelineState,
    output_items: &[RunItem],
) -> TalosResult<Vec<AgentEvent>> {
    let mut events = Vec::new();
    for output_item in output_items {
        match output_item {
            RunItem::ToolCall {
                call_id,
                name,
                arguments,
            } if !state.tool_item_by_call_id.contains_key(call_id) => {
                events.extend(close_active_reasoning_item(timeline, state)?);
                let started = timeline.start_tool_call(call_id, name, arguments.to_string());
                let item_id = item_id_from_started(&started)?;
                state
                    .tool_item_by_call_id
                    .insert(call_id.clone(), item_id.clone());
                state.active_tool_items.insert(call_id.clone(), item_id);
                events.push(started);
            }
            RunItem::ToolResult {
                call_id,
                tool_name,
                content,
                is_error,
            } if !state.completed_tool_call_ids.contains(call_id) => {
                events.extend(close_active_reasoning_item(timeline, state)?);
                let item_id = match state.tool_item_by_call_id.get(call_id).cloned() {
                    Some(item_id) => item_id,
                    None => {
                        let started = timeline.start_tool_call(call_id, tool_name, String::new());
                        let item_id = item_id_from_started(&started)?;
                        state
                            .tool_item_by_call_id
                            .insert(call_id.clone(), item_id.clone());
                        state
                            .active_tool_items
                            .insert(call_id.clone(), item_id.clone());
                        events.push(started);
                        item_id
                    }
                };
                let result = timeline.append_delta(
                    &item_id,
                    TalosTimelineDelta::ToolResult {
                        content: content.clone(),
                        is_error: *is_error,
                    },
                )?;
                events.push(result);
                events.push(timeline.complete_item(
                    &item_id,
                    if *is_error {
                        TalosTimelineItemStatus::Failed
                    } else {
                        TalosTimelineItemStatus::Completed
                    },
                    None,
                )?);
                state.completed_tool_call_ids.insert(call_id.clone());
            }
            _ => {}
        }
    }
    Ok(events)
}

fn item_id_from_started(event: &AgentEvent) -> TalosResult<String> {
    match &event.payload {
        AgentEventPayload::ItemStarted(payload) => Ok(payload.item.item_id.clone()),
        _ => Err(TalosError::InternalError(
            "expected item started timeline event".to_string(),
        )),
    }
}

fn tool_item_id(
    state: &KernelTimelineState,
    stream_key: ToolStreamKey,
    call_id: Option<&str>,
) -> TalosResult<String> {
    if let Some(call_id) = call_id {
        if let Some(item_id) = state.tool_item_by_call_id.get(call_id) {
            return Ok(item_id.clone());
        }
    }
    state
        .active_tool_items
        .get(&stream_key_key(&stream_key))
        .cloned()
        .ok_or_else(|| {
            TalosError::ValidationError(format!(
                "timeline tool item not found for stream key round={} index={}",
                stream_key.round, stream_key.index
            ))
        })
}

fn ensure_tool_item(
    timeline: &mut TimelineBuilder,
    state: &mut KernelTimelineState,
    stream_key: ToolStreamKey,
    call_id: Option<&str>,
    tool_name: Option<&str>,
    arguments: String,
) -> TalosResult<(String, Option<AgentEvent>)> {
    if let Ok(item_id) = tool_item_id(state, stream_key.clone(), call_id) {
        return Ok((item_id, None));
    }

    let call_id = call_id
        .map(ToString::to_string)
        .unwrap_or_else(|| stream_key_fallback(&stream_key));
    let tool_name = tool_name.unwrap_or("unknown");
    let started = timeline.start_tool_call(call_id.clone(), tool_name, arguments);
    let item_id = item_id_from_started(&started)?;
    state
        .active_tool_items
        .insert(stream_key_key(&stream_key), item_id.clone());
    state.tool_item_by_call_id.insert(call_id, item_id.clone());
    Ok((item_id, Some(started)))
}

fn ensure_tool_item_for_call(
    timeline: &mut TimelineBuilder,
    state: &mut KernelTimelineState,
    call_id: &str,
    tool_name: &str,
) -> (String, Option<AgentEvent>) {
    if let Some(item_id) = state.tool_item_by_call_id.get(call_id).cloned() {
        return (item_id, None);
    }

    let started = timeline.start_tool_call(call_id, tool_name, String::new());
    let item_id = match item_id_from_started(&started) {
        Ok(item_id) => item_id,
        Err(_) => return (call_id.to_string(), Some(started)),
    };
    state
        .tool_item_by_call_id
        .insert(call_id.to_string(), item_id.clone());
    state
        .active_tool_items
        .insert(call_id.to_string(), item_id.clone());
    (item_id, Some(started))
}

fn optional_started_with_delta(started: Option<AgentEvent>, delta: AgentEvent) -> Vec<AgentEvent> {
    match started {
        Some(started) => vec![started, delta],
        None => vec![delta],
    }
}

fn map_timeline_tool_progress_kind(kind: ToolProgressKind) -> TalosToolProgressKind {
    match kind {
        ToolProgressKind::Status => TalosToolProgressKind::Status,
        ToolProgressKind::Stdout => TalosToolProgressKind::Stdout,
        ToolProgressKind::Stderr => TalosToolProgressKind::Stderr,
    }
}

fn stream_key_fallback(stream_key: &ToolStreamKey) -> String {
    format!("tool_{}_{}", stream_key.round, stream_key.index)
}

fn stream_key_key(stream_key: &ToolStreamKey) -> String {
    format!("{}:{}", stream_key.round, stream_key.index)
}

/// Construction parameters for the built-in KernelAgent runtime.
///
/// These are factory inputs only. They must not become Coordinator routing DTOs
/// or Gateway/SDK public configuration objects.
pub(crate) struct KernelAgentParams {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) llm_provider_router: Arc<dyn LlmProviderRouter>,
    pub(crate) tool_registry: Arc<ToolRegistry>,
    pub(crate) kernel_config: AgentKernelConfig,
    pub(crate) skill_roots_resolver: Arc<dyn SkillRootsResolver>,
}

impl AgentRuntime for KernelAgent {
    fn kind(&self) -> AgentKind {
        AgentKind::Kernel
    }

    fn capabilities(&self) -> AgentCapabilities {
        AgentCapabilities::kernel()
    }

    async fn run(&self, params: AgentRunParams) -> AgentResult<AgentRunResult> {
        self.execute_unary_run(params).await
    }

    async fn run_streamed(&self, params: AgentRunParams) -> AgentResult<AgentRuntimeStream> {
        self.start_streamed_run(params).await
    }

    async fn resume(&self, params: AgentResumeParams) -> AgentResult<AgentRunResult> {
        self.execute_unary_run(params.run).await
    }

    async fn resume_streamed(&self, params: AgentResumeParams) -> AgentResult<AgentRuntimeStream> {
        self.start_streamed_run(params.run).await
    }
}
