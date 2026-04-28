use super::*;
use crate::{
    general_turn::{
        build_read_only_nudge_message, build_session_continuity, next_session_compaction_state,
        run_forced_finalization_step, should_force_finalization_for_empty_response,
        should_force_finalization_for_step, should_inject_read_only_nudge,
        stage_session_state_update, stop_reason_for_forced_finalization, ForcedFinalizationCause,
        SessionCompactionState, ToolLoopGuardState, LAST_PROMPT_DIAGNOSTICS_STATE_KEY,
        PROMPT_WARNING_STATE_KEY, SESSION_COMPACTION_STATE_KEY, SESSION_CONTINUITY_STATE_KEY,
    },
    StreamingToolExecutionConfig, ToolExecutionContext,
};

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
    pub(crate) allow_transfer: bool,
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

    pub(crate) fn rebuild_pipelines(&mut self) {
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
            selected_model_info: Some(ctx.selected_model_info.clone()),
            planning_instruction: ctx.planning_instruction.clone(),
            contents_mode: ContentsMode::FullSession,
            branch,
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
            self.llm_provider.clone(),
            self.tool_definition_provider.clone(),
            self.memory_store.clone(),
            &self.kernel_config,
        )
        .await?;
        let warnings = prepared.warnings.clone();
        if let Some(prompt_warning_state) = prepared.prompt_warning_state.as_ref() {
            if let Ok(value) = serde_json::to_value(prompt_warning_state) {
                stage_session_state_update(
                    ctx,
                    &prepared.session_id,
                    &prepared.invocation_id,
                    request.branch,
                    PROMPT_WARNING_STATE_KEY,
                    value,
                );
            }
        }
        if prepared.prompt_diagnostics.window_usage.window_resolved
            && !prepared
                .prompt_diagnostics
                .window_usage
                .within_effective_budget
        {
            if let Ok(value) =
                serde_json::to_value(SessionCompactionState::blocked_prompt_budget_exhausted())
            {
                stage_session_state_update(
                    ctx,
                    &prepared.session_id,
                    &prepared.invocation_id,
                    request.branch,
                    SESSION_COMPACTION_STATE_KEY,
                    value,
                );
            }
            return Err(talos_error::TalosError::ValidationError(format!(
                "session `{}` is blocked because prompt budget remains exhausted after compaction; reset_session or create a new session to continue",
                prepared.session_id
            )));
        }
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
        let is_plan_react = matches!(self.settings.planner_kind, crate::PlannerKind::PlanReAct);
        let mut state = GeneralTurnLoopState::new(&prepared);
        let mut plan_react_state = PlanReActLoopState::default();
        let mut tool_loop_guard = ToolLoopGuardState::default();
        let mut last_tool_batch: Option<GeneralTurnToolBatch> = None;
        let mut final_text = String::new();
        let kernel_config = Arc::new(self.kernel_config.clone());
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
            let mut step_request = build_general_turn_step_request(
                &prepared,
                &state,
                self.settings.tool_execution_mode == crate::ToolExecutionMode::Parallel,
            );
            if prepared.stream_tx.is_some() {
                step_request.streaming_tool_execution = Some(StreamingToolExecutionConfig {
                    execution_mode: self.settings.tool_execution_mode,
                    execution_context: ToolExecutionContext {
                        session_id: format!("agent-{}", prepared.agent_id),
                        user_id: self.agent_name.clone(),
                        project_root: prepared.project_root.clone(),
                        kernel_config: kernel_config.clone(),
                        memory_store: self.memory_store.clone(),
                        pre_approved_call_ids: std::collections::HashSet::new(),
                        dynamic_tool_results: std::collections::HashMap::new(),
                    },
                    tool_registry: self.tool_executor_port.clone(),
                });
            }
            let mut step_model_request = build_flow_step_model_request(
                request.selected_model_info.clone(),
                request.planning_instruction.clone(),
                request.native_thinking.clone(),
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
                    self.memory_store.clone(),
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
            let step_messages = step_model_request.messages.clone();
            let step_native_thinking = step_model_request.native_thinking.clone();
            step_request.messages = step_model_request.messages;
            step_request.tool_definitions = step_model_request.tool_definitions.clone();
            step_request.parallel_tool_calls = step_model_request.parallel_tool_calls;
            step_request.native_thinking = step_native_thinking.clone();
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
            if let Some(cause) = should_force_finalization_for_step(&mut tool_loop_guard, &step) {
                let forced_step = run_forced_finalization_step(
                    step_messages,
                    prepared.selected_model.clone(),
                    self.llm_provider.clone(),
                    step_native_thinking,
                    cause.clone(),
                )
                .await?;
                apply_general_turn_step(&mut state, &forced_step);
                final_text = forced_step.text.clone();
                break stop_reason_for_forced_finalization(cause);
            }
            self.observe_tool_callbacks(ctx, &step.output_items).await?;
            apply_general_turn_step(&mut state, &step);

            if should_force_finalization_for_empty_response(
                state.tool_call_count.saturating_sub(step.tool_calls.len()),
                &step,
            ) {
                let forced_step = run_forced_finalization_step(
                    step_messages.clone(),
                    prepared.selected_model.clone(),
                    self.llm_provider.clone(),
                    step_native_thinking.clone(),
                    ForcedFinalizationCause::EmptyFinalResponseAfterTools,
                )
                .await?;
                apply_general_turn_step(&mut state, &forced_step);
                final_text = forced_step.text.clone();
                break stop_reason_for_forced_finalization(
                    ForcedFinalizationCause::EmptyFinalResponseAfterTools,
                );
            }

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
            if is_plan_react {
                if is_read_only_tool_round(&step) {
                    plan_react_state.consecutive_read_only_rounds += 1;
                    if plan_react_state.consecutive_read_only_rounds >= 3 {
                        tracing::warn!(
                            round = state.round,
                            "plan-react terminated after repeated read-only investigation rounds"
                        );
                        let forced_step = run_forced_finalization_step(
                            step_messages.clone(),
                            prepared.selected_model.clone(),
                            self.llm_provider.clone(),
                            step_native_thinking.clone(),
                            ForcedFinalizationCause::RepeatedReadOnlyInvestigation,
                        )
                        .await?;
                        apply_general_turn_step(&mut state, &forced_step);
                        final_text = forced_step.text.clone();
                        break stop_reason_for_forced_finalization(
                            ForcedFinalizationCause::RepeatedReadOnlyInvestigation,
                        );
                    }
                } else {
                    plan_react_state.consecutive_read_only_rounds = 0;
                }
            }
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

            let tool_batch = if prepared.stream_tx.is_some() {
                collect_general_turn_tool_batch(step.streamed_tool_outputs.clone())
            } else {
                execute_general_turn_tool_batch(
                    &prepared,
                    self.tool_executor.clone(),
                    self.tool_executor_port.clone(),
                    self.memory_store.clone(),
                    kernel_config.clone(),
                    &self.agent_name,
                    &step.tool_calls,
                    std::collections::HashSet::new(),
                    std::collections::HashMap::new(),
                    prepared.stream_tx.as_ref(),
                )
                .await?
            };
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
            if should_inject_read_only_nudge(&tool_loop_guard) {
                state.messages.push(build_read_only_nudge_message());
            }
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
                let mut forced_request = build_flow_step_model_request(
                    request.selected_model_info.clone(),
                    request.planning_instruction.clone(),
                    request.native_thinking.clone(),
                    request.branch,
                    &prepared,
                    &state,
                    &self.settings,
                );
                events.extend(self.request_pipeline.run(ctx, &mut forced_request).await?);
                let forced_step = run_forced_finalization_step(
                    forced_request.messages.clone(),
                    prepared.selected_model.clone(),
                    self.llm_provider.clone(),
                    forced_request.native_thinking.clone(),
                    ForcedFinalizationCause::ToolRoundBudget,
                )
                .await?;
                apply_general_turn_step(&mut state, &forced_step);
                final_text = forced_step.text.clone();
                break stop_reason_for_forced_finalization(
                    ForcedFinalizationCause::ToolRoundBudget,
                );
            }
        };

        let executed = finalize_general_turn_artifacts(state, final_text.clone(), stop_reason);
        let mut prompt_diagnostics = prepared.prompt_diagnostics.clone();
        prompt_diagnostics.window_usage.cache_read_input_tokens =
            executed.usage.cache_read_input_tokens;
        prompt_diagnostics.window_usage.cache_creation_input_tokens =
            executed.usage.cache_creation_input_tokens;
        if let Ok(value) = serde_json::to_value(&prompt_diagnostics) {
            ctx.set_request_state(LAST_PROMPT_DIAGNOSTICS_STATE_KEY, value);
        }
        if let Ok(value) = serde_json::to_value(next_session_compaction_state(&prompt_diagnostics))
        {
            stage_session_state_update(
                ctx,
                &prepared.session_id,
                &prepared.invocation_id,
                request.branch,
                SESSION_COMPACTION_STATE_KEY,
                value,
            );
        }
        if let Some(session_continuity) = build_session_continuity(input, &executed) {
            if let Ok(value) = serde_json::to_value(session_continuity) {
                stage_session_state_update(
                    ctx,
                    &prepared.session_id,
                    &prepared.invocation_id,
                    request.branch,
                    SESSION_CONTINUITY_STATE_KEY,
                    value,
                );
            }
        }
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
