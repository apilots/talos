use super::*;
use crate::{EventAuthor, ExecutionBranch, SessionEvent, SessionEventKind, SessionEventPayload};
use serde::de::DeserializeOwned;
use talos_prompt::PromptSectionKind;
use talos_tools::{PermissionMode, ToolSelectionPhase, ToolSelectionRequest};

pub(crate) const SESSION_CONTINUITY_STATE_KEY: &str = "session_continuity";
pub(crate) const PROMPT_WARNING_STATE_KEY: &str = "prompt_warning_state";
pub(crate) const LAST_PROMPT_DIAGNOSTICS_STATE_KEY: &str = "last_prompt_diagnostics";

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
        include_session_summary: kernel_config.prompt.include_session_summary,
        include_recent_messages: kernel_config.prompt.include_recent_messages,
        include_retrieved_memories: kernel_config.prompt.include_retrieved_memories,
        session_recent_limit: kernel_config.prompt.session_recent_messages_limit,
        retrieved_memory_max_items: kernel_config.prompt.retrieved_memory_max_items,
        retrieved_memory_max_chars: kernel_config.prompt.retrieved_memory_max_chars,
        unified_recent_limit: 10,
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
        include_retrieved_memories: kernel_config.prompt.include_retrieved_memories,
        session_recent_limit: kernel_config.prompt.session_summary_recent_count.max(1),
        retrieved_memory_max_items: kernel_config.prompt.retrieved_memory_max_items,
        retrieved_memory_max_chars: kernel_config.prompt.retrieved_memory_max_chars,
        unified_recent_limit: 10,
    }
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
) -> TalosResult<Option<(talos_prompt::PromptAssembly, SessionHistoryCompaction)>> {
    let window_usage = &prompt_assembly.diagnostics.window_usage;
    let history_reduction_needed = history_sections_were_dropped(prompt_assembly);
    if !window_usage.window_resolved
        || (window_usage.within_effective_budget && !history_reduction_needed)
    {
        return Ok(None);
    }

    let pre_recent_messages = count_section_kind(
        &prompt_assembly.dynamic_suffix_sections,
        PromptSectionKind::RecentMessages,
    );
    let flush_workspace_memory = kernel_config.memory.workspace_memory_flush_enabled
        && kernel_config.memory.workspace_memory_flush_on_compaction;
    let compaction_usage = flush_session_memory_with_report(
        session_id,
        selected_model,
        llm_provider,
        memory_store.as_ref(),
        kernel_config,
        MemoryFlushPolicy::new(false, true, flush_workspace_memory),
    )
    .await?;

    let compacted_builder = build_prompt_builder(
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
        build_compacted_prompt_load_policy(kernel_config),
    )?;
    let compacted_assembly = compacted_builder.assemble().await?;
    let summary_sections_after = count_section_kind(
        &compacted_assembly.dynamic_suffix_sections,
        PromptSectionKind::SessionSummary,
    );
    if summary_sections_after == 0 {
        return Ok(None);
    }

    let recent_messages_after = count_section_kind(
        &compacted_assembly.dynamic_suffix_sections,
        PromptSectionKind::RecentMessages,
    );
    Ok(Some((
        compacted_assembly.clone(),
        SessionHistoryCompaction {
            pre_compaction_total_tokens: prompt_assembly.diagnostics.total_estimated_tokens,
            post_compaction_total_tokens: compacted_assembly.diagnostics.total_estimated_tokens,
            recent_messages_before: pre_recent_messages,
            recent_messages_after,
            summary_sections_after,
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
        KernelWarning::PromptDynamicSectionTruncated(payload) => Some(format!(
            "dynamic_truncated:{}",
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

pub(crate) fn build_session_continuity(
    executed: &ExecutedTurnArtifacts,
) -> Option<SessionContinuity> {
    let failed_call_ids = executed
        .tool_result_log
        .iter()
        .filter(|log| log.is_error)
        .map(|log| log.call_id.clone())
        .collect::<std::collections::HashSet<_>>();

    let mut recent_verified_paths = Vec::new();
    let mut recent_successful_tools = Vec::new();
    let mut recent_failed_tools = Vec::new();
    let mut grep_patterns = Vec::new();

    for call in &executed.tool_call_log {
        let tool_name = call.name.clone();
        if failed_call_ids.contains(&call.call_id) {
            if !recent_failed_tools.contains(&tool_name) {
                recent_failed_tools.push(tool_name);
            }
        } else if !recent_successful_tools.contains(&tool_name) {
            recent_successful_tools.push(tool_name.clone());
        }

        for key in ["file_path", "dir_path", "path"] {
            if let Some(path) = call.arguments.get(key).and_then(|value| value.as_str()) {
                if !recent_verified_paths.contains(&path.to_string()) {
                    recent_verified_paths.push(path.to_string());
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
    if !recent_failed_tools.is_empty() {
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
        && recent_verified_paths.len() < executed.tool_call_log.len().min(3)
    {
        recent_unhelpful_patterns
            .push("avoid re-reading the same path when no new evidence is needed".to_string());
    }

    let recent_answer_mode = if recent_successful_tools
        .iter()
        .any(|tool| matches!(tool.as_str(), "Grep" | "Read" | "Glob"))
    {
        Some(ContinuityAnswerMode::CodeFirst)
    } else if executed.tool_call_log.is_empty() {
        Some(ContinuityAnswerMode::DirectAnswer)
    } else {
        Some(ContinuityAnswerMode::Mixed)
    };

    if recent_verified_paths.is_empty()
        && recent_successful_tools.is_empty()
        && recent_failed_tools.is_empty()
        && recent_unhelpful_patterns.is_empty()
        && recent_answer_mode.is_none()
    {
        None
    } else {
        Some(SessionContinuity {
            recent_verified_paths: recent_verified_paths.into_iter().take(4).collect(),
            recent_successful_tools: recent_successful_tools.into_iter().take(4).collect(),
            recent_failed_tools: recent_failed_tools.into_iter().take(4).collect(),
            recent_unhelpful_patterns: recent_unhelpful_patterns.into_iter().take(3).collect(),
            recent_answer_mode,
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
    let session_continuity =
        load_session_state_value::<SessionContinuity>(ctx, SESSION_CONTINUITY_STATE_KEY);

    let phase = match ctx.phase {
        crate::TurnPhase::Planning => PromptPhase::Planning,
        crate::TurnPhase::Execution => PromptPhase::Execution,
    };

    let prompt_builder = build_prompt_builder(
        agent_name,
        agent_description,
        input,
        tool_definition_provider.as_ref(),
        memory_store.as_ref(),
        kernel_config,
        &session_id,
        phase,
        ctx.planning_instruction.clone(),
        ctx.selected_model_info.clone(),
        ctx.session_environment.clone(),
        ctx.project_root.clone(),
        session_continuity.clone(),
        build_prompt_load_policy(kernel_config),
    )?;
    let mut prompt_assembly = prompt_builder.assemble().await?;
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
        session_continuity,
    )
    .await?
    {
        tracing::debug!(
            session_id = %session_id,
            agent_id = %agent_id,
            pre_tokens = compaction.pre_compaction_total_tokens,
            post_tokens = compaction.post_compaction_total_tokens,
            recent_messages_before = compaction.recent_messages_before,
            recent_messages_after = compaction.recent_messages_after,
            summary_sections_after = compaction.summary_sections_after,
            "session history compaction rebuilt prompt"
        );
        prompt_assembly = compacted_assembly;
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
        truncated_sections = ?prompt_diagnostics.truncated_sections,
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

    Ok(PreparedGeneralTurn {
        session_id: session_id.clone(),
        invocation_id,
        agent_id,
        project_root: ctx.project_root.clone(),
        selected_model,
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
            break (step_text, StopReason::EndTurn);
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
    if let Some(session_continuity) = build_session_continuity(&executed) {
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
    use crate::{
        KernelWarning, PromptDynamicSectionDroppedWarning, PromptDynamicSectionTruncatedWarning,
    };

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

    #[test]
    fn test_prompt_warning_key_includes_dynamic_truncated_sections() {
        let warning =
            KernelWarning::PromptDynamicSectionTruncated(PromptDynamicSectionTruncatedWarning {
                sections: vec![PromptSectionKind::RecentMessages],
            });

        assert_eq!(
            prompt_warning_key(&warning).as_deref(),
            Some("dynamic_truncated:RecentMessages")
        );
    }
}
