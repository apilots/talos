use crate::execution::ContentsMode;
use crate::general_turn::{
    apply_general_turn_step, apply_general_turn_tool_batch, build_general_turn_step_request,
    emit_general_turn_finalization_stream, execute_general_turn_step,
    execute_general_turn_tool_batch, finalize_general_turn_artifacts,
    persist_general_turn_artifacts, prepare_general_turn, GeneralTurnLoopState,
    GeneralTurnStepDisposition, GeneralTurnToolBatch, PlanReActLoopState,
};
use crate::{
    execution::DynamicToolRequestPayload, CallbackControl, CallbackRegistry,
    ConfirmationRequestPayload, ConfirmationRequestStreamData, EventAuthor, EventPayload,
    ExecutionBranch, ExecutionEvent, ExecutionEventKind, FlowKind, FlowProfile, InvocationContext,
    InvocationPluginManager, KernelExecutionSettings, KernelStreamEvent, KernelStreamEventData,
    KernelStreamEventType, KernelThinkingSource, KernelThinkingStage, KernelWarning,
    LlmRequestEnvelope, LlmResponseEnvelope, MemoryBackedSessionService, Planner,
    RequestProcessorPipeline, ResponseProcessorPipeline, SessionEvent, SessionEventKind,
    SessionEventPayload, SessionService, StopReason, ThinkingDeltaStreamData, ToolCallbackPayload,
    ToolExecutor, ToolExecutorFactory,
};
use async_trait::async_trait;
use std::sync::Arc;
use talos_config::AgentKernelConfig;
use talos_error::TalosResult;
use talos_llm_provider::LlmProvider;
use talos_memory::MemoryStore;
use talos_tools::{ToolDefinitionProvider, ToolExecutorPort};
use talos_types::talos::{TalosOutputItem, TalosUsage};
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub struct FlowRunSummary {
    pub output_text: String,
    pub output_items: Vec<TalosOutputItem>,
    pub stop_reason: StopReason,
    pub usage: TalosUsage,
    pub warnings: Vec<KernelWarning>,
}

#[derive(Debug, Clone)]
pub struct FlowExecution {
    pub events: Vec<ExecutionEvent>,
    pub summary: FlowRunSummary,
}

#[async_trait]
pub trait BaseLlmFlow: Send + Sync {
    fn settings(&self) -> &KernelExecutionSettings;
    fn profile(&self) -> FlowProfile;
    async fn run_events(
        &self,
        ctx: &mut InvocationContext,
        input: &str,
    ) -> TalosResult<FlowExecution>;
}

pub struct SingleFlow {
    agent_name: String,
    agent_description: String,
    llm_provider: Arc<dyn LlmProvider>,
    tool_definition_provider: Arc<dyn ToolDefinitionProvider>,
    tool_executor_port: Arc<dyn ToolExecutorPort>,
    memory_store: Arc<MemoryStore>,
    kernel_config: AgentKernelConfig,
    #[allow(dead_code)]
    planner: Arc<dyn Planner>,
    settings: KernelExecutionSettings,
    flow_profile: FlowProfile,
    allow_transfer: bool,
    request_pipeline: RequestProcessorPipeline,
    response_pipeline: ResponseProcessorPipeline,
    plugin_manager: Arc<InvocationPluginManager>,
    tool_executor: Arc<dyn ToolExecutor>,
    session_service: Arc<dyn SessionService>,
}

impl SingleFlow {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        agent_name: impl Into<String>,
        agent_description: impl Into<String>,
        llm_provider: Arc<dyn LlmProvider>,
        tool_definition_provider: Arc<dyn ToolDefinitionProvider>,
        tool_executor_port: Arc<dyn ToolExecutorPort>,
        memory_store: Arc<MemoryStore>,
        kernel_config: AgentKernelConfig,
        planner: Arc<dyn Planner>,
    ) -> Self {
        let agent_name = agent_name.into();
        let agent_description = agent_description.into();
        let settings = KernelExecutionSettings::try_from(&kernel_config).unwrap_or_default();
        let flow_profile = select_flow_profile(&settings, tool_definition_provider.as_ref());
        let tool_executor = ToolExecutorFactory::build(settings.tool_execution_mode);
        Self {
            agent_name: agent_name.clone(),
            agent_description: agent_description.clone(),
            llm_provider,
            tool_definition_provider,
            tool_executor_port,
            memory_store: memory_store.clone(),
            kernel_config,
            planner: planner.clone(),
            settings,
            flow_profile,
            allow_transfer: false,
            request_pipeline: RequestProcessorPipeline::for_profile(
                flow_profile,
                planner.clone(),
                agent_name,
                agent_description,
                false,
            ),
            response_pipeline: ResponseProcessorPipeline::for_profile(flow_profile, planner),
            plugin_manager: Arc::new(InvocationPluginManager::new()),
            tool_executor,
            session_service: Arc::new(MemoryBackedSessionService::new(memory_store.clone())),
        }
    }

    pub fn settings(&self) -> &KernelExecutionSettings {
        &self.settings
    }

    pub fn tool_execution_mode(&self) -> crate::ToolExecutionMode {
        self.tool_executor.mode()
    }

    pub fn current_branch(&self) -> ExecutionBranch {
        match self.flow_profile {
            FlowProfile::DirectAnswer => ExecutionBranch::DirectAnswer,
            FlowProfile::ModelThinking => ExecutionBranch::ModelThinking,
            FlowProfile::ReactiveLoop => ExecutionBranch::ReactiveLoop,
        }
    }

    pub fn with_callbacks(mut self, callbacks: Arc<CallbackRegistry>) -> Self {
        self.plugin_manager = Arc::new(InvocationPluginManager::new().with_callbacks(callbacks));
        self
    }

    pub fn with_plugin_manager(mut self, plugin_manager: Arc<InvocationPluginManager>) -> Self {
        self.plugin_manager = plugin_manager;
        self
    }

    pub fn with_tool_executor(mut self, tool_executor: Arc<dyn ToolExecutor>) -> Self {
        self.tool_executor = tool_executor;
        self
    }

    pub fn with_session_service(mut self, session_service: Arc<dyn SessionService>) -> Self {
        self.session_service = session_service;
        self
    }

    fn rebuild_pipelines(&mut self) {
        self.request_pipeline = RequestProcessorPipeline::for_profile(
            self.flow_profile,
            self.planner.clone(),
            self.agent_name.clone(),
            self.agent_description.clone(),
            self.allow_transfer,
        );
        self.response_pipeline =
            ResponseProcessorPipeline::for_profile(self.flow_profile, self.planner.clone());
    }

    async fn observe_tool_callbacks(
        &self,
        ctx: &mut InvocationContext,
        output_items: &[TalosOutputItem],
    ) -> TalosResult<()> {
        for item in output_items {
            match item {
                TalosOutputItem::ToolCall { call_id, name, .. } => {
                    let payload = ToolCallbackPayload {
                        tool_name: name.clone(),
                        call_id: call_id.clone(),
                    };
                    let plugin_manager = ctx.plugin_manager.clone();
                    if matches!(
                        plugin_manager.before_tool(ctx, &payload).await?,
                        CallbackControl::Stop
                    ) {
                        break;
                    }
                }
                TalosOutputItem::ToolResult {
                    call_id, tool_name, ..
                } => {
                    let payload = ToolCallbackPayload {
                        tool_name: tool_name.clone(),
                        call_id: call_id.clone(),
                    };
                    let plugin_manager = ctx.plugin_manager.clone();
                    if matches!(
                        plugin_manager.after_tool(ctx, &payload).await?,
                        CallbackControl::Stop
                    ) {
                        break;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
}

fn build_flow_step_model_request(
    planning_instruction: Option<String>,
    native_thinking: Option<talos_llm_provider::NativeThinkingConfig>,
    native_thinking_support: talos_llm_provider::NativeThinkingSupport,
    branch: ExecutionBranch,
    prepared: &crate::general_turn::PreparedGeneralTurn,
    state: &GeneralTurnLoopState,
    settings: &KernelExecutionSettings,
) -> LlmRequestEnvelope {
    LlmRequestEnvelope {
        prompt_prefix_messages: prepared.prompt_prefix_messages.clone(),
        transient_messages: state.messages.clone(),
        planning_instruction,
        tool_definitions: prepared.tool_definitions.clone(),
        parallel_tool_calls: settings.tool_execution_mode == crate::ToolExecutionMode::Parallel,
        contents_mode: ContentsMode::FullSession,
        branch,
        native_thinking,
        native_thinking_support,
        ..LlmRequestEnvelope::default()
    }
}

fn stage_session_event(ctx: &mut InvocationContext, event: SessionEvent) {
    for delta in &event.actions.state_delta {
        ctx.session.state.apply_delta(delta);
    }
    ctx.session.events.push(event);
}

async fn emit_framework_thinking(
    stream_tx: Option<&mpsc::Sender<KernelStreamEvent>>,
    stage: KernelThinkingStage,
    round: u32,
    delta: impl Into<String>,
) {
    let Some(tx) = stream_tx else {
        return;
    };
    let _ = tx
        .send(KernelStreamEvent::new(
            KernelStreamEventType::ThinkingDelta,
            KernelStreamEventData::ThinkingDelta(ThinkingDeltaStreamData {
                delta: delta.into(),
                source: KernelThinkingSource::Framework,
                stage,
                round: Some(round),
            }),
        ))
        .await;
}

fn tool_names_for_thinking(output_items: &[TalosOutputItem]) -> Vec<String> {
    output_items
        .iter()
        .filter_map(|item| match item {
            TalosOutputItem::ToolCall { name, .. } => Some(name.clone()),
            TalosOutputItem::ToolResult { tool_name, .. } => Some(tool_name.clone()),
            _ => None,
        })
        .collect()
}

fn has_final_response(events: &[ExecutionEvent], branch: ExecutionBranch) -> bool {
    events
        .iter()
        .rev()
        .any(|event| event.branch == branch && event.is_final_response())
}

#[async_trait]
impl BaseLlmFlow for SingleFlow {
    fn settings(&self) -> &KernelExecutionSettings {
        &self.settings
    }

    fn profile(&self) -> FlowProfile {
        self.flow_profile
    }

    async fn run_events(
        &self,
        ctx: &mut InvocationContext,
        input: &str,
    ) -> TalosResult<FlowExecution> {
        ctx.plugin_manager = Arc::new(ctx.plugin_manager.merged_with(&self.plugin_manager));
        let mut events = Vec::new();
        let branch = self.current_branch();
        let native_thinking_support = self.llm_provider.native_thinking_support(&ctx.model);
        let session_service = ctx
            .session_service
            .clone()
            .unwrap_or_else(|| self.session_service.clone());
        ctx.session = session_service
            .load_session(&ctx.session_id, None, Some(branch))
            .await?;
        events.push(ExecutionEvent::text(
            ctx,
            EventAuthor::User,
            branch,
            ExecutionEventKind::UserInput,
            ctx.input.clone(),
            false,
        ));
        let mut request = LlmRequestEnvelope {
            planning_instruction: ctx.planning_instruction.clone(),
            contents_mode: ContentsMode::FullSession,
            branch,
            native_thinking_support,
            ..LlmRequestEnvelope::default()
        };
        events.extend(self.request_pipeline.run(ctx, &mut request).await?);
        let plugin_manager = ctx.plugin_manager.clone();
        if matches!(
            plugin_manager.before_model(ctx, &mut request).await?,
            CallbackControl::Stop
        ) {
            let stop_reason = if has_final_response(&events, branch) {
                StopReason::EndTurn
            } else {
                StopReason::BeforeModelCallbackStopped
            };
            return Ok(FlowExecution {
                events,
                summary: FlowRunSummary {
                    output_text: String::new(),
                    output_items: Vec::new(),
                    stop_reason,
                    usage: TalosUsage::default(),
                    warnings: Vec::new(),
                },
            });
        }

        let prepared = prepare_general_turn(
            ctx,
            &self.agent_name,
            &self.agent_description,
            input,
            self.tool_definition_provider.clone(),
            self.memory_store.clone(),
            &self.kernel_config,
        )
        .await?;
        let warnings = prepared.warnings.clone();
        stage_session_event(
            ctx,
            SessionEvent::new(
                prepared.session_id.clone(),
                prepared.invocation_id.clone(),
                request.branch,
                EventAuthor::User,
                SessionEventKind::UserMessage,
                SessionEventPayload::Message {
                    message: talos_llm_provider::ChatMessage::user(input),
                },
            ),
        );
        let max_tool_rounds = self.kernel_config.max_tool_rounds.max(1);
        let is_plan_react = matches!(
            self.settings.planner_kind,
            crate::PlannerKind::PlanReAct
        );
        let mut state = GeneralTurnLoopState::new(&prepared);
        let mut plan_react_state = PlanReActLoopState::default();
        let mut last_tool_batch: Option<GeneralTurnToolBatch> = None;
        let mut final_text = String::new();
        let stop_reason = loop {
            if matches!(self.flow_profile, FlowProfile::ReactiveLoop) {
                let stage = if state.round == 0 {
                    KernelThinkingStage::FrameworkPlan
                } else {
                    KernelThinkingStage::FrameworkReflect
                };
                let text = if state.round == 0 {
                    "Planning the next step from the current user request and available tools."
                } else {
                    "Re-evaluating the plan with the latest tool observations before the next model step."
                };
                emit_framework_thinking(prepared.stream_tx.as_ref(), stage, state.round, text)
                    .await;
            }
            let mut step_request = build_general_turn_step_request(&prepared, &state);
            let mut step_model_request = build_flow_step_model_request(
                request.planning_instruction.clone(),
                request.native_thinking.clone(),
                request.native_thinking_support,
                request.branch,
                &prepared,
                &state,
                &self.settings,
            );
            events.extend(
                self.request_pipeline
                    .run(ctx, &mut step_model_request)
                    .await?,
            );

            // Execute replay tool invocations injected by the confirmation processor when
            // a user approval has been received for a previously blocked tool call.
            if state.round == 0 && !step_model_request.replay_tool_invocations.is_empty() {
                let replay_calls: Vec<talos_llm_provider::FunctionCall> = step_model_request
                    .replay_tool_invocations
                    .iter()
                    .map(|r| {
                        talos_llm_provider::FunctionCall::new(
                            r.call_id.clone(),
                            r.tool_name.clone(),
                            r.arguments.clone(),
                        )
                    })
                    .collect();
                let pre_approved_call_ids: std::collections::HashSet<String> = step_model_request
                    .replay_tool_invocations
                    .iter()
                    .map(|r| r.call_id.clone())
                    .collect();
                let replay_batch = execute_general_turn_tool_batch(
                    &prepared,
                    self.tool_executor.clone(),
                    self.tool_executor_port.clone(),
                    Arc::new(self.kernel_config.clone()),
                    &self.agent_name,
                    &replay_calls,
                    pre_approved_call_ids,
                    step_model_request
                        .replay_dynamic_tool_results
                        .iter()
                        .map(|(call_id, result)| {
                            (
                                call_id.clone(),
                                serde_json::json!({
                                    "output": result.output,
                                    "success": result.success,
                                }),
                            )
                        })
                        .collect(),
                    prepared.stream_tx.as_ref(),
                )
                .await?;
                // Inject synthetic assistant message with tool calls followed by results so the
                // LLM sees a coherent conversation on the next step.
                state
                    .messages
                    .push(talos_llm_provider::ChatMessage::assistant_with_tool_calls(
                        "",
                        replay_calls,
                    ));
                apply_general_turn_tool_batch(&mut state, &replay_batch, &self.kernel_config);
            }

            let plugin_manager = ctx.plugin_manager.clone();
            if matches!(
                plugin_manager
                    .before_model(ctx, &mut step_model_request)
                    .await?,
                CallbackControl::Stop
            ) {
                let stop_reason = if has_final_response(&events, branch) {
                    StopReason::EndTurn
                } else {
                    StopReason::BeforeModelCallbackStopped
                };
                return Ok(FlowExecution {
                    events,
                    summary: FlowRunSummary {
                        output_text: final_text,
                        output_items: state.output_items.clone(),
                        stop_reason,
                        usage: state.usage,
                        warnings,
                    },
                });
            }
            step_request.messages = step_model_request.messages;
            step_request.tool_definitions = step_model_request.tool_definitions.clone();
            step_request.parallel_tool_calls = step_model_request.parallel_tool_calls;
            step_request.native_thinking = step_model_request.native_thinking.clone();
            let mut step =
                execute_general_turn_step(step_request, self.llm_provider.clone()).await?;
            let mut step_response = LlmResponseEnvelope {
                output_text: step.text.clone(),
                output_items: step.output_items.clone(),
                branch: request.branch,
                actions: crate::EventActions::default(),
                allow_final_projection: true,
            };
            let plugin_manager = ctx.plugin_manager.clone();
            if matches!(
                plugin_manager.after_model(ctx, &mut step_response).await?,
                CallbackControl::Stop
            ) {
                let stop_reason = if has_final_response(&events, branch) {
                    StopReason::EndTurn
                } else {
                    StopReason::AfterModelCallbackStopped
                };
                return Ok(FlowExecution {
                    events,
                    summary: FlowRunSummary {
                        output_text: step_response.output_text,
                        output_items: step_response.output_items,
                        stop_reason,
                        usage: state.usage,
                        warnings,
                    },
                });
            }
            step.text = step_response.output_text.clone();
            step.output_items = step_response.output_items.clone();

            let mut projected_step_response = step_response;
            if matches!(step.disposition, GeneralTurnStepDisposition::FinalResponse) {
                projected_step_response.allow_final_projection = false;
            }
            let response_output = self
                .response_pipeline
                .run(ctx, &mut projected_step_response)
                .await?;
            events.extend(response_output.events);
            step.text = projected_step_response.output_text.clone();
            step.output_items = projected_step_response.output_items.clone();
            if !projected_step_response.actions.state_delta.is_empty()
                || projected_step_response.actions.branch_override.is_some()
            {
                let mut planning_event = SessionEvent::new(
                    prepared.session_id.clone(),
                    prepared.invocation_id.clone(),
                    projected_step_response.branch,
                    EventAuthor::Agent,
                    SessionEventKind::Planning,
                    SessionEventPayload::StateOnly,
                );
                planning_event.actions = projected_step_response.actions.clone();
                stage_session_event(ctx, planning_event);
                request.branch = projected_step_response.branch;
            }
            self.observe_tool_callbacks(ctx, &step.output_items).await?;
            apply_general_turn_step(&mut state, &step);

            if matches!(step.disposition, GeneralTurnStepDisposition::FinalResponse) {
                // PlanReAct mode: when require_final_answer_tag is enabled, only accept a
                // FinalResponse that carries a confirmed FINAL_ANSWER tag. A response without
                // the tag is treated as a no-progress round and may trigger a replan.
                if is_plan_react
                    && self.kernel_config.plan_react.require_final_answer_tag
                    && !ctx.execution_state.plan_react_has_final_answer
                {
                    plan_react_state.consecutive_no_progress_rounds += 1;
                    if should_replan(
                        &plan_react_state,
                        last_tool_batch.as_ref(),
                        &self.kernel_config.plan_react,
                    ) {
                        plan_react_state.replan_count += 1;
                        if state.round < max_tool_rounds {
                            continue;
                        }
                    }
                    final_text = "[plan-react stalled: no final answer]".to_string();
                    break StopReason::ToolRoundLimit;
                }
                final_text = step.text.clone();
                stage_session_event(
                    ctx,
                    SessionEvent::new(
                        prepared.session_id.clone(),
                        prepared.invocation_id.clone(),
                        request.branch,
                        EventAuthor::Agent,
                        SessionEventKind::AgentMessage,
                        SessionEventPayload::Message {
                            message: talos_llm_provider::ChatMessage::assistant(final_text.clone()),
                        },
                    ),
                );
                break StopReason::EndTurn;
            }

            // PlanReAct mode with tool calls: if the model also produced a FINAL_ANSWER tag
            // in the same round, drop it and warn — tool calls take priority.
            if is_plan_react && ctx.execution_state.plan_react_has_final_answer {
                tracing::warn!(
                    round = state.round,
                    "plan-react response contains both tool_calls and final_answer; \
                     final_answer dropped for this round"
                );
                ctx.set_plan_react_has_final_answer(false);
            }
            plan_react_state.consecutive_no_progress_rounds = 0;
            if matches!(self.flow_profile, FlowProfile::ReactiveLoop) {
                let tool_names = tool_names_for_thinking(&step.output_items);
                let tool_list = if tool_names.is_empty() {
                    "available tools".to_string()
                } else {
                    tool_names.join(", ")
                };
                emit_framework_thinking(
                    prepared.stream_tx.as_ref(),
                    KernelThinkingStage::FrameworkAct,
                    state.round,
                    format!("Executing the next action with tool calls: {}", tool_list),
                )
                .await;
            }
            stage_session_event(
                ctx,
                SessionEvent::new(
                    prepared.session_id.clone(),
                    prepared.invocation_id.clone(),
                    request.branch,
                    EventAuthor::Agent,
                    SessionEventKind::ToolCall,
                    SessionEventPayload::Message {
                        message: talos_llm_provider::ChatMessage::assistant_with_tool_calls(
                            step.text.clone(),
                            step.tool_calls.clone(),
                        ),
                    },
                ),
            );

            let tool_batch = execute_general_turn_tool_batch(
                &prepared,
                self.tool_executor.clone(),
                self.tool_executor_port.clone(),
                Arc::new(self.kernel_config.clone()),
                &self.agent_name,
                &step.tool_calls,
                std::collections::HashSet::new(),
                std::collections::HashMap::new(),
                prepared.stream_tx.as_ref(),
            )
            .await?;
            let mut tool_response = LlmResponseEnvelope {
                output_text: String::new(),
                output_items: tool_batch.output_items.clone(),
                branch: request.branch,
                actions: crate::EventActions::default(),
                allow_final_projection: true,
            };
            let response_output = self.response_pipeline.run(ctx, &mut tool_response).await?;
            events.extend(response_output.events);
            self.observe_tool_callbacks(ctx, &tool_response.output_items)
                .await?;
            for tool_message in &tool_batch.tool_messages {
                stage_session_event(
                    ctx,
                    SessionEvent::new(
                        prepared.session_id.clone(),
                        prepared.invocation_id.clone(),
                        request.branch,
                        EventAuthor::Tool,
                        SessionEventKind::ToolResult,
                        SessionEventPayload::Message {
                            message: tool_message.clone(),
                        },
                    ),
                );
            }
            for confirmation in &tool_batch.pending_confirmations {
                events.push(ExecutionEvent::new(
                    ctx,
                    EventAuthor::System,
                    request.branch,
                    ExecutionEventKind::System,
                    EventPayload::ConfirmationRequest {
                        payload: ConfirmationRequestPayload {
                            tool_name: confirmation.tool_name.clone(),
                            call_id: confirmation.call_id.clone(),
                            prompt: confirmation.prompt.clone(),
                            arguments: confirmation.arguments.clone(),
                        },
                    },
                    false,
                ));
                stage_session_event(
                    ctx,
                    SessionEvent::new(
                        prepared.session_id.clone(),
                        prepared.invocation_id.clone(),
                        request.branch,
                        EventAuthor::System,
                        SessionEventKind::SystemMessage,
                        SessionEventPayload::Confirmation {
                            payload: crate::session::SessionConfirmationPayload {
                                tool_name: confirmation.tool_name.clone(),
                                call_id: confirmation.call_id.clone(),
                                prompt: confirmation.prompt.clone(),
                                arguments: confirmation.arguments.clone(),
                            },
                        },
                    ),
                );
            }
            for dynamic_request in &tool_batch.pending_dynamic_requests {
                events.push(ExecutionEvent::new(
                    ctx,
                    EventAuthor::System,
                    request.branch,
                    ExecutionEventKind::System,
                    EventPayload::DynamicToolRequest {
                        payload: DynamicToolRequestPayload {
                            tool_name: dynamic_request.tool_name.clone(),
                            call_id: dynamic_request.call_id.clone(),
                            arguments: dynamic_request.arguments.clone(),
                        },
                    },
                    false,
                ));
                stage_session_event(
                    ctx,
                    SessionEvent::new(
                        prepared.session_id.clone(),
                        prepared.invocation_id.clone(),
                        request.branch,
                        EventAuthor::System,
                        SessionEventKind::SystemMessage,
                        SessionEventPayload::DynamicToolRequest {
                            payload: crate::session::SessionDynamicToolRequestPayload {
                                tool_name: dynamic_request.tool_name.clone(),
                                call_id: dynamic_request.call_id.clone(),
                                arguments: dynamic_request.arguments.clone(),
                            },
                        },
                    ),
                );
            }
            apply_general_turn_tool_batch(&mut state, &tool_batch, &self.kernel_config);
            last_tool_batch = Some(tool_batch.clone());
            if !tool_batch.pending_confirmations.is_empty()
                || !tool_batch.pending_dynamic_requests.is_empty()
            {
                if let Some(tx) = prepared.stream_tx.as_ref() {
                    for confirmation in &tool_batch.pending_confirmations {
                        let _ = tx
                            .send(KernelStreamEvent::new(
                                KernelStreamEventType::ConfirmationRequest,
                                KernelStreamEventData::ConfirmationRequest(
                                    ConfirmationRequestStreamData {
                                        call_id: confirmation.call_id.clone(),
                                        tool_name: confirmation.tool_name.clone(),
                                        prompt: confirmation.prompt.clone(),
                                        arguments: confirmation.arguments.clone(),
                                    },
                                ),
                            ))
                            .await;
                    }
                }
                final_text = if !tool_batch.pending_confirmations.is_empty() {
                    "Tool confirmation pending.".to_string()
                } else {
                    "Dynamic tool response pending.".to_string()
                };
                break StopReason::EndTurn;
            }
            if matches!(self.flow_profile, FlowProfile::ReactiveLoop) {
                let tool_names = tool_names_for_thinking(&tool_response.output_items);
                let observed = tool_names.len();
                let tool_list = if tool_names.is_empty() {
                    "tool outputs".to_string()
                } else {
                    tool_names.join(", ")
                };
                emit_framework_thinking(
                    prepared.stream_tx.as_ref(),
                    KernelThinkingStage::FrameworkObserve,
                    state.round,
                    format!(
                        "Observed {} result(s) from {} and feeding them back into the planner.",
                        observed.max(1),
                        tool_list
                    ),
                )
                .await;
            }

            if state.round >= max_tool_rounds {
                final_text = "[tool-call rounds exceeded]".to_string();
                break StopReason::ToolRoundLimit;
            }
        };

        let executed = finalize_general_turn_artifacts(state, final_text.clone(), stop_reason);
        emit_general_turn_finalization_stream(&prepared, &executed).await;
        let mut final_response = LlmResponseEnvelope {
            output_text: executed.output_text.clone(),
            output_items: executed
                .output_items
                .iter()
                .rev()
                .take(2)
                .cloned()
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect(),
            branch: request.branch,
            actions: crate::EventActions::default(),
            allow_final_projection: true,
        };
        let response_output = self.response_pipeline.run(ctx, &mut final_response).await?;
        events.extend(response_output.events);
        persist_general_turn_artifacts(
            &prepared,
            input,
            &executed,
            self.llm_provider.clone(),
            self.memory_store.clone(),
            &self.kernel_config,
        )
        .await;

        Ok(FlowExecution {
            events,
            summary: FlowRunSummary {
                output_text: final_response.output_text,
                output_items: executed.output_items,
                stop_reason,
                usage: executed.usage,
                warnings,
            },
        })
    }
}

pub struct AutoFlow {
    inner: SingleFlow,
}

impl AutoFlow {
    pub fn new(mut inner: SingleFlow) -> Self {
        inner.allow_transfer = true;
        inner.rebuild_pipelines();
        Self { inner }
    }

    pub fn inner(&self) -> &SingleFlow {
        &self.inner
    }
}

#[async_trait]
impl BaseLlmFlow for AutoFlow {
    fn settings(&self) -> &KernelExecutionSettings {
        self.inner.settings()
    }

    fn profile(&self) -> FlowProfile {
        self.inner.profile()
    }

    async fn run_events(
        &self,
        ctx: &mut InvocationContext,
        input: &str,
    ) -> TalosResult<FlowExecution> {
        self.inner.run_events(ctx, input).await
    }
}

impl From<crate::ExecutionPath> for ExecutionBranch {
    fn from(value: crate::ExecutionPath) -> Self {
        match value {
            crate::ExecutionPath::DirectAnswer => Self::DirectAnswer,
            crate::ExecutionPath::ReactiveLoop => Self::ReactiveLoop,
        }
    }
}

pub struct FlowFactory;

/// Determine whether the PlanReAct loop should allow another replan round.
///
/// `replan_count` is the number of replan *decisions* already made this turn (one per
/// no-progress or tool-error round that continued). Both the tool-error path and the
/// no-progress path share the same `max_replans` budget so callers cannot bypass the
/// limit by mixing the two triggers.
fn should_replan(
    state: &PlanReActLoopState,
    last_tool_batch: Option<&GeneralTurnToolBatch>,
    policy: &talos_config::PlanReActPolicy,
) -> bool {
    // Global replan budget exhausted — never allow another round.
    if state.replan_count >= policy.max_replans {
        return false;
    }
    // Allow replan when the last tool batch had errors (model-driven replan).
    if let Some(tool_batch) = last_tool_batch {
        if tool_batch.error_count > 0 {
            return true;
        }
    }
    // Allow replan for genuine no-progress rounds within the separate no-progress budget.
    state.consecutive_no_progress_rounds < policy.max_no_progress_rounds
}

fn select_flow_profile(
    settings: &KernelExecutionSettings,
    tool_registry: &dyn ToolDefinitionProvider,
) -> FlowProfile {
    match settings.planner_kind {
        crate::PlannerKind::None => FlowProfile::DirectAnswer,
        crate::PlannerKind::BuiltIn => {
            if tool_registry.get_tool_definitions().is_empty() {
                FlowProfile::DirectAnswer
            } else {
                FlowProfile::ModelThinking
            }
        }
        crate::PlannerKind::PlanReAct => FlowProfile::ReactiveLoop,
    }
}

impl FlowFactory {
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        agent_name: impl Into<String>,
        agent_description: impl Into<String>,
        llm_provider: Arc<dyn LlmProvider>,
        tool_definition_provider: Arc<dyn ToolDefinitionProvider>,
        tool_executor_port: Arc<dyn ToolExecutorPort>,
        memory_store: Arc<MemoryStore>,
        kernel_config: AgentKernelConfig,
        planner: Arc<dyn Planner>,
    ) -> Box<dyn BaseLlmFlow> {
        let settings = KernelExecutionSettings::try_from(&kernel_config).unwrap_or_default();
        let single = SingleFlow::new(
            agent_name,
            agent_description,
            llm_provider,
            tool_definition_provider,
            tool_executor_port,
            memory_store,
            kernel_config,
            planner,
        );

        match settings.flow_kind {
            FlowKind::Single => Box::new(single),
            FlowKind::Auto => Box::new(AutoFlow::new(single)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        BeforeModelCallback, BuiltInPlanner, ContextStateBags, InvocationExecutionState,
        InvocationLimits, PlanReActPlanner, ResponseDisposition, TurnPhase,
    };
    use async_trait::async_trait;
    use serde_json::{json, Value};
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    use talos_config::AgentKernelConfig;
    use talos_error::TalosResult;
    use talos_llm_provider::{
        ChatChunk, ChatRequest, ChatResponse, ChatStream, FunctionCall, LlmProvider, Role,
        ToolStream, ToolStreamEvent, Usage,
    };
    use talos_tools::{Tool, ToolContext, ToolOutput, ToolRegistry};
    use tokio::sync::mpsc;
    use uuid::Uuid;

    struct NoopLlmProvider;

    #[async_trait]
    impl LlmProvider for NoopLlmProvider {
        fn name(&self) -> &str {
            "noop"
        }

        async fn chat(&self, _request: ChatRequest) -> TalosResult<ChatResponse> {
            Ok(ChatResponse {
                content: String::new(),
                model: "noop".to_string(),
                usage: Usage::default(),
                finish_reason: Some("stop".to_string()),
                tool_calls: None,
            })
        }

        async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
            let (_tx, rx) = mpsc::channel::<TalosResult<ChatChunk>>(1);
            Ok(ChatStream::new(rx))
        }

        async fn stream_with_tools(&self, _request: ChatRequest) -> TalosResult<ToolStream> {
            let (_tx, rx) = mpsc::channel::<TalosResult<ToolStreamEvent>>(1);
            Ok(ToolStream::new(rx))
        }

        fn default_model(&self) -> &str {
            "noop"
        }
    }

    struct StopBeforeModelCallback;

    #[async_trait]
    impl BeforeModelCallback for StopBeforeModelCallback {
        async fn run(
            &self,
            _ctx: &mut InvocationContext,
            _request: &mut LlmRequestEnvelope,
        ) -> TalosResult<CallbackControl> {
            Ok(CallbackControl::Stop)
        }
    }

    #[derive(Default)]
    struct StepEventTestState {
        chat_round: AtomicUsize,
        tool_payload: Mutex<Option<String>>,
    }

    struct EchoTool;

    #[async_trait]
    impl Tool for EchoTool {
        fn name(&self) -> &str {
            "echo_test"
        }

        fn description(&self) -> &str {
            "Echo tool for flow step tests"
        }

        fn parameters_schema(&self) -> Option<Value> {
            Some(json!({
                "type": "object",
                "required": ["message"],
                "properties": {
                    "message": { "type": "string" }
                }
            }))
        }

        async fn execute(&self, input: Value, _context: ToolContext) -> TalosResult<ToolOutput> {
            let message = input
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("missing");
            Ok(ToolOutput {
                success: true,
                output: format!("echo:{message}"),
                error: None,
            })
        }
    }

    struct StepEventMockProvider {
        state: Arc<StepEventTestState>,
    }

    #[derive(Default)]
    struct ApprovalFlowState {
        chat_round: AtomicUsize,
    }

    struct ApprovalMockProvider {
        state: Arc<ApprovalFlowState>,
    }

    #[async_trait]
    impl LlmProvider for StepEventMockProvider {
        fn name(&self) -> &str {
            "step-event-mock"
        }

        fn default_model(&self) -> &str {
            "mock-model"
        }

        async fn chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
            self.chat_with_tools(request).await
        }

        async fn chat_with_tools(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
            let round = self.state.chat_round.fetch_add(1, Ordering::SeqCst);
            if round == 0 {
                return Ok(ChatResponse {
                    content: "Need tool data first.".to_string(),
                    model: request.model,
                    usage: Usage::default(),
                    finish_reason: Some("tool_calls".to_string()),
                    tool_calls: Some(vec![FunctionCall::new(
                        "call_1",
                        "echo_test",
                        json!({ "message": "ping" }),
                    )]),
                });
            }

            if let Some(message) = request.messages.iter().find(|msg| {
                msg.role == Role::Tool
                    && msg.name.as_deref() == Some("echo_test")
                    && msg.tool_call_id.as_deref() == Some("call_1")
            }) {
                self.state
                    .tool_payload
                    .lock()
                    .expect("tool payload mutex poisoned")
                    .replace(message.content.clone());
            }

            Ok(ChatResponse {
                content: "Final answer after tool.".to_string(),
                model: request.model,
                usage: Usage::default(),
                finish_reason: Some("stop".to_string()),
                tool_calls: None,
            })
        }

        async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
            let (_tx, rx) = mpsc::channel::<TalosResult<ChatChunk>>(1);
            Ok(ChatStream::new(rx))
        }

        async fn stream_with_tools(&self, _request: ChatRequest) -> TalosResult<ToolStream> {
            let (_tx, rx) = mpsc::channel::<TalosResult<ToolStreamEvent>>(1);
            Ok(ToolStream::new(rx))
        }
    }

    #[async_trait]
    impl LlmProvider for ApprovalMockProvider {
        fn name(&self) -> &str {
            "approval-mock"
        }

        fn default_model(&self) -> &str {
            "mock-model"
        }

        async fn chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
            self.chat_with_tools(request).await
        }

        async fn chat_with_tools(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
            let round = self.state.chat_round.fetch_add(1, Ordering::SeqCst);
            match round {
                0 => Ok(ChatResponse {
                    content: "Need approval first.".to_string(),
                    model: request.model,
                    usage: Usage::default(),
                    finish_reason: Some("tool_calls".to_string()),
                    tool_calls: Some(vec![FunctionCall::new(
                        "call_approval",
                        "exec_command",
                        json!({
                            "cmd": "echo hi",
                            "shell": "/bin/sh",
                            "login": false,
                            "sandbox_permissions": "require_escalated",
                            "justification": "Please confirm the risky command."
                        }),
                    )]),
                }),
                _ => panic!("approval flow should stop before another model round"),
            }
        }

        async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
            let (_tx, rx) = mpsc::channel::<TalosResult<ChatChunk>>(1);
            Ok(ChatStream::new(rx))
        }

        async fn stream_with_tools(&self, _request: ChatRequest) -> TalosResult<ToolStream> {
            let (_tx, rx) = mpsc::channel::<TalosResult<ToolStreamEvent>>(1);
            Ok(ToolStream::new(rx))
        }
    }

    fn build_ctx() -> InvocationContext {
        InvocationContext {
            session_id: "s1".to_string(),
            invocation_id: "i1".to_string(),
            agent_id: "a1".to_string(),
            workspace_id: "w1".to_string(),
            input: "hello".to_string(),
            model: "m1".to_string(),
            phase: TurnPhase::Execution,
            planning_instruction: None,
            transfer_to: None,
            execution_state: InvocationExecutionState::default(),
            session: crate::InvocationSession {
                session_id: "s1".to_string(),
                ..crate::InvocationSession::default()
            },
            state: HashMap::new(),
            state_bags: ContextStateBags::default(),
            limits: InvocationLimits::default(),
            events: Vec::new(),
            plugin_manager: crate::default_plugin_manager(),
            session_service: None,
            stream_tx: None,
        }
    }

    #[test]
    fn test_project_events_includes_final_response() {
        let ctx = build_ctx();
        let events = vec![
            ExecutionEvent::text(
                &ctx,
                EventAuthor::User,
                ExecutionBranch::ReactiveLoop,
                ExecutionEventKind::UserInput,
                "hello",
                false,
            ),
            ExecutionEvent::final_response(&ctx, ExecutionBranch::ReactiveLoop, "done"),
        ];
        assert!(events
            .iter()
            .all(|event| matches!(event.branch, ExecutionBranch::ReactiveLoop)));
        assert!(events
            .iter()
            .any(|event| matches!(event.kind, ExecutionEventKind::FinalResponse)));
    }

    #[test]
    fn test_build_flow_step_model_request_preserves_state_messages() {
        let prepared = crate::general_turn::PreparedGeneralTurn {
            session_id: "s1".to_string(),
            invocation_id: "i1".to_string(),
            agent_id: "a1".to_string(),
            selected_model: "m1".to_string(),
            prompt_prefix_messages: vec![talos_llm_provider::ChatMessage::system("prefix")],
            messages: vec![
                talos_llm_provider::ChatMessage::system("prefix"),
                talos_llm_provider::ChatMessage::user("hello"),
            ],
            tool_definitions: Vec::new(),
            prompt_diagnostics: talos_prompt::PromptDiagnostics {
                mode: talos_prompt::PromptMode::Full,
                included_sections: Vec::new(),
                dropped_sections: Vec::new(),
                truncated_sections: Vec::new(),
                bootstrap_files: Vec::new(),
                total_estimated_tokens: 0,
                static_estimated_tokens: 0,
                dynamic_estimated_tokens: 0,
            },
            warnings: Vec::new(),
            stream_tx: None,
        };
        let mut state = GeneralTurnLoopState::new(&prepared);
        state.messages.push(talos_llm_provider::ChatMessage::system(
            "Retrieved memories:\n- MEMORY.md: prefer tea",
        ));

        let request = build_flow_step_model_request(
            None,
            None,
            talos_llm_provider::NativeThinkingSupport::Unsupported,
            ExecutionBranch::ReactiveLoop,
            &prepared,
            &state,
            &KernelExecutionSettings::default(),
        );

        assert_eq!(request.prompt_prefix_messages.len(), 1);
        assert!(request
            .transient_messages
            .iter()
            .any(|message| message.role == Role::System
                && message.content.contains("Retrieved memories:")));
    }

    #[test]
    fn test_build_flow_step_model_request_preserves_native_thinking() {
        let prepared = crate::general_turn::PreparedGeneralTurn {
            session_id: "s1".to_string(),
            invocation_id: "i1".to_string(),
            agent_id: "a1".to_string(),
            selected_model: "m1".to_string(),
            prompt_prefix_messages: Vec::new(),
            messages: Vec::new(),
            tool_definitions: Vec::new(),
            prompt_diagnostics: talos_prompt::PromptDiagnostics {
                mode: talos_prompt::PromptMode::Full,
                included_sections: Vec::new(),
                dropped_sections: Vec::new(),
                truncated_sections: Vec::new(),
                bootstrap_files: Vec::new(),
                total_estimated_tokens: 0,
                static_estimated_tokens: 0,
                dynamic_estimated_tokens: 0,
            },
            warnings: Vec::new(),
            stream_tx: None,
        };
        let state = GeneralTurnLoopState::new(&prepared);
        let request = build_flow_step_model_request(
            None,
            Some(talos_llm_provider::NativeThinkingConfig {
                effort: talos_llm_provider::NativeThinkingEffort::High,
                budget_tokens: Some(1024),
            }),
            talos_llm_provider::NativeThinkingSupport::Supported,
            ExecutionBranch::ModelThinking,
            &prepared,
            &state,
            &KernelExecutionSettings::default(),
        );

        assert_eq!(
            request
                .native_thinking
                .as_ref()
                .and_then(|config| config.budget_tokens),
            Some(1024)
        );
    }

    #[test]
    fn test_select_flow_profile_follows_planner_and_tooling() {
        let built_in_settings = KernelExecutionSettings {
            planner_kind: crate::PlannerKind::BuiltIn,
            ..KernelExecutionSettings::default()
        };
        let empty_tools = ToolRegistry::new();
        assert_eq!(
            select_flow_profile(&built_in_settings, &empty_tools),
            FlowProfile::DirectAnswer
        );

        let tool_registry = Arc::new(ToolRegistry::new());
        tool_registry.register(Arc::new(EchoTool));
        assert_eq!(
            select_flow_profile(&built_in_settings, tool_registry.as_ref()),
            FlowProfile::ModelThinking
        );

        let plan_react_settings = KernelExecutionSettings {
            planner_kind: crate::PlannerKind::PlanReAct,
            ..KernelExecutionSettings::default()
        };
        assert_eq!(
            select_flow_profile(&plan_react_settings, tool_registry.as_ref()),
            FlowProfile::ReactiveLoop
        );
    }

    #[tokio::test]
    async fn test_response_pipeline_defaults_to_continue() {
        let pipeline = ResponseProcessorPipeline::new();
        let mut ctx = build_ctx();
        let mut response = LlmResponseEnvelope::default();
        let output = pipeline
            .run(&mut ctx, &mut response)
            .await
            .expect("pipeline should succeed");
        assert_eq!(output.disposition, ResponseDisposition::Continue);
        assert!(output.events.is_empty());
    }

    #[tokio::test]
    async fn test_single_flow_respects_before_model_callback_stop() {
        let callbacks =
            Arc::new(CallbackRegistry::new().with_before_model(Arc::new(StopBeforeModelCallback)));
        let mut ctx = build_ctx();
        let flow = SingleFlow::new(
            "agent-1",
            "test agent",
            Arc::new(NoopLlmProvider),
            Arc::new(ToolRegistry::new()),
            Arc::new(ToolRegistry::new()),
            Arc::new(MemoryStore::new(
                std::env::temp_dir().join("talos-flow-tests"),
            )),
            AgentKernelConfig::default(),
            Arc::new(BuiltInPlanner::new()),
        )
        .with_callbacks(callbacks);

        let result = flow
            .run_events(&mut ctx, "hello")
            .await
            .expect("flow should succeed");

        assert_eq!(
            result.summary.stop_reason,
            crate::StopReason::BeforeModelCallbackStopped
        );
        assert!(result.summary.output_text.is_empty());
        assert!(result.summary.output_items.is_empty());
    }

    #[tokio::test]
    async fn test_single_flow_emits_step_level_events_for_tool_loop() {
        let state = Arc::new(StepEventTestState::default());
        let tool_registry = Arc::new(ToolRegistry::new());
        tool_registry.register(Arc::new(EchoTool));
        let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
        let mut ctx = build_ctx();
        let flow = SingleFlow::new(
            "agent-1",
            "test agent",
            Arc::new(StepEventMockProvider {
                state: state.clone(),
            }),
            tool_registry.clone(),
            tool_registry,
            Arc::new(MemoryStore::new(workspace_dir)),
            AgentKernelConfig::default(),
            Arc::new(BuiltInPlanner::new()),
        );

        let result = flow
            .run_events(&mut ctx, "hello")
            .await
            .expect("flow should succeed");

        let kinds = result
            .events
            .iter()
            .map(|event| event.kind.clone())
            .collect::<Vec<_>>();
        let tool_call_index = kinds
            .iter()
            .position(|kind| matches!(kind, ExecutionEventKind::ToolCall))
            .expect("tool call event should exist");
        let tool_result_index = kinds
            .iter()
            .position(|kind| matches!(kind, ExecutionEventKind::ToolResult))
            .expect("tool result event should exist");
        let final_response_index = kinds
            .iter()
            .position(|kind| matches!(kind, ExecutionEventKind::FinalResponse))
            .expect("final response event should exist");
        assert_eq!(kinds.first(), Some(&ExecutionEventKind::UserInput));
        assert!(tool_call_index < tool_result_index);
        assert!(tool_result_index < final_response_index);
        assert!(result
            .events
            .iter()
            .any(|event| matches!(event.kind, ExecutionEventKind::Reasoning)));
        assert_eq!(result.summary.output_text, "Final answer after tool.");
        assert_eq!(state.chat_round.load(Ordering::SeqCst), 2);
        let payload = state
            .tool_payload
            .lock()
            .expect("tool payload mutex poisoned")
            .clone()
            .unwrap_or_default();
        assert!(payload.contains("\"ok\":true"));
    }

    #[tokio::test]
    async fn test_single_flow_stages_confirmation_event_for_pending_tool_approval() {
        let state = Arc::new(ApprovalFlowState::default());
        let tool_registry = Arc::new(ToolRegistry::new());
        talos_tools::register_core_local_tools(tool_registry.as_ref());
        let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
        std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
        let mut ctx = build_ctx();
        let flow = SingleFlow::new(
            "agent-1",
            "test agent",
            Arc::new(ApprovalMockProvider {
                state: state.clone(),
            }),
            tool_registry.clone(),
            tool_registry,
            Arc::new(MemoryStore::new(workspace_dir)),
            AgentKernelConfig::default(),
            Arc::new(BuiltInPlanner::new()),
        );

        let result = flow
            .run_events(&mut ctx, "hello")
            .await
            .expect("flow should succeed");

        assert_eq!(result.summary.output_text, "Tool confirmation pending.");
        assert_eq!(state.chat_round.load(Ordering::SeqCst), 1);
        assert!(result.events.iter().any(|event| matches!(
            &event.payload,
            EventPayload::ConfirmationRequest { payload }
                if payload.tool_name == "exec_command"
                    && payload.call_id == "call_approval"
                    && payload.prompt == "tool approval is required: Please confirm the risky command."
        )));
        assert!(ctx.session.events.iter().any(|event| matches!(
            &event.payload,
            SessionEventPayload::Confirmation { payload }
                if payload.tool_name == "exec_command"
                    && payload.call_id == "call_approval"
                    && payload.prompt == "tool approval is required: Please confirm the risky command."
        )));
    }

    #[test]
    fn test_single_flow_current_branch_uses_profile() {
        let built_in_direct = SingleFlow::new(
            "agent-1",
            "test agent",
            Arc::new(NoopLlmProvider),
            Arc::new(ToolRegistry::new()),
            Arc::new(ToolRegistry::new()),
            Arc::new(MemoryStore::new(
                std::env::temp_dir().join(Uuid::new_v4().to_string()),
            )),
            AgentKernelConfig {
                planner_mode: talos_config::PlannerMode::BuiltIn,
                ..AgentKernelConfig::default()
            },
            Arc::new(BuiltInPlanner::new()),
        );
        assert_eq!(
            built_in_direct.current_branch(),
            ExecutionBranch::DirectAnswer
        );

        let tool_registry = Arc::new(ToolRegistry::new());
        tool_registry.register(Arc::new(EchoTool));
        let built_in_model = SingleFlow::new(
            "agent-1",
            "test agent",
            Arc::new(NoopLlmProvider),
            tool_registry.clone(),
            tool_registry.clone(),
            Arc::new(MemoryStore::new(
                std::env::temp_dir().join(Uuid::new_v4().to_string()),
            )),
            AgentKernelConfig {
                planner_mode: talos_config::PlannerMode::BuiltIn,
                ..AgentKernelConfig::default()
            },
            Arc::new(BuiltInPlanner::new()),
        );
        assert_eq!(
            built_in_model.current_branch(),
            ExecutionBranch::ModelThinking
        );

        let plan_react = SingleFlow::new(
            "agent-1",
            "test agent",
            Arc::new(NoopLlmProvider),
            tool_registry.clone(),
            tool_registry,
            Arc::new(MemoryStore::new(
                std::env::temp_dir().join(Uuid::new_v4().to_string()),
            )),
            AgentKernelConfig {
                planner_mode: talos_config::PlannerMode::PlanReAct,
                ..AgentKernelConfig::default()
            },
            Arc::new(PlanReActPlanner::new()),
        );
        assert_eq!(plan_react.current_branch(), ExecutionBranch::ReactiveLoop);
    }

    #[test]
    fn test_should_replan_false_when_replan_budget_exhausted_regardless_of_no_progress_budget() {
        // max_replans = 0 means no replan at all, even when no-progress budget remains.
        let policy = talos_config::PlanReActPolicy {
            max_replans: 0,
            max_no_progress_rounds: 5,
            require_final_answer_tag: true,
        };
        let state = PlanReActLoopState {
            replan_count: 0,
            consecutive_no_progress_rounds: 0,
        };
        assert!(!should_replan(&state, None, &policy));
    }

    #[test]
    fn test_should_replan_false_when_replan_count_reached_max() {
        let policy = talos_config::PlanReActPolicy {
            max_replans: 2,
            max_no_progress_rounds: 5,
            require_final_answer_tag: true,
        };
        let state = PlanReActLoopState {
            replan_count: 2,
            consecutive_no_progress_rounds: 1,
        };
        // replan_count == max_replans: no further replan even if no-progress budget remains
        assert!(!should_replan(&state, None, &policy));
    }

    #[test]
    fn test_should_replan_true_when_no_progress_budget_remains() {
        let policy = talos_config::PlanReActPolicy {
            max_replans: 3,
            max_no_progress_rounds: 2,
            require_final_answer_tag: true,
        };
        let state = PlanReActLoopState {
            replan_count: 1,
            consecutive_no_progress_rounds: 1,
        };
        assert!(should_replan(&state, None, &policy));
    }

    #[test]
    fn test_should_replan_true_when_tool_errors_within_budget() {
        // max_no_progress_rounds = 0, but tool errors still trigger replan via max_replans budget.
        let policy = talos_config::PlanReActPolicy {
            max_replans: 3,
            max_no_progress_rounds: 0,
            require_final_answer_tag: true,
        };
        let state = PlanReActLoopState {
            replan_count: 1,
            consecutive_no_progress_rounds: 0,
        };
        let tool_batch = GeneralTurnToolBatch {
            output_items: Vec::new(),
            tool_messages: Vec::new(),
            pending_confirmations: Vec::new(),
            pending_dynamic_requests: Vec::new(),
            retrieved_memories: Vec::new(),
            logs: Vec::new(),
            error_count: 3, // multiple errors in one batch count as one replan opportunity
        };
        assert!(should_replan(&state, Some(&tool_batch), &policy));
    }

    #[test]
    fn test_should_replan_false_when_tool_errors_but_replan_budget_exhausted() {
        let policy = talos_config::PlanReActPolicy {
            max_replans: 2,
            max_no_progress_rounds: 0,
            require_final_answer_tag: true,
        };
        let state = PlanReActLoopState {
            replan_count: 2,
            consecutive_no_progress_rounds: 0,
        };
        let tool_batch = GeneralTurnToolBatch {
            output_items: Vec::new(),
            tool_messages: Vec::new(),
            pending_confirmations: Vec::new(),
            pending_dynamic_requests: Vec::new(),
            retrieved_memories: Vec::new(),
            logs: Vec::new(),
            error_count: 5,
        };
        // Multiple errors in batch still can't replan — budget exhausted at decision level.
        assert!(!should_replan(&state, Some(&tool_batch), &policy));
    }
}
