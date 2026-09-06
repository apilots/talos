use super::prepare::{
    blocked_compaction_error, build_blocked_compaction_state,
    prompt_budget_still_exhausted_from_diagnostics,
};
use super::*;
use crate::ExecutionBranch;
use std::sync::Arc;
use talos_run_control::RunTaskGroup;

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub async fn run_general_turn(
    ctx: &mut InvocationContext,
    agent_name: &str,
    agent_description: &str,
    input: &str,
    llm_provider: Arc<dyn LlmProvider>,
    tool_definition_provider: Arc<dyn ToolDefinitionProvider>,
    tool_executor_port: Arc<dyn ToolExecutorPort>,
    dream_memory_store: Arc<dyn DreamMemoryFormationStore>,
    user_workspace_memory: talos_prompt::UserWorkspaceMemoryPromptContext,
    kernel_config: &AgentKernelConfig,
    skill_options: GeneralTurnSkillOptions,
    run_tasks: RunTaskGroup,
) -> TalosResult<GeneralTurnResult> {
    run_general_turn_with_resume(
        ctx,
        agent_name,
        agent_description,
        input,
        llm_provider,
        tool_definition_provider,
        tool_executor_port,
        dream_memory_store,
        user_workspace_memory,
        kernel_config,
        None,
        Some(skill_options),
        None,
        run_tasks,
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
    dream_memory_store: Arc<dyn DreamMemoryFormationStore>,
    user_workspace_memory: talos_prompt::UserWorkspaceMemoryPromptContext,
    kernel_config: &AgentKernelConfig,
    resume: Option<AgentKernelContinuation>,
    skill_options: Option<GeneralTurnSkillOptions>,
    message_rx: Option<mpsc::Receiver<crate::ActiveTurnInputDelivery>>,
    run_tasks: RunTaskGroup,
) -> TalosResult<GeneralTurnResult> {
    let mut prepared = prepare_general_turn(
        ctx,
        agent_name,
        agent_description,
        input,
        llm_provider.clone(),
        tool_definition_provider,
        dream_memory_store,
        user_workspace_memory,
        kernel_config,
        skill_options.clone(),
        run_tasks,
    )
    .await?;
    prepared.message_rx = message_rx.map(|receiver| Arc::new(std::sync::Mutex::new(receiver)));
    let mut prompt_diagnostics = prepared.prompt_diagnostics.clone();
    if let Some(prompt_warning_state) = prepared.prompt_warning_state.as_ref() {
        stage_session_state_update(
            ctx,
            &prepared.session_id,
            &prepared.invocation_id,
            ExecutionBranch::ModelThinking,
            crate::SessionStateUpdate::PromptWarning(prompt_warning_state.clone()),
        )
        .await?;
    }
    if prompt_budget_still_exhausted_from_diagnostics(&prepared.prompt_diagnostics) {
        stage_session_state_update(
            ctx,
            &prepared.session_id,
            &prepared.invocation_id,
            ExecutionBranch::ModelThinking,
            crate::SessionStateUpdate::Compaction(build_blocked_compaction_state()),
        )
        .await?;
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
    stage_session_state_update(
        ctx,
        &prepared.session_id,
        &prepared.invocation_id,
        ExecutionBranch::ModelThinking,
        crate::SessionStateUpdate::Compaction(next_session_compaction_state(&prompt_diagnostics)),
    )
    .await?;
    if let Some(session_continuity) = build_session_continuity(input, &executed) {
        stage_session_state_update(
            ctx,
            &prepared.session_id,
            &prepared.invocation_id,
            ExecutionBranch::ModelThinking,
            crate::SessionStateUpdate::Continuity(session_continuity),
        )
        .await?;
    }
    persist_general_turn_state(&prepared, &executed, ctx.transcript_service.clone()).await?;
    let outcome = general_turn_outcome_from_artifacts(&executed);

    Ok(GeneralTurnResult {
        last_model_usage: executed.last_model_usage.clone(),
        prompt_diagnostics,
        outcome,
    })
}

pub(crate) fn general_turn_outcome_from_artifacts(
    executed: &ExecutedTurnArtifacts,
) -> GeneralTurnOutcome {
    if matches!(executed.stop_reason, StopReason::WaitingUserAction) {
        if let Some(pending) = executed.pending_user_action.clone() {
            return GeneralTurnOutcome::Waiting {
                output_items: executed.output_items.clone(),
                pending_action: Box::new(pending),
                usage: executed.usage.clone(),
                metrics: executed.metrics.clone(),
            };
        }
    }

    GeneralTurnOutcome::Completed {
        output_items: executed.output_items.clone(),
        stop_reason: executed.stop_reason,
        usage: executed.usage.clone(),
        metrics: executed.metrics.clone(),
    }
}
