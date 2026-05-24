use super::*;

pub async fn execute_general_turn_loop(
    prepared: PreparedGeneralTurn,
    agent_name: &str,
    llm_provider: Arc<dyn LlmProvider>,
    tool_executor_port: Arc<dyn ToolExecutorPort>,
    memory_store: Arc<MemoryStore>,
    kernel_config: &AgentKernelConfig,
) -> TalosResult<ExecutedTurnArtifacts> {
    execute_general_turn_loop_with_resume(
        prepared,
        agent_name,
        llm_provider,
        tool_executor_port,
        memory_store,
        kernel_config,
        None,
    )
    .await
}

pub async fn execute_general_turn_loop_with_resume(
    prepared: PreparedGeneralTurn,
    agent_name: &str,
    llm_provider: Arc<dyn LlmProvider>,
    tool_executor_port: Arc<dyn ToolExecutorPort>,
    memory_store: Arc<MemoryStore>,
    kernel_config: &AgentKernelConfig,
    resume: Option<GeneralTurnResume>,
) -> TalosResult<ExecutedTurnArtifacts> {
    execute_general_turn_loop_with_policy(
        prepared,
        agent_name,
        llm_provider,
        tool_executor_port,
        memory_store,
        kernel_config,
        resume,
        general_turn_policy_for_config(kernel_config),
    )
    .await
}

fn general_turn_policy_for_config(kernel_config: &AgentKernelConfig) -> Arc<dyn GeneralTurnPolicy> {
    match kernel_config.planner_mode {
        talos_config::PlannerMode::BuiltIn => Arc::new(BuiltInGeneralTurnPolicy),
        talos_config::PlannerMode::PlanReAct => Arc::new(PlanReActGeneralTurnPolicy::new(
            kernel_config.plan_react.require_final_answer_tag,
        )),
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn execute_general_turn_loop_with_policy(
    prepared: PreparedGeneralTurn,
    agent_name: &str,
    llm_provider: Arc<dyn LlmProvider>,
    tool_executor_port: Arc<dyn ToolExecutorPort>,
    memory_store: Arc<MemoryStore>,
    kernel_config: &AgentKernelConfig,
    resume: Option<GeneralTurnResume>,
    policy: Arc<dyn GeneralTurnPolicy>,
) -> TalosResult<ExecutedTurnArtifacts> {
    let max_tool_rounds = kernel_config.max_tool_rounds.max(1);
    let mut state = GeneralTurnLoopState::new(&prepared);
    let execution_settings =
        crate::KernelExecutionSettings::try_from(kernel_config).unwrap_or_default();
    let tool_executor = crate::ToolExecutorFactory::build(execution_settings.tool_execution_mode);
    let kernel_config = Arc::new(kernel_config.clone());
    let mut tool_loop_guard = ToolLoopGuardState::default();

    if let Some(resume) = resume {
        apply_general_turn_resume(
            &prepared,
            tool_executor.clone(),
            tool_executor_port.clone(),
            memory_store.clone(),
            kernel_config.clone(),
            agent_name,
            &mut state,
            resume,
            policy.clone(),
        )
        .await?;
    }

    let (final_text, stop_reason) = loop {
        let mut step_request = build_general_turn_step_request(
            &prepared,
            &state,
            execution_settings.tool_execution_mode == crate::ToolExecutionMode::Parallel,
        );
        if prepared.stream_tx.is_some() {
            step_request.streaming_tool_execution = Some(StreamingToolExecutionConfig {
                execution_mode: execution_settings.tool_execution_mode,
                execution_context: ToolExecutionContext {
                    session_id: prepared.session_id.clone(),
                    user_id: agent_name.to_string(),
                    project_root: prepared.project_root.clone(),
                    kernel_config: kernel_config.clone(),
                    memory_store: memory_store.clone(),
                    dynamic_tool_results: std::collections::HashMap::new(),
                },
                tool_registry: tool_executor_port.clone(),
            });
        }
        policy.before_step_request(&mut step_request)?;
        step_request.defer_no_hint_final_projection = matches!(
            policy.accepts_final_response(""),
            GeneralTurnPolicyFinalAcceptance::Reject
        );
        let mut step = execute_general_turn_step(step_request, llm_provider.clone()).await?;
        if let Some(policy_stop) = policy.after_step_result(&mut step)? {
            apply_general_turn_step(&mut state, &step);
            break (policy_stop.output_text, policy_stop.stop_reason);
        }
        let step_text = step.text.clone();
        let step_disposition = step.disposition;
        if let Some(cause) = should_force_finalization_for_step(&mut tool_loop_guard, &step) {
            let mut messages = prepared.messages.clone();
            messages.extend(state.messages.clone());
            let forced_step = run_forced_finalization_step(
                messages,
                prepared.selected_model.clone(),
                llm_provider.clone(),
                prepared.stream_tx.clone(),
                None,
                None,
                cause.clone(),
            )
            .await?;
            apply_general_turn_step(&mut state, &forced_step);
            break (
                forced_step.text.clone(),
                stop_reason_for_forced_finalization(cause),
            );
        }
        apply_general_turn_step(&mut state, &step);

        if should_force_finalization_for_empty_response(state.tool_call_count, &step) {
            let mut messages = prepared.messages.clone();
            messages.extend(state.messages.clone());
            let forced_step = run_forced_finalization_step(
                messages,
                prepared.selected_model.clone(),
                llm_provider.clone(),
                prepared.stream_tx.clone(),
                None,
                None,
                ForcedFinalizationCause::EmptyFinalResponseAfterTools,
            )
            .await?;
            apply_general_turn_step(&mut state, &forced_step);
            break (
                forced_step.text.clone(),
                stop_reason_for_forced_finalization(
                    ForcedFinalizationCause::EmptyFinalResponseAfterTools,
                ),
            );
        }

        if matches!(step_disposition, GeneralTurnStepDisposition::FinalResponse) {
            state
                .messages
                .push(ChatMessage::assistant(step_text.clone()));
            let stop_reason = match step.model_finish_reason {
                ModelFinishReason::Length => StopReason::ModelOutputLimit,
                ModelFinishReason::Stop | ModelFinishReason::Other => StopReason::EndTurn,
                ModelFinishReason::ToolCalls => StopReason::EndTurn,
            };
            break (step_text, stop_reason);
        }

        if step.tool_calls.is_empty() {
            state.round = state.round.saturating_add(1);
            state.messages.push(ChatMessage::system(
                "The previous PlanReAct response did not include a FINAL_ANSWER tag. Continue with a valid tool action or provide the final answer under /*FINAL_ANSWER*/.",
            ));
            if state.round >= max_tool_rounds {
                let mut messages = prepared.messages.clone();
                messages.extend(state.messages.clone());
                let forced_step = run_forced_finalization_step(
                    messages,
                    prepared.selected_model.clone(),
                    llm_provider.clone(),
                    prepared.stream_tx.clone(),
                    None,
                    None,
                    ForcedFinalizationCause::ToolRoundBudget,
                )
                .await?;
                apply_general_turn_step(&mut state, &forced_step);
                break (
                    forced_step.text.clone(),
                    stop_reason_for_forced_finalization(ForcedFinalizationCause::ToolRoundBudget),
                );
            }
            continue;
        }

        let mut tool_batch = if prepared.stream_tx.is_some() {
            super::tool_batch::collect_general_turn_tool_batch(step.streamed_tool_outputs.clone())
        } else {
            execute_general_turn_tool_batch(
                &prepared,
                tool_executor.clone(),
                tool_executor_port.clone(),
                memory_store.clone(),
                kernel_config.clone(),
                agent_name,
                &step.tool_calls,
                std::collections::HashMap::new(),
                prepared.stream_tx.as_ref(),
            )
            .await?
        };
        let policy_stop = policy.after_tool_batch(&mut tool_batch)?;
        emit_pending_user_action_stream_events(&prepared, &tool_batch).await;
        apply_general_turn_tool_batch(&mut state, &tool_batch, kernel_config.as_ref());
        if let Some(message) = build_tool_failure_recovery_nudge_message(&tool_batch) {
            state.messages.push(message);
        }
        if should_inject_read_only_nudge(&tool_loop_guard) {
            state.messages.push(build_read_only_nudge_message());
        }

        if let Some(policy_stop) = policy_stop {
            break (policy_stop.output_text, policy_stop.stop_reason);
        }

        if !tool_batch.pending_dynamic_requests.is_empty() {
            break (String::new(), StopReason::WaitingDynamicInput);
        }

        if state.round >= max_tool_rounds {
            let mut messages = prepared.messages.clone();
            messages.extend(state.messages.clone());
            let forced_step = run_forced_finalization_step(
                messages,
                prepared.selected_model.clone(),
                llm_provider.clone(),
                prepared.stream_tx.clone(),
                None,
                None,
                ForcedFinalizationCause::ToolRoundBudget,
            )
            .await?;
            apply_general_turn_step(&mut state, &forced_step);
            break (
                forced_step.text.clone(),
                stop_reason_for_forced_finalization(ForcedFinalizationCause::ToolRoundBudget),
            );
        }
    };

    let executed = finalize_general_turn_artifacts(state, final_text, stop_reason);
    emit_general_turn_finalization_stream(&prepared, &executed).await;
    Ok(executed)
}

async fn apply_general_turn_resume(
    prepared: &PreparedGeneralTurn,
    tool_executor: Arc<dyn ToolExecutor>,
    tool_executor_port: Arc<dyn ToolExecutorPort>,
    memory_store: Arc<MemoryStore>,
    kernel_config: Arc<AgentKernelConfig>,
    agent_name: &str,
    state: &mut GeneralTurnLoopState,
    resume: GeneralTurnResume,
    policy: Arc<dyn GeneralTurnPolicy>,
) -> TalosResult<()> {
    let GeneralTurnResume::DynamicToolResult(mut resume) = resume;
    policy.before_dynamic_resume(&mut resume)?;
    let original_call = resume.original_call.unwrap_or_else(|| ResumeToolCall {
        item_id: None,
        call_id: resume.call_id.clone(),
        tool_name: resume.tool_name.clone(),
        arguments: Value::Null.into(),
    });
    let mut replay_dynamic_tool_results = resume.replay_dynamic_tool_results;
    replay_dynamic_tool_results.insert(
        resume.call_id.clone(),
        crate::execution::ReplayDynamicToolResult {
            output: resume.output,
            success: resume.success,
        },
    );
    let tool_calls = vec![FunctionCall::new(
        original_call.call_id,
        original_call.tool_name,
        Value::from(original_call.arguments),
    )];
    state.messages.push(ChatMessage::assistant_with_tool_calls(
        "",
        tool_calls.clone(),
    ));
    let replay_batch = execute_general_turn_tool_batch(
        prepared,
        tool_executor,
        tool_executor_port,
        memory_store,
        kernel_config.clone(),
        agent_name,
        &tool_calls,
        replay_dynamic_tool_results,
        prepared.stream_tx.as_ref(),
    )
    .await?;
    emit_pending_user_action_stream_events(prepared, &replay_batch).await;
    apply_general_turn_tool_batch(state, &replay_batch, kernel_config.as_ref());
    Ok(())
}

async fn emit_pending_user_action_stream_events(
    prepared: &PreparedGeneralTurn,
    tool_batch: &GeneralTurnToolBatch,
) {
    let Some(tx) = prepared.stream_tx.as_ref() else {
        return;
    };
    for dynamic_request in &tool_batch.pending_dynamic_requests {
        let _ = tx
            .send(KernelStreamEvent::new(
                KernelStreamEventType::DynamicToolRequest,
                KernelStreamEventData::DynamicToolRequest(DynamicToolRequestStreamData {
                    call_id: dynamic_request.call_id.clone(),
                    tool_name: dynamic_request.tool_name.clone(),
                    arguments: dynamic_request.arguments.clone(),
                }),
            ))
            .await;
    }
}
