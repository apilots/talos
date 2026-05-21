use super::*;

pub async fn execute_general_turn_loop(
    prepared: PreparedGeneralTurn,
    agent_name: &str,
    llm_provider: Arc<dyn LlmProvider>,
    tool_executor_port: Arc<dyn ToolExecutorPort>,
    memory_store: Arc<MemoryStore>,
    kernel_config: &AgentKernelConfig,
) -> TalosResult<ExecutedTurnArtifacts> {
    let max_tool_rounds = kernel_config.max_tool_rounds.max(1);
    let mut state = GeneralTurnLoopState::new(&prepared);
    let execution_settings =
        crate::KernelExecutionSettings::try_from(kernel_config).unwrap_or_default();
    let tool_executor = crate::ToolExecutorFactory::build(execution_settings.tool_execution_mode);
    let kernel_config = Arc::new(kernel_config.clone());
    let mut tool_loop_guard = ToolLoopGuardState::default();

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
                    pre_approved_call_ids: std::collections::HashSet::new(),
                    dynamic_tool_results: std::collections::HashMap::new(),
                },
                tool_registry: tool_executor_port.clone(),
            });
        }
        let step = execute_general_turn_step(step_request, llm_provider.clone()).await?;
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
                Some(build_execution_summary(
                    state.tool_call_count,
                    state.tool_error_count,
                )),
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
                Some(build_execution_summary(
                    state.tool_call_count,
                    state.tool_error_count,
                )),
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
            if kernel_config.finalization_required
                && !step.final_answer_streamed
                && !matches!(step.model_finish_reason, ModelFinishReason::Length)
            {
                let mut messages = prepared.messages.clone();
                messages.extend(state.messages.clone());
                let forced_step = run_forced_finalization_step(
                    messages,
                    prepared.selected_model.clone(),
                    llm_provider.clone(),
                    prepared.stream_tx.clone(),
                    None,
                    Some(build_execution_summary(
                        state.tool_call_count,
                        state.tool_error_count,
                    )),
                    ForcedFinalizationCause::LoopTerminal,
                )
                .await?;
                apply_general_turn_step(&mut state, &forced_step);
                break (
                    forced_step.text.clone(),
                    stop_reason_for_forced_finalization(ForcedFinalizationCause::LoopTerminal),
                );
            }
            let stop_reason = match step.model_finish_reason {
                ModelFinishReason::Length => StopReason::ModelOutputLimit,
                ModelFinishReason::Stop | ModelFinishReason::Other => StopReason::EndTurn,
                ModelFinishReason::ToolCalls => StopReason::EndTurn,
            };
            break (step_text, stop_reason);
        }

        let tool_batch = if prepared.stream_tx.is_some() {
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
                std::collections::HashSet::new(),
                std::collections::HashMap::new(),
                prepared.stream_tx.as_ref(),
            )
            .await?
        };
        apply_general_turn_tool_batch(&mut state, &tool_batch, kernel_config.as_ref());
        if should_inject_read_only_nudge(&tool_loop_guard) {
            state.messages.push(build_read_only_nudge_message());
        }

        if !tool_batch.pending_confirmations.is_empty()
            || !tool_batch.pending_dynamic_requests.is_empty()
        {
            break (
                if !tool_batch.pending_confirmations.is_empty() {
                    "Tool confirmation pending.".to_string()
                } else {
                    "Dynamic tool response pending.".to_string()
                },
                StopReason::EndTurn,
            );
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
                Some(build_execution_summary(
                    state.tool_call_count,
                    state.tool_error_count,
                )),
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
