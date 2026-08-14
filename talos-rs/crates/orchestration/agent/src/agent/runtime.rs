use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::Arc;

use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use uuid::Uuid;

use self::resume::build_general_turn_resume;
use super::kernel;
use super::lifecycle::AgentRuntimeAbortHandle;
use super::request::{agent_run_params_to_kernel_params, AgentKernelParams};
use super::runtime_compaction::emit_context_compaction_timeline_events;
use crate::session_artifacts::SessionStoreKernelTranscriptService;
use crate::{
    AgentCapabilities, AgentError, AgentEvent, AgentKind, AgentResult, AgentResumeParams,
    AgentRunConfig, AgentRunContext, AgentRunParams, AgentRunResult, SkillRootsResolver,
};
use talos_agent_kernel::{
    run_general_turn_with_resume, AssistantTextPhase, AttachmentReader, ContextStateBags,
    GeneralTurnOutcome, GeneralTurnResult, GeneralTurnSkillOptions, InvocationContext,
    InvocationExecutionState, InvocationLimits, KernelEvent, KernelEventData, KernelEventLabel,
    KernelEventType, KernelStreamEvent, KernelStreamEventData, KernelStreamEventType,
    KernelStreamItemKey, StopReason as KernelStopReason, TimelineBuilder, ToolProgressKind,
    ToolStreamKey, TurnFileScope, UserInput, UserInputEventData,
};
use talos_config::{
    resolve_effective_kernel_workspace, validate_session_workspace_user, AgentKernelConfig,
    ToolUserPermissionModeConfig,
};
use talos_error::{TalosError, TalosResult};
use talos_llm_provider::{LlmProvider, LlmProviderRouter};
use talos_memory::MemoryWriter;
use talos_store::session::{SessionArtifactStore, SessionStore};

use talos_tools::ToolRegistry;
use talos_types::agent_dispatch::AgentDispatchResumeContext;
use talos_types::talos::{
    AgentEventPayload, RunItem, StopReason as TalosStopReason, TalosContentItem, TalosInputItem,
    TalosTimelineContent, TalosTimelineDelta, TalosTimelineItemKind, TalosTimelineItemStatus,
    TalosTimelinePhase, TalosToolProgressKind, TokenUsage, TurnExecutionMetrics,
};
use talos_types::{ModelInfo, PendingUserAction, TimelineUserActionEvent, UserActionState};

mod resume;

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
        message_rx: mpsc::Receiver<String>,
    ) -> impl Future<Output = AgentResult<AgentRunResult>> + Send;

    /// Execute a backend run and stream standardized Agent events.
    fn run_streamed(
        &self,
        params: AgentRunParams,
        message_rx: mpsc::Receiver<String>,
    ) -> impl Future<Output = AgentResult<AgentRuntimeStream>> + Send;

    /// Continue a non-streaming backend run using same-turn Agent parameters.
    fn resume(
        &self,
        params: AgentResumeParams,
        message_rx: mpsc::Receiver<String>,
    ) -> impl Future<Output = AgentResult<AgentRunResult>> + Send;

    /// Continue a backend run and stream standardized same-turn Agent events.
    fn resume_streamed(
        &self,
        params: AgentResumeParams,
        message_rx: mpsc::Receiver<String>,
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
    session_artifact_store: Arc<dyn SessionArtifactStore>,
    session_store: Arc<dyn SessionStore>,
    memory_writer: Arc<dyn MemoryWriter>,
    attachment_reader: Arc<dyn AttachmentReader>,
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
            session_artifact_store: params.session_artifact_store,
            session_store: params.session_store,
            memory_writer: params.memory_writer,
            attachment_reader: params.attachment_reader,
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

    fn kernel_config_for_run(
        &self,
        context: &AgentRunContext,
        user_permission_mode: talos_types::UserPermissionMode,
    ) -> TalosResult<AgentKernelConfig> {
        let execution_user_id = context
            .user_id
            .as_deref()
            .unwrap_or(context.owner_user_id.as_str());
        validate_session_workspace_user(&context.owner_user_id, execution_user_id)?;
        let kernel_config = kernel::normalized_kernel_config(self.kernel_config.clone());
        let mut kernel_config =
            resolve_effective_kernel_workspace(&kernel_config, &context.owner_user_id)?
                .kernel_config;
        kernel_config.tools.sandbox_policy.user_permission_mode = match user_permission_mode {
            talos_types::UserPermissionMode::AskForApproval => {
                ToolUserPermissionModeConfig::AskForApproval
            }
            talos_types::UserPermissionMode::ApproveForMe => {
                ToolUserPermissionModeConfig::ApproveForMe
            }
            talos_types::UserPermissionMode::FullAccess => ToolUserPermissionModeConfig::FullAccess,
        };
        Ok(kernel_config)
    }

    fn llm_provider_for_run(&self, config: &AgentRunConfig) -> TalosResult<Arc<dyn LlmProvider>> {
        self.llm_provider_router
            .provider_for(&config.model_selection.model_ref)
    }

    fn resolve_selected_model_info(&self, config: &AgentRunConfig) -> TalosResult<ModelInfo> {
        let selected_model = config.model_selection.model_ref.id.clone();
        let provider_name = config.model_selection.model_ref.provider.clone();

        let model = config
            .current_provider_config()
            .and_then(|provider| {
                provider
                    .models
                    .iter()
                    .find(|model| model.id == selected_model)
            })
            .ok_or_else(|| {
                TalosError::UnsupportedModel(format!("{provider_name}:{selected_model}"))
            })?;

        Ok(ModelInfo::from_provider_model(provider_name, model.clone()))
    }

    async fn build_invocation_context(
        &self,
        agent_request: &AgentKernelParams,
        input: UserInput,
        invocation_id: String,
        stream_tx: Option<mpsc::Sender<KernelStreamEvent>>,
        kernel_config: &AgentKernelConfig,
    ) -> TalosResult<InvocationContext> {
        let session_id = agent_request.context.session_id.clone();
        let message_id = agent_request.context.message_id.clone();
        let workspace_id = agent_request
            .context
            .workspace_id
            .clone()
            .unwrap_or_default();
        let resolved_attachments = self
            .attachment_reader
            .read(
                &TurnFileScope {
                    owner_user_id: agent_request.context.owner_user_id.clone(),
                    project_id: agent_request.context.project_id.clone(),
                },
                &input.file_ids,
            )
            .await?;

        let transcript_service: Arc<dyn talos_agent_kernel::KernelTranscriptService> =
            Arc::new(SessionStoreKernelTranscriptService::new(
                self.session_artifact_store.clone(),
                std::path::PathBuf::from(&kernel_config.workspace_dir),
            ));
        let session = transcript_service
            .load_session(&session_id, None, None)
            .await?;

        Ok(InvocationContext {
            session_id: session_id.clone(),
            user_id: agent_request
                .context
                .user_id
                .clone()
                .unwrap_or_else(|| agent_request.context.owner_user_id.clone()),
            message_id,
            invocation_id,
            agent_id: agent_request.context.agent_id.clone(),
            workspace_id,
            project_root: agent_request.context.project_root.clone(),
            input,
            resolved_attachments,
            model_selection: agent_request.config.model_selection.clone(),
            selected_model_info: self.resolve_selected_model_info(&agent_request.config)?,
            phase: agent_request.phase.clone(),
            planning_instruction: agent_request.planning_instruction.clone(),
            transfer_to: agent_request.transfer_to.clone(),
            execution_state: InvocationExecutionState::default(),
            session,
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
            transcript_service: Some(transcript_service),
            session_artifact_store: Some(self.session_artifact_store.clone()),
            session_store: Some(self.session_store.clone()),
            stream_tx,
        })
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

    async fn execute_unary_run(
        &self,
        request: AgentRunParams,
        message_rx: mpsc::Receiver<String>,
    ) -> AgentResult<AgentRunResult> {
        let turn_id = request.turn_id().to_string();
        let resume_context = request.resume_context.clone();
        let agent_request = agent_run_params_to_kernel_params(request);
        let llm_provider = self.llm_provider_for_run(&agent_request.config)?;
        let input = AgentKernelParams::extract_user_input(&agent_request.input)?;
        let invocation_id = turn_id.clone();
        let session_id = agent_request.context.session_id.clone();
        let kernel_config = self.kernel_config_for_run(
            &agent_request.context,
            agent_request.config.user_permission_mode,
        )?;
        let skill_options = self.build_skill_options(&agent_request.input, &kernel_config)?;
        let resume = build_general_turn_resume(
            self.session_artifact_store.as_ref(),
            std::path::Path::new(&kernel_config.workspace_dir),
            &session_id,
            &turn_id,
            resume_context,
        )
        .await?;
        let mut context = self
            .build_invocation_context(
                &agent_request,
                input.clone(),
                invocation_id,
                None,
                &kernel_config,
            )
            .await?;

        let result = run_general_turn_with_resume(
            &mut context,
            &self.name,
            &self.description,
            &input.text,
            llm_provider,
            self.tool_registry.clone(),
            self.tool_registry.clone(),
            self.memory_writer.clone(),
            &kernel_config,
            resume,
            Some(skill_options),
            Some(message_rx),
        )
        .await?;

        Ok(Self::agent_run_result_from_general_turn_result(
            &result, session_id,
        ))
    }

    async fn start_streamed_run(
        &self,
        request: AgentRunParams,
        message_rx: mpsc::Receiver<String>,
    ) -> AgentResult<AgentRuntimeStream> {
        let turn_id = request.turn_id().to_string();
        let resume_context = request.resume_context.clone();
        let agent_request = agent_run_params_to_kernel_params(request);
        let input = AgentKernelParams::extract_user_input(&agent_request.input)?;
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
        let abort_handle = AgentRuntimeAbortHandle::inner_only();
        let task_abort_handle = abort_handle.clone();
        let task: JoinHandle<AgentResult<AgentRunResult>> = tokio::spawn(async move {
            let llm_provider = match this.llm_provider_for_run(&agent_request.config) {
                Ok(llm_provider) => llm_provider,
                Err(err) => {
                    let _ = tx.send(timeline.fail_turn(&err)).await;
                    return Err(err.into());
                }
            };
            let kernel_config = match this.kernel_config_for_run(
                &agent_request.context,
                agent_request.config.user_permission_mode,
            ) {
                Ok(value) => value,
                Err(err) => {
                    let _ = tx.send(timeline.fail_turn(&err)).await;
                    return Err(err.into());
                }
            };
            let (kernel_tx, mut kernel_rx) = mpsc::channel(256);
            let kernel_ctx = match this
                .build_invocation_context(
                    &agent_request,
                    input.clone(),
                    invocation_id,
                    Some(kernel_tx),
                    &kernel_config,
                )
                .await
            {
                Ok(context) => context,
                Err(err) => {
                    let _ = tx.send(timeline.fail_turn(&err)).await;
                    return Err(err.into());
                }
            };
            let resume = match build_general_turn_resume(
                this.session_artifact_store.as_ref(),
                std::path::Path::new(&kernel_config.workspace_dir),
                &session_id,
                &turn_id,
                resume_context.clone(),
            )
            .await
            {
                Ok(resume) => resume,
                Err(err) => {
                    let _ = tx.send(timeline.fail_turn(&err)).await;
                    return Err(err.into());
                }
            };

            let _ = tx.send(timeline.start_turn()).await;
            if resume.is_none() {
                let _ = tx
                    .send(timeline.turn_metrics(TurnExecutionMetrics::default()))
                    .await;
            }
            if !input.text.trim().is_empty() || !input.file_ids.is_empty() {
                match emit_user_input_item(&mut timeline, &input) {
                    Ok(events) => {
                        for event in events {
                            let _ = tx.send(event).await;
                        }
                    }
                    Err(err) => {
                        let _ = tx.send(timeline.fail_turn(&err)).await;
                        return Err(err.into());
                    }
                }
            }

            let skill_options = match this.build_skill_options(&agent_request.input, &kernel_config)
            {
                Ok(skill_options) => skill_options,
                Err(err) => {
                    let _ = tx.send(timeline.fail_turn(&err)).await;
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
                        &input.text,
                        llm_provider,
                        this.tool_registry.clone(),
                        this.tool_registry.clone(),
                        this.memory_writer.clone(),
                        &kernel_config,
                        resume,
                        Some(skill_options),
                        Some(message_rx),
                    )
                    .await
                }
            });
            let run_abort_handle = run_handle.abort_handle();
            task_abort_handle.attach_inner(run_abort_handle.clone());
            if task_abort_handle.is_cancelled() {
                run_abort_handle.abort();
            }

            tokio::pin!(run_handle);
            let run_result = loop {
                tokio::select! {
                    kernel_event = kernel_rx.recv() => {
                        let Some(kernel_event) = kernel_event else {
                            break (&mut run_handle).await;
                        };
                        if let Err(err) = forward_kernel_event(
                            &mut timeline,
                            &mut timeline_state,
                            &tx,
                            kernel_event,
                        )
                        .await
                        {
                            run_abort_handle.abort();
                            return Err(err.into());
                        }
                    }
                    result = &mut run_handle => break result,
                }
            };

            while let Ok(kernel_event) = kernel_rx.try_recv() {
                forward_kernel_event(&mut timeline, &mut timeline_state, &tx, kernel_event)
                    .await
                    .map_err(AgentError::from)?;
            }

            match run_result {
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
                            let _ = tx.send(timeline.fail_turn(&err)).await;
                            return Err(err.into());
                        }
                    }
                    if matches!(
                        result.outcome,
                        talos_agent_kernel::GeneralTurnOutcome::Completed { .. }
                    ) {
                        let active_tool_item_ids = timeline.active_tool_item_ids();
                        if !active_tool_item_ids.is_empty() {
                            let err = TalosError::InternalError(format!(
                                "completed turn has active tool items: {}",
                                active_tool_item_ids.join(", ")
                            ));
                            emit_terminal_item_closures(
                                &mut timeline,
                                &tx,
                                TalosTimelineItemStatus::Failed,
                            )
                            .await;
                            let _ = tx.send(timeline.fail_turn(&err)).await;
                            return Err(err.into());
                        }
                        emit_success_item_closures(&mut timeline, &tx).await;
                    }
                    let _ = tx
                        .send(timeline.turn_metrics(outcome_metrics(&result.outcome)))
                        .await;
                    let _ = tx
                        .send(timeline.complete_turn(
                            kernel::map_stop_reason(outcome_stop_reason(&result.outcome)),
                            outcome_usage(&result.outcome),
                            Some(outcome_metrics(&result.outcome)),
                            Some(result.artifact_seq),
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
                            None,
                        ))
                        .await;
                    Ok(AgentRunResult {
                        output_text: String::new(),
                        output_items: Vec::new(),
                        reasoning: Default::default(),
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
                    let _ = tx.send(timeline.fail_turn(&err)).await;
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
                    let talos_error = error.clone().into_talos_error();
                    let _ = tx.send(timeline.fail_turn(&talos_error)).await;
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
    input: &UserInput,
) -> TalosResult<Vec<AgentEvent>> {
    let started = timeline.start_user_input(input.text.clone(), input.file_ids.clone());
    let item_id = item_id_from_started(&started)?;
    let completed = timeline.complete_item(&item_id, TalosTimelineItemStatus::Completed, None)?;
    Ok(vec![started, completed])
}

async fn forward_kernel_event(
    timeline: &mut TimelineBuilder,
    state: &mut KernelTimelineState,
    tx: &mpsc::Sender<AgentEvent>,
    kernel_event: KernelStreamEvent,
) -> TalosResult<()> {
    match emit_kernel_timeline_events(timeline, state, kernel_event) {
        Ok(events) => {
            for event in events {
                let _ = tx.send(event).await;
            }
            Ok(())
        }
        Err(err) => {
            emit_terminal_item_closures(timeline, tx, TalosTimelineItemStatus::Failed).await;
            let _ = tx.send(timeline.fail_turn(&err)).await;
            Err(err)
        }
    }
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

async fn emit_success_item_closures(timeline: &mut TimelineBuilder, tx: &mpsc::Sender<AgentEvent>) {
    match timeline.close_active_non_tool_items(TalosTimelineItemStatus::Completed) {
        Ok(events) => {
            for event in events {
                let _ = tx.send(event).await;
            }
        }
        Err(err) => {
            tracing::warn!(error = %err, "failed to close active non-tool timeline items");
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
    let Some(context) = resume_context else {
        return;
    };
    for tool_call in context.tool_calls {
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
            if matches!(data.phase, AssistantTextPhase::FinalAnswer)
                && state.final_answer_item_id.is_some()
            {
                return Err(TalosError::ValidationError(
                    "timeline final answer item already started".to_string(),
                ));
            }
            if matches!(data.phase, AssistantTextPhase::Commentary) {
                reject_after_final_answer(state, "commentary assistant text")?;
            }
            let started = match data.phase {
                AssistantTextPhase::Unknown => {
                    reject_after_final_answer(state, "unknown assistant text")?;
                    timeline.start_unknown_assistant_text()
                }
                AssistantTextPhase::Commentary => timeline.start_commentary(),
                AssistantTextPhase::FinalAnswer => timeline.start_final_answer(),
            };
            let item_id = item_id_from_started(&started)?;
            if matches!(data.phase, AssistantTextPhase::FinalAnswer) {
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
            if matches!(data.phase, AssistantTextPhase::Unknown) {
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
                AssistantTextPhase::Unknown => unreachable!(),
                AssistantTextPhase::Commentary => {
                    reject_after_final_answer(state, "commentary assistant text commit")?;
                    TalosTimelinePhase::Commentary
                }
                AssistantTextPhase::FinalAnswer => {
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
            let delta = timeline.append_status_delta(&item_id, TalosTimelineItemStatus::Queued)?;
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
            let delta = timeline.append_status_delta(&item_id, TalosTimelineItemStatus::Running)?;
            let progress = timeline.append_delta(
                &item_id,
                TalosTimelineDelta::ToolProgress {
                    progress_kind: TalosToolProgressKind::Status,
                    text_delta: format!("Executing {}", data.tool_name),
                },
            )?;
            events.extend(optional_started_with_delta(started, delta));
            events.push(progress);
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
            KernelStreamEventType::UserActionRequest,
            KernelStreamEventData::UserActionRequest(data),
        ) => {
            reject_after_final_answer(state, "user action request")?;
            let mut events = close_active_reasoning_item(timeline, state)?;
            let item_id = state
                .tool_item_by_call_id
                .get(&data.call_id)
                .cloned()
                .ok_or_else(|| {
                    TalosError::ValidationError(format!(
                        "timeline tool item not found for user action: {}",
                        data.call_id
                    ))
                })?;
            events.push(
                timeline
                    .append_status_delta(&item_id, TalosTimelineItemStatus::WaitingUserAction)?,
            );
            events.push(timeline.append_delta(
                &item_id,
                TalosTimelineDelta::UserAction {
                    event: TimelineUserActionEvent::Requested {
                        action: PendingUserAction {
                            action_id: data.action_id,
                            call_id: data.call_id,
                            item_id: Some(item_id.clone()),
                            state: UserActionState::Pending,
                            params: data.params,
                        },
                    },
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

pub(super) fn reject_after_final_answer(
    state: &KernelTimelineState,
    event_name: &str,
) -> TalosResult<()> {
    if state.final_answer_item_id.is_some() {
        return Err(TalosError::ValidationError(format!(
            "timeline event after final answer is not allowed: {event_name}"
        )));
    }
    Ok(())
}

pub(super) fn close_active_reasoning_item(
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

pub(super) fn item_id_from_started(event: &AgentEvent) -> TalosResult<String> {
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
    pub(crate) session_artifact_store: Arc<dyn SessionArtifactStore>,
    pub(crate) session_store: Arc<dyn SessionStore>,
    pub(crate) memory_writer: Arc<dyn MemoryWriter>,
    pub(crate) attachment_reader: Arc<dyn AttachmentReader>,
}

impl AgentRuntime for KernelAgent {
    fn kind(&self) -> AgentKind {
        AgentKind::Kernel
    }

    fn capabilities(&self) -> AgentCapabilities {
        AgentCapabilities::kernel()
    }

    async fn run(
        &self,
        params: AgentRunParams,
        message_rx: mpsc::Receiver<String>,
    ) -> AgentResult<AgentRunResult> {
        self.execute_unary_run(params, message_rx).await
    }

    async fn run_streamed(
        &self,
        params: AgentRunParams,
        message_rx: mpsc::Receiver<String>,
    ) -> AgentResult<AgentRuntimeStream> {
        self.start_streamed_run(params, message_rx).await
    }

    async fn resume(
        &self,
        params: AgentResumeParams,
        message_rx: mpsc::Receiver<String>,
    ) -> AgentResult<AgentRunResult> {
        self.execute_unary_run(params.run, message_rx).await
    }

    async fn resume_streamed(
        &self,
        params: AgentResumeParams,
        message_rx: mpsc::Receiver<String>,
    ) -> AgentResult<AgentRuntimeStream> {
        self.start_streamed_run(params.run, message_rx).await
    }
}
