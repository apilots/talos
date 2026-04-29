use super::*;
use crate::{EventAuthor, ExecutionBranch, SessionEvent, SessionEventKind, SessionEventPayload};
use serde::de::DeserializeOwned;
use talos_prompt::{DynamicTrimPolicy, PromptSectionKind};
use talos_tools::{PermissionMode, ToolSelectionPhase, ToolSelectionRequest};

pub(crate) const SESSION_CONTINUITY_STATE_KEY: &str = "session_continuity";
pub(crate) const PROMPT_WARNING_STATE_KEY: &str = "prompt_warning_state";
pub(crate) const LAST_PROMPT_DIAGNOSTICS_STATE_KEY: &str = "last_prompt_diagnostics";
pub(crate) const SESSION_COMPACTION_STATE_KEY: &str = "session_compaction_state";

fn split_prompt_prefix_messages(messages: &[ChatMessage], input: &str) -> Vec<ChatMessage> {
    let Some(last_message) = messages.last() else {
        return Vec::new();
    };
    if last_message.role == talos_llm_provider::Role::User && last_message.content == input {
        return messages[..messages.len().saturating_sub(1)].to_vec();
    }
    messages.to_vec()
}

fn build_prompt_load_policy(kernel_config: &AgentKernelConfig) -> PromptLoadPolicy {
    PromptLoadPolicy {
        include_session_summary: true,
        include_recent_messages: true,
        include_recent_tool_history: true,
        include_recent_tool_outputs: true,
        include_retrieved_memories: true,
        session_recent_limit: kernel_config.compaction.recent_messages_keep,
        session_recent_max_chars: kernel_config.compaction.recent_messages_max_chars,
        recent_tool_history_limit: kernel_config.compaction.source_recent_tool_exchanges_limit,
        recent_tool_history_max_chars: kernel_config.compaction.tool_exchange_max_chars,
        recent_tool_outputs_limit: kernel_config.compaction.source_recent_tool_results_limit,
        recent_tool_outputs_max_chars: kernel_config.compaction.tool_output_max_chars,
        session_summary_recent_count: kernel_config.compaction.recent_summaries_keep,
        session_summary_max_chars: kernel_config.compaction.summary_max_chars,
        retrieved_memory_max_items: kernel_config.compaction.retrieved_memories_keep,
        retrieved_memory_max_chars: kernel_config.compaction.retrieved_memory_max_chars,
        unified_recent_limit: 10,
    }
}

fn build_initial_prompt_load_policy(
    kernel_config: &AgentKernelConfig,
    compaction_state: Option<&SessionCompactionState>,
) -> PromptLoadPolicy {
    if compaction_state.is_some_and(SessionCompactionState::is_recovery) {
        build_compacted_prompt_load_policy(kernel_config)
    } else {
        build_prompt_load_policy(kernel_config)
    }
}

fn build_tool_selection_request(
    session_id: &str,
    input: &str,
    phase: PromptPhase,
    kernel_config: &AgentKernelConfig,
) -> ToolSelectionRequest {
    ToolSelectionRequest {
        phase: match phase {
            PromptPhase::Planning => ToolSelectionPhase::Planning,
            PromptPhase::Execution => ToolSelectionPhase::Execution,
        },
        planner_mode: Some(kernel_config.planner_mode.as_str().to_string()),
        permission_mode: if matches!(phase, PromptPhase::Planning) {
            PermissionMode::Plan
        } else {
            PermissionMode::Default
        },
        user_input: Some(input.to_string()),
        session_id: Some(session_id.to_string()),
    }
}

fn build_compacted_prompt_load_policy(kernel_config: &AgentKernelConfig) -> PromptLoadPolicy {
    PromptLoadPolicy {
        include_session_summary: true,
        include_recent_messages: true,
        include_recent_tool_history: true,
        include_recent_tool_outputs: true,
        include_retrieved_memories: true,
        session_recent_limit: kernel_config
            .compaction
            .compacted_recent_messages_keep
            .max(1),
        session_recent_max_chars: kernel_config.compaction.compacted_recent_messages_max_chars,
        recent_tool_history_limit: kernel_config.compaction.tool_exchange_keep_last.max(1),
        recent_tool_history_max_chars: kernel_config.compaction.compacted_tool_exchange_max_chars,
        recent_tool_outputs_limit: kernel_config.compaction.tool_output_keep_last.max(1),
        recent_tool_outputs_max_chars: kernel_config.compaction.compacted_tool_output_max_chars,
        session_summary_recent_count: kernel_config.compaction.recent_summaries_keep.max(1),
        session_summary_max_chars: kernel_config.compaction.compacted_summary_max_chars,
        retrieved_memory_max_items: kernel_config.compaction.retrieved_memories_keep,
        retrieved_memory_max_chars: kernel_config
            .compaction
            .compacted_retrieved_memory_max_chars,
        unified_recent_limit: 10,
    }
}

fn resolve_compaction_input_budget_tokens(model_info: &talos_types::ModelInfo) -> Option<usize> {
    let context_window = model_info.context_window as usize;
    if context_window == 0 {
        return None;
    }

    Some((context_window / 2).clamp(256, 16_384).min(context_window))
}

#[allow(clippy::too_many_arguments)]
fn build_prompt_builder<'a>(
    agent_name: &str,
    agent_description: &str,
    input: &str,
    tool_definition_provider: &'a dyn ToolDefinitionProvider,
    memory_store: &'a MemoryStore,
    kernel_config: &AgentKernelConfig,
    session_id: &'a str,
    phase: PromptPhase,
    planning_instruction: Option<String>,
    selected_model_info: talos_types::ModelInfo,
    session_environment: Option<talos_types::talos::TalosSessionEnvironment>,
    project_root: Option<std::path::PathBuf>,
    session_continuity: Option<SessionContinuity>,
    load_policy: PromptLoadPolicy,
    dynamic_trim_policy: DynamicTrimPolicy,
) -> TalosResult<PromptBuilder<'a>> {
    Ok(PromptBuilder::new()
        .agent(agent_name.to_string(), agent_description.to_string())
        .phase(phase)
        .planner_mode(kernel_config.planner_mode.as_str())?
        .planning_instruction(planning_instruction)
        .config(PromptBuildConfig {
            load_policy,
            prompt: kernel_config.prompt.clone(),
            memory: kernel_config.memory.clone(),
            tools: kernel_config.tools.clone(),
            timezone: kernel_config.timezone.clone(),
        })
        .tools(tool_definition_provider)
        .memory_store(session_id, memory_store)
        .model_info(selected_model_info)
        .session_environment(session_environment)
        .project_root(project_root)
        .session_continuity(session_continuity)
        .dynamic_trim_policy(dynamic_trim_policy)
        .user_input(input.to_string()))
}

fn count_section_kind(sections: &[talos_prompt::PromptSection], kind: PromptSectionKind) -> usize {
    sections
        .iter()
        .filter(|section| section.kind == kind)
        .count()
}

fn history_sections_were_dropped(prompt_assembly: &talos_prompt::PromptAssembly) -> bool {
    prompt_assembly
        .diagnostics
        .dropped_sections
        .iter()
        .any(|kind| {
            matches!(
                kind,
                PromptSectionKind::RecentMessages | PromptSectionKind::SessionSummary
            )
        })
}

fn prompt_budget_usage_percent(window_usage: &talos_prompt::PromptWindowUsage) -> Option<u8> {
    if !window_usage.window_resolved {
        return None;
    }

    if window_usage.effective_prompt_budget == 0 {
        return Some(100);
    }

    let usage_percent =
        (window_usage.total_tokens.saturating_mul(100)) / window_usage.effective_prompt_budget;
    Some(usage_percent.min(100) as u8)
}

fn determine_history_compaction_trigger(
    prompt_assembly: &talos_prompt::PromptAssembly,
    kernel_config: &AgentKernelConfig,
) -> Option<SessionHistoryCompactionTrigger> {
    let history_reduction_needed = history_sections_were_dropped(prompt_assembly);
    let window_usage = &prompt_assembly.diagnostics.window_usage;
    if !window_usage.window_resolved {
        return None;
    }

    if !window_usage.within_effective_budget {
        Some(SessionHistoryCompactionTrigger::BudgetExceeded)
    } else if prompt_budget_usage_percent(window_usage)
        .is_some_and(|usage| usage >= kernel_config.compaction.hard_ratio_percent)
    {
        Some(SessionHistoryCompactionTrigger::HardThresholdExceeded)
    } else if prompt_budget_usage_percent(window_usage)
        .is_some_and(|usage| usage >= kernel_config.compaction.trigger_ratio_percent)
    {
        Some(SessionHistoryCompactionTrigger::ThresholdExceeded)
    } else if history_reduction_needed {
        Some(SessionHistoryCompactionTrigger::HistorySectionsDropped)
    } else {
        None
    }
}

fn is_recoverable_compaction_parse_error(error: &talos_error::TalosError) -> bool {
    let message = error.to_string();
    message.contains("parse memory summary json after")
        || message.contains("parse memory persistence decision json after")
}

#[allow(clippy::too_many_arguments)]
async fn rebuild_prompt_assembly(
    agent_name: &str,
    agent_description: &str,
    input: &str,
    tool_definition_provider: Arc<dyn ToolDefinitionProvider>,
    memory_store: Arc<MemoryStore>,
    kernel_config: &AgentKernelConfig,
    session_id: &str,
    phase: PromptPhase,
    planning_instruction: Option<String>,
    selected_model_info: talos_types::ModelInfo,
    session_environment: Option<talos_types::talos::TalosSessionEnvironment>,
    project_root: Option<std::path::PathBuf>,
    session_continuity: Option<SessionContinuity>,
    load_policy: PromptLoadPolicy,
    dynamic_trim_policy: DynamicTrimPolicy,
) -> TalosResult<talos_prompt::PromptAssembly> {
    build_prompt_builder(
        agent_name,
        agent_description,
        input,
        tool_definition_provider.as_ref(),
        memory_store.as_ref(),
        kernel_config,
        session_id,
        phase,
        planning_instruction,
        selected_model_info,
        session_environment,
        project_root,
        session_continuity,
        load_policy,
        dynamic_trim_policy,
    )?
    .assemble()
    .await
}

#[allow(clippy::too_many_arguments)]
async fn compact_session_history_if_needed(
    prompt_assembly: &talos_prompt::PromptAssembly,
    agent_name: &str,
    agent_description: &str,
    input: &str,
    llm_provider: Arc<dyn LlmProvider>,
    tool_definition_provider: Arc<dyn ToolDefinitionProvider>,
    memory_store: Arc<MemoryStore>,
    kernel_config: &AgentKernelConfig,
    session_id: &str,
    selected_model: &str,
    selected_model_info: talos_types::ModelInfo,
    phase: PromptPhase,
    planning_instruction: Option<String>,
    session_environment: Option<talos_types::talos::TalosSessionEnvironment>,
    project_root: Option<std::path::PathBuf>,
    session_continuity: Option<SessionContinuity>,
) -> TalosResult<
    Option<(
        Option<talos_prompt::PromptAssembly>,
        SessionHistoryCompaction,
    )>,
> {
    let Some(trigger) = determine_history_compaction_trigger(prompt_assembly, kernel_config) else {
        return Ok(None);
    };

    let pre_recent_messages = count_section_kind(
        &prompt_assembly.dynamic_suffix_sections,
        PromptSectionKind::RecentMessages,
    );
    let flush_workspace_memory = kernel_config.memory.workspace_memory_flush_enabled
        && kernel_config.memory.workspace_memory_flush_on_compaction;
    let compaction_usage = match flush_session_memory_with_report(
        session_id,
        selected_model,
        llm_provider,
        memory_store.as_ref(),
        kernel_config,
        MemoryFlushPolicy::new(false, true, flush_workspace_memory)
            .with_compaction_input_budget_tokens(resolve_compaction_input_budget_tokens(
                &selected_model_info,
            )),
    )
    .await
    {
        Ok(report) => report,
        Err(error) if is_recoverable_compaction_parse_error(&error) => {
            return Ok(Some((
                None,
                SessionHistoryCompaction {
                    trigger,
                    outcome: SessionHistoryCompactionOutcome::RejectedMissingSummary,
                    pre_compaction_total_tokens: prompt_assembly.diagnostics.total_estimated_tokens,
                    post_compaction_total_tokens: prompt_assembly
                        .diagnostics
                        .total_estimated_tokens,
                    recent_messages_before: pre_recent_messages,
                    recent_messages_after: pre_recent_messages,
                    summary_sections_after: 0,
                    trimmed_context_unit_count: 0,
                    trimmed_context_unit_kind: None,
                    fallback_section_drop_applied: false,
                    compaction_request_count: 0,
                    compaction_input_tokens: 0,
                    compaction_output_tokens: 0,
                    compaction_total_tokens: 0,
                    compaction_cache_creation_input_tokens: None,
                    compaction_cache_read_input_tokens: None,
                },
            )));
        }
        Err(error) => return Err(error),
    };

    let compacted_assembly = rebuild_prompt_assembly(
        agent_name,
        agent_description,
        input,
        tool_definition_provider,
        memory_store,
        kernel_config,
        session_id,
        phase,
        planning_instruction,
        selected_model_info,
        session_environment,
        project_root,
        session_continuity,
        build_compacted_prompt_load_policy(kernel_config),
        DynamicTrimPolicy::PreserveAll,
    )
    .await?;
    let summary_sections_after = count_section_kind(
        &compacted_assembly.dynamic_suffix_sections,
        PromptSectionKind::SessionSummary,
    );
    let recent_messages_after = count_section_kind(
        &compacted_assembly.dynamic_suffix_sections,
        PromptSectionKind::RecentMessages,
    );
    let post_compaction_total_tokens = compacted_assembly.diagnostics.total_estimated_tokens;
    let outcome = if summary_sections_after > 0 {
        SessionHistoryCompactionOutcome::Applied
    } else {
        SessionHistoryCompactionOutcome::RejectedMissingSummary
    };
    Ok(Some((
        (summary_sections_after > 0).then_some(compacted_assembly),
        SessionHistoryCompaction {
            trigger,
            outcome,
            pre_compaction_total_tokens: prompt_assembly.diagnostics.total_estimated_tokens,
            post_compaction_total_tokens,
            recent_messages_before: pre_recent_messages,
            recent_messages_after,
            summary_sections_after,
            trimmed_context_unit_count: compaction_usage.trimmed_context_unit_count,
            trimmed_context_unit_kind: compaction_usage.trimmed_context_unit_kind,
            fallback_section_drop_applied: false,
            compaction_request_count: compaction_usage.request_count,
            compaction_input_tokens: compaction_usage.input_tokens,
            compaction_output_tokens: compaction_usage.output_tokens,
            compaction_total_tokens: compaction_usage.total_tokens,
            compaction_cache_creation_input_tokens: compaction_usage.cache_creation_input_tokens,
            compaction_cache_read_input_tokens: compaction_usage.cache_read_input_tokens,
        },
    )))
}

async fn emit_prompt_warning_events(ctx: &mut InvocationContext, warnings: &[KernelWarning]) {
    for warning in warnings {
        let payload = PromptWarningEventData {
            warning: warning.clone(),
            agent_id: ctx.agent_id.clone(),
            session_id: ctx.session_id.clone(),
            phase: ctx.phase.clone(),
        };
        ctx.events.push(KernelEvent::new(
            KernelEventType::PromptWarning,
            KernelEventLabel::PromptWarning,
            KernelEventData::PromptWarning(payload.clone()),
        ));
        emit_stream_event(
            ctx.stream_tx.as_ref(),
            KernelStreamEventType::PromptWarning,
            KernelStreamEventData::PromptWarning(payload),
        )
        .await;
    }
}

fn load_session_state_value<T: DeserializeOwned>(ctx: &InvocationContext, key: &str) -> Option<T> {
    ctx.session
        .state
        .get(crate::StateNamespace::Session, key)
        .and_then(|value| serde_json::from_value(value.clone()).ok())
}

fn blocked_compaction_error(session_id: &str) -> talos_error::TalosError {
    talos_error::TalosError::ValidationError(format!(
        "session `{session_id}` is blocked because prompt budget remains exhausted after compaction; reset_session or create a new session to continue"
    ))
}

fn build_blocked_compaction_state() -> SessionCompactionState {
    SessionCompactionState::blocked_prompt_budget_exhausted()
}

fn build_compressing_compaction_state() -> SessionCompactionState {
    SessionCompactionState::compressing()
}

pub(crate) fn next_session_compaction_state(
    diagnostics: &PromptDiagnostics,
) -> SessionCompactionState {
    if diagnostics
        .session_history_compaction
        .as_ref()
        .is_some_and(|report| report.outcome == SessionHistoryCompactionOutcome::Applied)
    {
        SessionCompactionState::recovery()
    } else {
        SessionCompactionState::normal()
    }
}

fn prompt_budget_still_exhausted(prompt_assembly: &talos_prompt::PromptAssembly) -> bool {
    prompt_budget_still_exhausted_from_diagnostics(&prompt_assembly.diagnostics)
}

fn prompt_budget_still_exhausted_from_diagnostics(diagnostics: &PromptDiagnostics) -> bool {
    diagnostics.window_usage.window_resolved && !diagnostics.window_usage.within_effective_budget
}

fn prompt_warning_key(warning: &KernelWarning) -> Option<String> {
    match warning {
        KernelWarning::PromptBootstrapFileMissing(payload) => {
            Some(format!("missing:{:?}:{}", payload.file_kind, payload.path))
        }
        KernelWarning::PromptBootstrapFileTruncated(payload) => Some(format!(
            "truncated:{:?}:{}",
            payload.file_kind, payload.path
        )),
        KernelWarning::PromptBootstrapFileDropped(payload) => {
            Some(format!("dropped:{:?}:{}", payload.file_kind, payload.path))
        }
        KernelWarning::PromptDynamicSectionDropped(payload) => Some(format!(
            "dynamic_dropped:{}",
            serialize_section_kinds(&payload.sections)
        )),
    }
}

fn serialize_section_kinds(sections: &[PromptSectionKind]) -> String {
    sections
        .iter()
        .map(|section| format!("{section:?}"))
        .collect::<Vec<_>>()
        .join(",")
}

fn filter_prompt_warnings(
    ctx: &InvocationContext,
    warnings: Vec<KernelWarning>,
) -> (Vec<KernelWarning>, Option<PromptWarningState>) {
    let mut warning_state =
        load_session_state_value::<PromptWarningState>(ctx, PROMPT_WARNING_STATE_KEY)
            .unwrap_or_default();
    let mut changed = false;
    let mut filtered = Vec::new();

    for warning in warnings {
        if let Some(key) = prompt_warning_key(&warning) {
            if warning_state.seen_warning_keys.insert(key) {
                changed = true;
                filtered.push(warning);
            }
        } else {
            filtered.push(warning);
        }
    }

    (filtered, changed.then_some(warning_state))
}

pub(crate) fn stage_session_state_update(
    ctx: &mut InvocationContext,
    session_id: &str,
    invocation_id: &str,
    branch: ExecutionBranch,
    key: &str,
    value: serde_json::Value,
) {
    let mut event = SessionEvent::new(
        session_id.to_string(),
        invocation_id.to_string(),
        branch,
        EventAuthor::System,
        SessionEventKind::Planning,
        SessionEventPayload::StateOnly,
    );
    event.actions.state_delta.push(crate::StateDelta {
        namespace: crate::StateNamespace::Session,
        key: key.to_string(),
        value,
    });
    crate::flow::stage_session_event(ctx, event);
}

fn build_continuity_last_failure(executed: &ExecutedTurnArtifacts) -> Option<String> {
    executed
        .tool_result_log
        .iter()
        .rev()
        .find(|log| log.is_error)
        .map(|log| format!("{} returned an error", log.tool))
}

fn build_continuity_suggested_next_step(
    last_failure: Option<&str>,
    verified_paths: &[talos_prompt::VerifiedPathEvidence],
    tool_summaries: &[talos_prompt::ContinuityToolSummary],
) -> String {
    if last_failure.is_some() {
        "adjust the failing path, command, or tool arguments before retrying".to_string()
    } else if !verified_paths.is_empty() {
        "answer from the verified evidence without re-reading the same paths".to_string()
    } else if tool_summaries.is_empty() {
        "answer directly unless new evidence is required".to_string()
    } else {
        "continue with a narrower follow-up tool step".to_string()
    }
}

fn build_continuity_pending_tasks(
    last_failure: Option<&str>,
    verified_paths: &[talos_prompt::VerifiedPathEvidence],
    tool_summaries: &[talos_prompt::ContinuityToolSummary],
) -> Vec<String> {
    if last_failure.is_some() {
        vec!["fix the failing tool step and retry".to_string()]
    } else if !verified_paths.is_empty() {
        vec!["complete the answer from the verified evidence".to_string()]
    } else if tool_summaries.is_empty() {
        Vec::new()
    } else {
        vec!["continue with a narrower follow-up tool step".to_string()]
    }
}

pub(crate) fn build_session_continuity(
    input: &str,
    executed: &ExecutedTurnArtifacts,
) -> Option<SessionContinuity> {
    let recent_goal = input.trim().to_string();
    let failed_call_ids = executed
        .tool_result_log
        .iter()
        .filter(|log| log.is_error)
        .map(|log| log.call_id.clone())
        .collect::<std::collections::HashSet<_>>();

    let mut verified_paths: Vec<talos_prompt::VerifiedPathEvidence> = Vec::new();
    let mut tool_summaries: Vec<talos_prompt::ContinuityToolSummary> = Vec::new();
    let mut grep_patterns = Vec::new();

    for call in &executed.tool_call_log {
        let tool_name = call.name.clone();
        if failed_call_ids.contains(&call.call_id) {
            if !tool_summaries.iter().any(|summary| {
                summary.tool_name == tool_name
                    && summary.outcome == talos_prompt::ContinuityToolOutcome::Failure
            }) {
                tool_summaries.push(talos_prompt::ContinuityToolSummary {
                    tool_name: tool_name.clone(),
                    outcome: talos_prompt::ContinuityToolOutcome::Failure,
                });
            }
        } else if !tool_summaries.iter().any(|summary| {
            summary.tool_name == tool_name
                && summary.outcome == talos_prompt::ContinuityToolOutcome::Success
        }) {
            tool_summaries.push(talos_prompt::ContinuityToolSummary {
                tool_name: tool_name.clone(),
                outcome: talos_prompt::ContinuityToolOutcome::Success,
            });
        }

        for key in ["file_path", "dir_path", "path"] {
            if let Some(path) = call.arguments.get(key).and_then(|value| value.as_str()) {
                if !verified_paths
                    .iter()
                    .any(|evidence: &talos_prompt::VerifiedPathEvidence| evidence.path == path)
                {
                    verified_paths.push(talos_prompt::VerifiedPathEvidence {
                        path: path.to_string(),
                        source_tool: tool_name.clone(),
                    });
                }
                break;
            }
        }

        if matches!(call.name.as_str(), "Grep" | "grep") {
            if let Some(pattern) = call
                .arguments
                .get("pattern")
                .and_then(|value| value.as_str())
            {
                grep_patterns.push(pattern.to_string());
            }
        }
    }

    let mut recent_unhelpful_patterns = Vec::new();
    if tool_summaries
        .iter()
        .any(|summary| summary.outcome == talos_prompt::ContinuityToolOutcome::Failure)
    {
        recent_unhelpful_patterns
            .push("avoid retrying the same failing path or command".to_string());
    }
    if grep_patterns.len() > 1 {
        let unique_patterns = grep_patterns
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len();
        if unique_patterns < grep_patterns.len() {
            recent_unhelpful_patterns.push(
                "avoid repeating grep on the same pattern without narrowing scope".to_string(),
            );
        }
    }
    if executed.tool_call_log.len() > 1
        && verified_paths.len() < executed.tool_call_log.len().min(3)
    {
        recent_unhelpful_patterns
            .push("avoid re-reading the same path when no new evidence is needed".to_string());
    }

    let recent_answer_mode = if tool_summaries.iter().any(|summary| {
        summary.outcome == talos_prompt::ContinuityToolOutcome::Success
            && matches!(summary.tool_name.as_str(), "Grep" | "Read" | "Glob")
    }) {
        Some(ContinuityAnswerMode::CodeFirst)
    } else if executed.tool_call_log.is_empty() {
        Some(ContinuityAnswerMode::DirectAnswer)
    } else {
        Some(ContinuityAnswerMode::Mixed)
    };

    let last_failure = build_continuity_last_failure(executed);
    let suggested_next_step = Some(build_continuity_suggested_next_step(
        last_failure.as_deref(),
        &verified_paths,
        &tool_summaries,
    ));
    let pending_tasks =
        build_continuity_pending_tasks(last_failure.as_deref(), &verified_paths, &tool_summaries);

    if recent_goal.is_empty()
        && verified_paths.is_empty()
        && tool_summaries.is_empty()
        && pending_tasks.is_empty()
        && recent_unhelpful_patterns.is_empty()
        && recent_answer_mode.is_none()
        && last_failure.is_none()
    {
        None
    } else {
        Some(SessionContinuity {
            recent_goal,
            verified_paths: verified_paths.into_iter().take(4).collect(),
            tool_summaries: tool_summaries.into_iter().take(4).collect(),
            pending_tasks: pending_tasks.into_iter().take(2).collect(),
            recent_unhelpful_patterns: recent_unhelpful_patterns.into_iter().take(3).collect(),
            recent_answer_mode,
            last_failure,
            suggested_next_step,
        })
    }
}

pub async fn prepare_general_turn(
    ctx: &mut InvocationContext,
    agent_name: &str,
    agent_description: &str,
    input: &str,
    llm_provider: Arc<dyn LlmProvider>,
    tool_definition_provider: Arc<dyn ToolDefinitionProvider>,
    memory_store: Arc<MemoryStore>,
    kernel_config: &AgentKernelConfig,
) -> TalosResult<PreparedGeneralTurn> {
    let session_id = ctx.session_id.clone();
    let invocation_id = ctx.invocation_id.clone();
    let agent_id = ctx.agent_id.clone();
    let selected_model = ctx.selected_model_id().to_string();
    let stream_tx = ctx.stream_tx.clone();
    let session_compaction_state =
        load_session_state_value::<SessionCompactionState>(ctx, SESSION_COMPACTION_STATE_KEY);
    if session_compaction_state
        .as_ref()
        .is_some_and(SessionCompactionState::is_blocked)
    {
        return Err(blocked_compaction_error(&session_id));
    }
    let session_continuity =
        load_session_state_value::<SessionContinuity>(ctx, SESSION_CONTINUITY_STATE_KEY);

    let phase = match ctx.phase {
        crate::TurnPhase::Planning => PromptPhase::Planning,
        crate::TurnPhase::Execution => PromptPhase::Execution,
    };

    let initial_load_policy =
        build_initial_prompt_load_policy(kernel_config, session_compaction_state.as_ref());
    let mut prompt_assembly = rebuild_prompt_assembly(
        agent_name,
        agent_description,
        input,
        tool_definition_provider.clone(),
        memory_store.clone(),
        kernel_config,
        &session_id,
        phase,
        ctx.planning_instruction.clone(),
        ctx.selected_model_info.clone(),
        ctx.session_environment.clone(),
        ctx.project_root.clone(),
        session_continuity.clone(),
        initial_load_policy.clone(),
        DynamicTrimPolicy::PreserveAll,
    )
    .await?;
    let mut session_history_compaction = None;
    if determine_history_compaction_trigger(&prompt_assembly, kernel_config).is_some() {
        if let Ok(value) = serde_json::to_value(build_compressing_compaction_state()) {
            stage_session_state_update(
                ctx,
                &session_id,
                &invocation_id,
                ExecutionBranch::ModelThinking,
                SESSION_COMPACTION_STATE_KEY,
                value,
            );
        }
    }
    if let Some((compacted_assembly, compaction)) = compact_session_history_if_needed(
        &prompt_assembly,
        agent_name,
        agent_description,
        input,
        llm_provider,
        tool_definition_provider.clone(),
        memory_store.clone(),
        kernel_config,
        &session_id,
        &selected_model,
        ctx.selected_model_info.clone(),
        phase,
        ctx.planning_instruction.clone(),
        ctx.session_environment.clone(),
        ctx.project_root.clone(),
        session_continuity.clone(),
    )
    .await?
    {
        tracing::debug!(
            session_id = %session_id,
            agent_id = %agent_id,
            outcome = ?compaction.outcome,
            pre_tokens = compaction.pre_compaction_total_tokens,
            post_tokens = compaction.post_compaction_total_tokens,
            recent_messages_before = compaction.recent_messages_before,
            recent_messages_after = compaction.recent_messages_after,
            summary_sections_after = compaction.summary_sections_after,
            trimmed_context_unit_count = compaction.trimmed_context_unit_count,
            "session history compaction attempted prompt rebuild"
        );
        if let Some(compacted_assembly) = compacted_assembly {
            prompt_assembly = compacted_assembly;
        }
        session_history_compaction = Some(compaction);
    }

    if prompt_budget_still_exhausted(&prompt_assembly) {
        let fallback_load_policy = if session_history_compaction
            .as_ref()
            .map(|report| report.outcome == SessionHistoryCompactionOutcome::Applied)
            .unwrap_or(false)
        {
            build_compacted_prompt_load_policy(kernel_config)
        } else {
            initial_load_policy
        };
        prompt_assembly = rebuild_prompt_assembly(
            agent_name,
            agent_description,
            input,
            tool_definition_provider.clone(),
            memory_store.clone(),
            kernel_config,
            &session_id,
            phase,
            ctx.planning_instruction.clone(),
            ctx.selected_model_info.clone(),
            ctx.session_environment.clone(),
            ctx.project_root.clone(),
            session_continuity,
            fallback_load_policy,
            DynamicTrimPolicy::TrimToBudget,
        )
        .await?;
    }

    if let Some(mut compaction) = session_history_compaction {
        compaction.fallback_section_drop_applied =
            !prompt_assembly.diagnostics.dropped_sections.is_empty();
        prompt_assembly.diagnostics.session_history_compaction = Some(compaction);
    }
    let prompt_diagnostics = prompt_assembly.diagnostics.clone();
    let (warnings, prompt_warning_state) = filter_prompt_warnings(
        ctx,
        derive_prompt_warnings(&prompt_diagnostics, kernel_config),
    );
    tracing::debug!(
        session_id = %session_id,
        agent_id = %agent_id,
        prompt_mode = ?prompt_diagnostics.mode,
        included_sections = ?prompt_diagnostics.included_sections,
        dropped_sections = ?prompt_diagnostics.dropped_sections,
        context_window_tokens = prompt_diagnostics.window_usage.context_window,
        reserved_output_tokens = prompt_diagnostics.window_usage.reserved_output_tokens,
        compaction_buffer_tokens = prompt_diagnostics.window_usage.compaction_buffer_tokens,
        effective_prompt_budget_tokens = prompt_diagnostics.window_usage.effective_prompt_budget,
        static_prefix_tokens = prompt_diagnostics.window_usage.static_prefix_tokens,
        dynamic_suffix_tokens = prompt_diagnostics.window_usage.dynamic_suffix_tokens,
        cache_read_input_tokens = prompt_diagnostics.window_usage.cache_read_input_tokens,
        cache_creation_input_tokens = prompt_diagnostics.window_usage.cache_creation_input_tokens,
        within_effective_budget = prompt_diagnostics.window_usage.within_effective_budget,
        static_estimated_tokens = prompt_diagnostics.static_estimated_tokens,
        dynamic_estimated_tokens = prompt_diagnostics.dynamic_estimated_tokens,
        total_estimated_tokens = prompt_diagnostics.total_estimated_tokens,
        session_history_compaction = ?prompt_diagnostics.session_history_compaction,
        bootstrap_files = ?prompt_diagnostics.bootstrap_files,
        "prompt diagnostics"
    );
    if !warnings.is_empty() {
        warn!(
            session_id = %session_id,
            agent_id = %agent_id,
            warnings = ?warnings,
            "prompt warnings emitted"
        );
    }
    emit_prompt_warning_events(ctx, &warnings).await;
    let max_output_tokens = Some(kernel_config.output.effective_max_tokens());

    Ok(PreparedGeneralTurn {
        session_id: session_id.clone(),
        invocation_id,
        agent_id,
        project_root: ctx.project_root.clone(),
        selected_model,
        max_output_tokens,
        prompt_prefix_messages: split_prompt_prefix_messages(
            &render_prompt_assembly(&prompt_assembly),
            input,
        ),
        messages: render_prompt_assembly(&prompt_assembly),
        tool_definitions: tool_definition_provider.get_tool_definitions_for_request(
            &build_tool_selection_request(&session_id, input, phase, kernel_config),
        ),
        prompt_diagnostics,
        warnings,
        prompt_warning_state,
        stream_tx,
    })
}

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

#[allow(clippy::too_many_arguments)]
pub async fn persist_general_turn_artifacts(
    prepared: &PreparedGeneralTurn,
    input: &str,
    executed: &ExecutedTurnArtifacts,
    llm_provider: Arc<dyn LlmProvider>,
    memory_store: Arc<MemoryStore>,
    kernel_config: &AgentKernelConfig,
) {
    let seq = match memory_store.next_turn_seq(&prepared.session_id).await {
        Ok(seq) => seq,
        Err(err) => {
            warn!(
                session_id = %prepared.session_id,
                error = %err,
                "failed to allocate next turn seq"
            );
            return;
        }
    };
    let turn_id = Uuid::new_v4().to_string();
    let now = Utc::now();
    let mut items = Vec::with_capacity(executed.output_items.len().saturating_add(1));
    items.push(TalosOutputItem::UserInput {
        text: input.to_string(),
    });
    items.extend(executed.output_items.clone());
    let talos_turn = TalosTurn {
        turn_id: turn_id.clone(),
        session_id: prepared.session_id.to_string(),
        seq,
        timestamp: now,
        items,
        usage: executed.usage.clone(),
        stop_reason: to_api_stop_reason(executed.stop_reason),
    };
    if let Err(err) = memory_store.append_turn(&talos_turn).await {
        warn!(
            session_id = %prepared.session_id,
            error = %err,
            "failed to append memory store turn"
        );
    }

    if let Err(err) = compact_workspace_context_if_needed(
        &prepared.session_id,
        &prepared.selected_model,
        llm_provider.clone(),
        memory_store.as_ref(),
        kernel_config,
    )
    .await
    {
        warn!(
            session_id = %prepared.session_id,
            error = %err,
            "failed to compact workspace memory context"
        );
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn run_general_turn(
    ctx: &mut InvocationContext,
    agent_name: &str,
    agent_description: &str,
    input: &str,
    llm_provider: Arc<dyn LlmProvider>,
    tool_definition_provider: Arc<dyn ToolDefinitionProvider>,
    tool_executor_port: Arc<dyn ToolExecutorPort>,
    memory_store: Arc<MemoryStore>,
    kernel_config: &AgentKernelConfig,
) -> TalosResult<GeneralTurnResult> {
    let prepared = prepare_general_turn(
        ctx,
        agent_name,
        agent_description,
        input,
        llm_provider.clone(),
        tool_definition_provider,
        memory_store.clone(),
        kernel_config,
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
            );
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
            );
        }
        return Err(blocked_compaction_error(&prepared.session_id));
    }
    let executed = execute_general_turn_loop(
        prepared.clone(),
        agent_name,
        llm_provider.clone(),
        tool_executor_port,
        memory_store.clone(),
        kernel_config,
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
    persist_general_turn_artifacts(
        &prepared,
        input,
        &executed,
        llm_provider.clone(),
        memory_store,
        kernel_config,
    )
    .await;
    if let Ok(value) = serde_json::to_value(next_session_compaction_state(&prompt_diagnostics)) {
        stage_session_state_update(
            ctx,
            &prepared.session_id,
            &prepared.invocation_id,
            ExecutionBranch::ModelThinking,
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
                ExecutionBranch::ModelThinking,
                SESSION_CONTINUITY_STATE_KEY,
                value,
            );
        }
    }

    Ok(GeneralTurnResult {
        output_text: executed.output_text,
        output_items: executed.output_items,
        stop_reason: executed.stop_reason,
        usage: executed.usage,
        prompt_diagnostics,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{KernelWarning, PromptDynamicSectionDroppedWarning};
    use serde_json::json;

    fn build_executed_turn_artifacts(
        tool_call_log: Vec<ToolCallRecord>,
        tool_result_log: Vec<ToolResultLog>,
    ) -> ExecutedTurnArtifacts {
        ExecutedTurnArtifacts {
            output_text: "ok".to_string(),
            output_items: Vec::new(),
            stop_reason: StopReason::EndTurn,
            usage: TalosUsage::default(),
            reasoning_log: Vec::new(),
            tool_call_log,
            tool_result_log,
            saw_output_delta: false,
        }
    }

    #[test]
    fn test_build_session_continuity_records_recent_goal_failure_and_retry_guidance() {
        let executed = build_executed_turn_artifacts(
            vec![ToolCallRecord {
                call_id: "call-1".to_string(),
                name: "Bash".to_string(),
                arguments: json!({"path": "/tmp/project/src/lib.rs"}),
            }],
            vec![ToolResultLog {
                call_id: "call-1".to_string(),
                tool: "Bash".to_string(),
                payload: json!({"error": "permission denied"}),
                envelope: json!({}),
                is_error: true,
                truncated: false,
                size_bytes: 0,
                semantic_events: Vec::new(),
                next_turn_failure: None,
            }],
        );

        let continuity =
            build_session_continuity("Inspect the failing file update flow.", &executed)
                .expect("continuity should be built");

        assert_eq!(
            continuity.recent_goal,
            "Inspect the failing file update flow."
        );
        assert_eq!(
            continuity.last_failure.as_deref(),
            Some("Bash returned an error")
        );
        assert_eq!(
            continuity.suggested_next_step.as_deref(),
            Some("adjust the failing path, command, or tool arguments before retrying")
        );
        assert_eq!(
            continuity.pending_tasks,
            vec!["fix the failing tool step and retry".to_string()]
        );
    }

    #[test]
    fn test_build_session_continuity_prefers_verified_evidence_guidance_without_failure() {
        let executed = build_executed_turn_artifacts(
            vec![ToolCallRecord {
                call_id: "call-1".to_string(),
                name: "Read".to_string(),
                arguments: json!({"path": "/tmp/project/Cargo.toml"}),
            }],
            vec![ToolResultLog {
                call_id: "call-1".to_string(),
                tool: "Read".to_string(),
                payload: json!({"content": "[package]"}),
                envelope: json!({}),
                is_error: false,
                truncated: false,
                size_bytes: 0,
                semantic_events: Vec::new(),
                next_turn_failure: None,
            }],
        );

        let continuity = build_session_continuity("Summarize the package setup.", &executed)
            .expect("continuity should be built");

        assert_eq!(continuity.recent_goal, "Summarize the package setup.");
        assert_eq!(continuity.last_failure, None);
        assert_eq!(
            continuity.suggested_next_step.as_deref(),
            Some("answer from the verified evidence without re-reading the same paths")
        );
        assert_eq!(
            continuity.pending_tasks,
            vec!["complete the answer from the verified evidence".to_string()]
        );
    }

    #[test]
    fn test_prompt_warning_key_includes_dynamic_dropped_sections() {
        let warning =
            KernelWarning::PromptDynamicSectionDropped(PromptDynamicSectionDroppedWarning {
                sections: vec![
                    PromptSectionKind::ExpandedToolCatalog,
                    PromptSectionKind::RecentMessages,
                ],
            });

        assert_eq!(
            prompt_warning_key(&warning).as_deref(),
            Some("dynamic_dropped:ExpandedToolCatalog,RecentMessages")
        );
    }
}
