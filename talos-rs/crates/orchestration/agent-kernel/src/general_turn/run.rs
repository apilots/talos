use super::prepare::{
    blocked_compaction_error, build_blocked_compaction_state,
    prompt_budget_still_exhausted_from_diagnostics,
};
use super::*;
use crate::ExecutionBranch;

#[allow(clippy::too_many_arguments)]
pub async fn run_general_turn(
    ctx: &mut InvocationContext,
    agent_name: &str,
    agent_description: &str,
    input: &str,
    llm_provider: Arc<dyn LlmProvider>,
    tool_definition_provider: Arc<dyn ToolDefinitionProvider>,
    tool_executor_port: Arc<dyn ToolExecutorPort>,
    memory_writer: Arc<dyn MemoryWriter>,
    kernel_config: &AgentKernelConfig,
    skill_options: GeneralTurnSkillOptions,
) -> TalosResult<GeneralTurnResult> {
    run_general_turn_with_resume(
        ctx,
        agent_name,
        agent_description,
        input,
        llm_provider,
        tool_definition_provider,
        tool_executor_port,
        memory_writer,
        kernel_config,
        None,
        Some(skill_options),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn run_general_turn_with_resume(
    ctx: &mut InvocationContext,
    agent_name: &str,
    agent_description: &str,
    input: &str,
    llm_provider: Arc<dyn LlmProvider>,
    tool_definition_provider: Arc<dyn ToolDefinitionProvider>,
    tool_executor_port: Arc<dyn ToolExecutorPort>,
    memory_writer: Arc<dyn MemoryWriter>,
    kernel_config: &AgentKernelConfig,
    resume: Option<GeneralTurnResume>,
    skill_options: Option<GeneralTurnSkillOptions>,
) -> TalosResult<GeneralTurnResult> {
    let prepared = prepare_general_turn(
        ctx,
        agent_name,
        agent_description,
        input,
        llm_provider.clone(),
        tool_definition_provider,
        memory_writer,
        kernel_config,
        skill_options.clone(),
    )
    .await?;
    let mut prompt_diagnostics = prepared.prompt_diagnostics.clone();
    let warnings = prepared.warnings.clone();
    if let Some(prompt_warning_state) = prepared.prompt_warning_state.as_ref() {
        if let Ok(value) = serde_json::to_value(prompt_warning_state) {
            stage_session_state_update(
                ctx,
                &prepared.session_id,
                &prepared.invocation_id,
                ExecutionBranch::ModelThinking,
                PROMPT_WARNING_STATE_KEY,
                value,
            )
            .await?;
        }
    }
    if prompt_budget_still_exhausted_from_diagnostics(&prepared.prompt_diagnostics) {
        if let Ok(value) = serde_json::to_value(build_blocked_compaction_state()) {
            stage_session_state_update(
                ctx,
                &prepared.session_id,
                &prepared.invocation_id,
                ExecutionBranch::ModelThinking,
                SESSION_COMPACTION_STATE_KEY,
                value,
            )
            .await?;
        }
        return Err(blocked_compaction_error(&prepared.session_id));
    }
    let executed = execute_general_turn_loop_with_resume(
        prepared.clone(),
        llm_provider.clone(),
        tool_executor_port,
        kernel_config,
        resume,
    )
    .await?;
    prompt_diagnostics.window_usage.cache_read_input_tokens =
        executed.usage.cache_read_input_tokens;
    prompt_diagnostics.window_usage.cache_creation_input_tokens =
        executed.usage.cache_creation_input_tokens;
    if let Ok(value) = serde_json::to_value(&prompt_diagnostics) {
        ctx.set_request_state(LAST_PROMPT_DIAGNOSTICS_STATE_KEY, value);
    }
    tracing::debug!(
        session_id = %prepared.session_id,
        agent_id = %prepared.agent_id,
        prompt_context_window_tokens = prompt_diagnostics.window_usage.context_window,
        prompt_effective_budget_tokens = prompt_diagnostics.window_usage.effective_prompt_budget,
        prompt_static_prefix_tokens = prompt_diagnostics.window_usage.static_prefix_tokens,
        prompt_dynamic_suffix_tokens = prompt_diagnostics.window_usage.dynamic_suffix_tokens,
        prompt_total_tokens = prompt_diagnostics.window_usage.total_tokens,
        cache_read_input_tokens = prompt_diagnostics.window_usage.cache_read_input_tokens,
        cache_creation_input_tokens = prompt_diagnostics.window_usage.cache_creation_input_tokens,
        session_history_compaction = ?prompt_diagnostics.session_history_compaction,
        "prompt governance metrics"
    );
    if let Ok(value) = serde_json::to_value(next_session_compaction_state(&prompt_diagnostics)) {
        stage_session_state_update(
            ctx,
            &prepared.session_id,
            &prepared.invocation_id,
            ExecutionBranch::ModelThinking,
            SESSION_COMPACTION_STATE_KEY,
            value,
        )
        .await?;
    }
    if let Some(session_continuity) = build_session_continuity(input, &executed) {
        if let Ok(value) = serde_json::to_value(session_continuity) {
            stage_session_state_update(
                ctx,
                &prepared.session_id,
                &prepared.invocation_id,
                ExecutionBranch::ModelThinking,
                SESSION_CONTINUITY_STATE_KEY,
                value,
            )
            .await?;
        }
    }
    let persisted_input = UserInput {
        text: input.to_string(),
        file_ids: ctx.input.file_ids.clone(),
    };
    let artifact_seq = persist_general_turn_artifacts(
        &prepared,
        &persisted_input,
        &executed,
        ctx.session_artifact_store()?,
        ctx.transcript_service.clone(),
        kernel_config,
    )
    .await?;
    let outcome = general_turn_outcome_from_artifacts(&executed);

    Ok(GeneralTurnResult {
        output_text: executed.output_text.clone(),
        output_items: executed.output_items.clone(),
        stop_reason: executed.stop_reason,
        usage: executed.usage.clone(),
        metrics: executed.metrics.clone(),
        prompt_diagnostics,
        warnings,
        outcome,
        artifact_seq,
    })
}

pub(crate) fn general_turn_outcome_from_artifacts(
    executed: &ExecutedTurnArtifacts,
) -> GeneralTurnOutcome {
    if matches!(executed.stop_reason, StopReason::WaitingDynamicInput) {
        if let Some(pending) = executed.pending_dynamic_requests.first().cloned() {
            return GeneralTurnOutcome::Waiting {
                output_items: executed.output_items.clone(),
                pending_action: PendingGeneralTurnAction::DynamicToolResult(pending),
                stop_reason: executed.stop_reason,
                usage: executed.usage.clone(),
                metrics: executed.metrics.clone(),
            };
        }
    }

    GeneralTurnOutcome::Completed {
        output_text: executed.output_text.clone(),
        output_items: executed.output_items.clone(),
        stop_reason: executed.stop_reason,
        usage: executed.usage.clone(),
        metrics: executed.metrics.clone(),
    }
}
