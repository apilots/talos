use super::prepare_compaction::*;
use super::*;
use crate::{
    EventAuthor, ExecutionBranch, KernelTranscriptEvent, KernelTranscriptEventKind,
    KernelTranscriptPayload,
};
use serde::de::DeserializeOwned;
use talos_config::SkillsRuntimeConfig;
use talos_prompt::{DynamicTrimPolicy, PromptSectionKind};
use talos_skills::{LocalSkillProvider, SkillPromptBudget, SkillPromptRequest};
use talos_tools::{ToolSelectionPhase, ToolSelectionRequest};
use talos_types::talos::{ContextCompactionOutcome, ContextCompactionStage};

pub(crate) const SESSION_CONTINUITY_STATE_KEY: &str = "session_continuity";
pub(crate) const PROMPT_WARNING_STATE_KEY: &str = "prompt_warning_state";
pub(crate) const LAST_PROMPT_DIAGNOSTICS_STATE_KEY: &str = "last_prompt_diagnostics";

fn build_prompt_load_policy(kernel_config: &AgentKernelConfig) -> PromptLoadPolicy {
    PromptLoadPolicy {
        compact_cursor: None,
        include_session_summary: true,
        include_recent_messages: true,
        include_recent_tool_history: true,
        include_recent_tool_outputs: true,
        session_recent_limit: kernel_config.compaction.recent_messages_keep,
        session_recent_max_chars: kernel_config.compaction.recent_messages_max_chars,
        recent_tool_history_limit: kernel_config.compaction.source_recent_tool_exchanges_limit,
        recent_tool_history_max_chars: kernel_config.compaction.tool_exchange_max_chars,
        recent_tool_outputs_limit: kernel_config.compaction.source_recent_tool_results_limit,
        recent_tool_outputs_max_chars: kernel_config.compaction.tool_output_max_chars,
        session_summary_recent_count: kernel_config.compaction.recent_summaries_keep,
        session_summary_max_chars: kernel_config.compaction.summary_max_chars,
        unified_recent_limit: 10,
    }
}

fn build_initial_prompt_load_policy(
    kernel_config: &AgentKernelConfig,
    compaction_state: Option<&SessionCompactionState>,
) -> PromptLoadPolicy {
    if compaction_state.is_some_and(SessionCompactionState::is_recovery) {
        build_compacted_prompt_load_policy(kernel_config, None)
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
        user_input: Some(input.to_string()),
        session_id: Some(session_id.to_string()),
    }
}

fn build_compacted_prompt_load_policy(
    kernel_config: &AgentKernelConfig,
    compact_cursor: Option<talos_prompt::CompactPromptCursor>,
) -> PromptLoadPolicy {
    PromptLoadPolicy {
        compact_cursor,
        include_session_summary: true,
        include_recent_messages: true,
        include_recent_tool_history: true,
        include_recent_tool_outputs: true,
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
        unified_recent_limit: 10,
    }
}

#[allow(clippy::too_many_arguments)]
fn build_prompt_builder<'a>(
    agent_name: &str,
    agent_description: &str,
    input: &str,
    tool_definition_provider: &'a dyn ToolDefinitionProvider,
    session_prompt_context: talos_prompt::SessionPromptContext,
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
    skill_options: Option<GeneralTurnSkillOptions>,
) -> TalosResult<PromptBuilder<'a>> {
    let mut builder = PromptBuilder::new()
        .agent(agent_name.to_string(), agent_description.to_string())
        .phase(phase)
        .planner_mode(kernel_config.planner_mode.as_str())?
        .planning_instruction(planning_instruction)
        .config(PromptBuildConfig {
            load_policy,
            workspace_dir: kernel_config.workspace_dir.clone(),
            prompt: kernel_config.prompt.clone(),
            tools: kernel_config.tools.clone(),
            timezone: kernel_config.timezone.clone(),
        })
        .tools(tool_definition_provider)
        .session_prompt_context(session_id, session_prompt_context)
        .model_info(selected_model_info)
        .session_environment(session_environment)
        .project_root(project_root)
        .session_continuity(session_continuity)
        .dynamic_trim_policy(dynamic_trim_policy)
        .user_input(input.to_string());

    let skill_options = skill_options.ok_or_else(|| {
        TalosError::internal(
            "skill options must be provided by the orchestration layer".to_string(),
        )
    })?;
    let roots = skill_options.roots.ok_or_else(|| {
        TalosError::internal("skill roots must be provided by the orchestration layer".to_string())
    })?;
    let provider = std::sync::Arc::new(LocalSkillProvider::new(roots));
    builder = builder.skills(
        provider,
        SkillPromptRequest {
            text: input.to_string(),
            selected: skill_options.selected,
            budget: build_skill_prompt_budget(&kernel_config.skills),
        },
    );

    Ok(builder)
}

fn build_skill_prompt_budget(config: &SkillsRuntimeConfig) -> SkillPromptBudget {
    SkillPromptBudget {
        max_available_summary_bytes: config.max_available_summary_bytes,
        max_skill_md_bytes: config.max_skill_md_bytes,
    }
}

#[allow(clippy::too_many_arguments)]
async fn rebuild_prompt_assembly(
    agent_name: &str,
    agent_description: &str,
    input: &str,
    tool_definition_provider: Arc<dyn ToolDefinitionProvider>,
    session_artifact_store: Arc<dyn talos_store::session::SessionArtifactStore>,
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
    skill_options: Option<GeneralTurnSkillOptions>,
) -> TalosResult<talos_prompt::PromptAssembly> {
    let context = crate::build_session_prompt_context(
        session_artifact_store.as_ref(),
        std::path::Path::new(&kernel_config.workspace_dir),
        session_id,
        &SessionPromptContextConfig {
            compact_cursor: load_policy.compact_cursor,
            recent_window: SessionPromptRecentWindow::Latest,
            recent_messages_limit: if load_policy.include_recent_messages {
                load_policy.session_recent_limit
            } else {
                0
            },
            recent_tool_results_limit: if load_policy.include_recent_tool_outputs {
                load_policy.recent_tool_outputs_limit
            } else {
                0
            },
            tool_output_max_chars: load_policy.recent_tool_outputs_max_chars,
            recent_tool_exchanges_limit: if load_policy.include_recent_tool_history {
                load_policy.recent_tool_history_limit
            } else {
                0
            },
            tool_exchange_max_chars: load_policy.recent_tool_history_max_chars,
            include_summary: load_policy.include_session_summary,
            summary_recent_count: load_policy.session_summary_recent_count,
            summary_max_chars: load_policy.session_summary_max_chars,
        },
    )
    .await?;
    let session_prompt_context = talos_prompt::SessionPromptContext {
        summaries: context
            .summaries
            .into_iter()
            .map(|summary| talos_prompt::SessionPromptSummary {
                id: summary.header.id,
                content: summary.content,
            })
            .collect(),
        recent_messages: context
            .recent_messages
            .into_iter()
            .map(|message| talos_prompt::SessionPromptMessage {
                id: message.header.id,
                role: match message.role {
                    TalosMessageRole::User => talos_prompt::TalosMessageRole::User,
                    TalosMessageRole::Assistant => talos_prompt::TalosMessageRole::Assistant,
                    TalosMessageRole::System => talos_prompt::TalosMessageRole::System,
                },
                text: message.text,
            })
            .collect(),
        recent_tool_results: context
            .recent_tool_results
            .into_iter()
            .map(|result| talos_prompt::SessionToolResultContext {
                tool_name: result.tool_name,
                content: result.content,
                is_error: result.is_error,
                truncated: result.truncated,
            })
            .collect(),
        recent_tool_exchanges: context
            .recent_tool_exchanges
            .into_iter()
            .map(|exchange| talos_prompt::SessionToolExchangeContext {
                tool_name: exchange.tool_name,
                arguments_preview: exchange.arguments_preview,
                outcome: match exchange.outcome {
                    SessionToolExchangeOutcome::Succeeded => {
                        talos_prompt::SessionToolExchangeOutcome::Succeeded
                    }
                    SessionToolExchangeOutcome::Failed => {
                        talos_prompt::SessionToolExchangeOutcome::Failed
                    }
                    SessionToolExchangeOutcome::Pending => {
                        talos_prompt::SessionToolExchangeOutcome::Pending
                    }
                },
                output_preview: exchange.output_preview,
                output_truncated: exchange.output_truncated,
            })
            .collect(),
    };
    build_prompt_builder(
        agent_name,
        agent_description,
        input,
        tool_definition_provider.as_ref(),
        session_prompt_context,
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
        skill_options,
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
    memory_writer: Arc<dyn MemoryWriter>,
    memory_scope: &MemoryScope,
    session_artifact_store: Arc<dyn talos_store::session::SessionArtifactStore>,
    session_store: Arc<dyn talos_store::session::SessionStore>,
    kernel_config: &AgentKernelConfig,
    session_id: &str,
    selected_model: &str,
    selected_model_info: talos_types::ModelInfo,
    phase: PromptPhase,
    planning_instruction: Option<String>,
    session_environment: Option<talos_types::talos::TalosSessionEnvironment>,
    project_root: Option<std::path::PathBuf>,
    session_continuity: Option<SessionContinuity>,
    skill_options: Option<GeneralTurnSkillOptions>,
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
    let compaction_usage = match compact_session_memory(
        MemoryFormationParams {
            session_id,
            selected_model,
            llm_provider,
            memory_writer: memory_writer.as_ref(),
            memory_scope,
            session_artifact_store: session_artifact_store.as_ref(),
            session_store: session_store.as_ref(),
            kernel_config,
        },
        MemoryCompactionPolicy::for_compaction().with_compaction_input_budget_tokens(
            resolve_compaction_input_budget_tokens(&selected_model_info),
        ),
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
                    covers_from_seq: None,
                    covers_until_seq: None,
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

    let compact_cursor =
        latest_compact_prompt_cursor(session_artifact_store.as_ref(), kernel_config, session_id)
            .await?;
    let compacted_assembly = rebuild_prompt_assembly(
        agent_name,
        agent_description,
        input,
        tool_definition_provider,
        session_artifact_store,
        kernel_config,
        session_id,
        phase,
        planning_instruction,
        selected_model_info,
        session_environment,
        project_root,
        session_continuity,
        build_compacted_prompt_load_policy(kernel_config, compact_cursor),
        DynamicTrimPolicy::PreserveAll,
        skill_options,
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
    let outcome = if summary_sections_after > 0 && compaction_usage.covers_until_seq.is_some() {
        SessionHistoryCompactionOutcome::Applied
    } else {
        SessionHistoryCompactionOutcome::RejectedMissingSummary
    };
    Ok(Some((
        (outcome == SessionHistoryCompactionOutcome::Applied).then_some(compacted_assembly),
        SessionHistoryCompaction {
            trigger,
            outcome,
            covers_from_seq: compaction_usage.covers_from_seq,
            covers_until_seq: compaction_usage.covers_until_seq,
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

pub(crate) fn blocked_compaction_error(session_id: &str) -> talos_error::TalosError {
    talos_error::TalosError::public(talos_error::ErrorCode::ContextCompactionFailed, format!(
        "session `{session_id}` is blocked because prompt budget remains exhausted after compaction; reset_session or create a new session to continue"
    ))
}

pub(crate) fn build_blocked_compaction_state() -> SessionCompactionState {
    SessionCompactionState::blocked_prompt_budget_exhausted()
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

pub(crate) fn prompt_budget_still_exhausted_from_diagnostics(
    diagnostics: &PromptDiagnostics,
) -> bool {
    diagnostics.window_usage.window_resolved && !diagnostics.window_usage.within_effective_budget
}

pub(crate) fn prompt_warning_key(warning: &KernelWarning) -> Option<String> {
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

pub(crate) async fn stage_session_state_update(
    ctx: &mut InvocationContext,
    session_id: &str,
    invocation_id: &str,
    branch: ExecutionBranch,
    key: &str,
    value: serde_json::Value,
) -> TalosResult<()> {
    let mut event = KernelTranscriptEvent::new(
        session_id.to_string(),
        invocation_id.to_string(),
        branch,
        EventAuthor::System,
        KernelTranscriptEventKind::Planning,
        KernelTranscriptPayload::StateOnly,
    );
    event.actions.state_delta.push(crate::StateDelta {
        namespace: crate::StateNamespace::Session,
        key: key.to_string(),
        value,
    });
    if let Some(transcript_service) = ctx.transcript_service.clone() {
        transcript_service
            .append_event(&mut ctx.session, event)
            .await?;
    } else {
        stage_kernel_transcript_event(ctx, event);
    }
    Ok(())
}

fn stage_kernel_transcript_event(ctx: &mut InvocationContext, event: KernelTranscriptEvent) {
    for delta in &event.actions.state_delta {
        ctx.session.state.apply_delta(delta);
    }
    ctx.session.events.push(event);
}

pub async fn prepare_general_turn(
    ctx: &mut InvocationContext,
    agent_name: &str,
    agent_description: &str,
    input: &str,
    llm_provider: Arc<dyn LlmProvider>,
    tool_definition_provider: Arc<dyn ToolDefinitionProvider>,
    memory_writer: Arc<dyn MemoryWriter>,
    kernel_config: &AgentKernelConfig,
    skill_options: Option<GeneralTurnSkillOptions>,
) -> TalosResult<PreparedGeneralTurn> {
    let session_id = ctx.session_id.clone();
    let message_id = ctx.message_id.clone();
    let invocation_id = ctx.invocation_id.clone();
    let agent_id = ctx.agent_id.clone();
    let selected_model = ctx.selected_model_id().to_string();
    let (attachments, attachment_tokens) =
        super::attachments::prepare_model_attachments(ctx, llm_provider.as_ref())?;
    let session_artifact_store = ctx.session_artifact_store()?;
    let session_store = ctx.session_store()?;
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
        session_artifact_store.clone(),
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
        skill_options.clone(),
    )
    .await?;
    super::attachments::reserve_prompt_attachment_tokens(&mut prompt_assembly, attachment_tokens)?;
    let mut session_history_compaction = None;
    let history_compaction_trigger =
        determine_history_compaction_trigger(&prompt_assembly, kernel_config);
    let context_compaction_trigger = history_compaction_trigger
        .clone()
        .map(map_context_compaction_trigger);
    if let Some(trigger) = context_compaction_trigger {
        emit_context_compaction_stream_event(
            stream_tx.as_ref(),
            trigger,
            ContextCompactionStage::Preparing,
            None,
            None,
            Some("Preparing session history compaction.".to_string()),
        )
        .await;
        emit_context_compaction_stream_event(
            stream_tx.as_ref(),
            trigger,
            ContextCompactionStage::Summarizing,
            None,
            None,
            Some("Summarizing session history.".to_string()),
        )
        .await;
    }
    let compaction_result = compact_session_history_if_needed(
        &prompt_assembly,
        agent_name,
        agent_description,
        input,
        llm_provider.clone(),
        tool_definition_provider.clone(),
        memory_writer,
        &MemoryScope {
            user_id: ctx.user_id.clone(),
            workspace_root: std::path::PathBuf::from(&kernel_config.workspace_dir),
        },
        session_artifact_store.clone(),
        session_store,
        kernel_config,
        &session_id,
        &selected_model,
        ctx.selected_model_info.clone(),
        phase,
        ctx.planning_instruction.clone(),
        ctx.session_environment.clone(),
        ctx.project_root.clone(),
        session_continuity.clone(),
        skill_options.clone(),
    )
    .await;
    let maybe_compaction = match compaction_result {
        Ok(value) => value,
        Err(error) => {
            if let Some(trigger) = context_compaction_trigger {
                emit_context_compaction_stream_event(
                    stream_tx.as_ref(),
                    trigger,
                    ContextCompactionStage::Failed,
                    Some(context_compaction_failure_outcome(&error)),
                    None,
                    Some("Session history compaction failed.".to_string()),
                )
                .await;
            }
            stage_session_state_update(
                ctx,
                &session_id,
                &invocation_id,
                ExecutionBranch::ModelThinking,
                SESSION_COMPACTION_STATE_KEY,
                serde_json::to_value(SessionCompactionState::normal()).map_err(|error| {
                    TalosError::internal(format!("serialize session compaction state: {error}"))
                })?,
            )
            .await?;
            return Err(error);
        }
    };
    if let Some((mut compacted_assembly, compaction)) = maybe_compaction {
        if let Some(assembly) = compacted_assembly.as_mut() {
            super::attachments::reserve_prompt_attachment_tokens(assembly, attachment_tokens)?;
        }
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
        let compacted_prompt_applied = compacted_assembly.is_some();
        if let Some(next_prompt_assembly) = compacted_assembly {
            prompt_assembly = next_prompt_assembly;
        }
        if compacted_prompt_applied {
            if let Some(trigger) = context_compaction_trigger {
                emit_context_compaction_stream_event(
                    stream_tx.as_ref(),
                    trigger,
                    ContextCompactionStage::RebuildingPrompt,
                    None,
                    None,
                    Some("Rebuilding prompt after session history compaction.".to_string()),
                )
                .await;
            }
        }
        let state = if compaction.outcome == SessionHistoryCompactionOutcome::Applied {
            SessionCompactionState::recovery()
        } else {
            SessionCompactionState::normal()
        };
        stage_session_state_update(
            ctx,
            &session_id,
            &invocation_id,
            ExecutionBranch::ModelThinking,
            SESSION_COMPACTION_STATE_KEY,
            serde_json::to_value(state).map_err(|error| {
                TalosError::internal(format!("serialize session compaction state: {error}"))
            })?,
        )
        .await?;
        session_history_compaction = Some(compaction);
    }

    if prompt_budget_still_exhausted(&prompt_assembly) {
        if session_history_compaction.is_some() {
            if let Some(trigger) = context_compaction_trigger {
                emit_context_compaction_stream_event(
                    stream_tx.as_ref(),
                    trigger,
                    ContextCompactionStage::Trimming,
                    None,
                    None,
                    Some("Trimming prompt after session history compaction.".to_string()),
                )
                .await;
            }
        }
        let fallback_load_policy = if session_history_compaction
            .as_ref()
            .map(|report| report.outcome == SessionHistoryCompactionOutcome::Applied)
            .unwrap_or(false)
        {
            build_compacted_prompt_load_policy(
                kernel_config,
                latest_compact_prompt_cursor(
                    session_artifact_store.as_ref(),
                    kernel_config,
                    &session_id,
                )
                .await?,
            )
        } else {
            initial_load_policy
        };
        prompt_assembly = rebuild_prompt_assembly(
            agent_name,
            agent_description,
            input,
            tool_definition_provider.clone(),
            session_artifact_store,
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
            skill_options,
        )
        .await?;
        super::attachments::reserve_prompt_attachment_tokens(
            &mut prompt_assembly,
            attachment_tokens,
        )?;
    }

    let final_prompt_budget_exhausted = prompt_budget_still_exhausted(&prompt_assembly);
    if let Some(mut compaction) = session_history_compaction {
        compaction.fallback_section_drop_applied =
            !prompt_assembly.diagnostics.dropped_sections.is_empty();
        compaction.post_compaction_total_tokens =
            prompt_assembly.diagnostics.total_estimated_tokens;
        compaction.recent_messages_after = count_section_kind(
            &prompt_assembly.dynamic_suffix_sections,
            PromptSectionKind::RecentMessages,
        );
        compaction.summary_sections_after = count_section_kind(
            &prompt_assembly.dynamic_suffix_sections,
            PromptSectionKind::SessionSummary,
        );
        let terminal_stage = if final_prompt_budget_exhausted {
            ContextCompactionStage::Failed
        } else {
            ContextCompactionStage::Completed
        };
        let terminal_outcome = if final_prompt_budget_exhausted {
            ContextCompactionOutcome::FailedContextStillTooLarge
        } else if compaction.fallback_section_drop_applied {
            ContextCompactionOutcome::TrimmedFallbackApplied
        } else {
            map_context_compaction_outcome(compaction.outcome.clone())
        };
        let terminal_message = match terminal_outcome {
            ContextCompactionOutcome::Applied => "Session history compaction finished.",
            ContextCompactionOutcome::NoChanges => {
                "Session history compaction found no new content."
            }
            ContextCompactionOutcome::RejectedMissingSummary => {
                "Session history summary was not applied."
            }
            ContextCompactionOutcome::TrimmedFallbackApplied => {
                "Session history summary was unavailable; context was trimmed."
            }
            ContextCompactionOutcome::FailedContextStillTooLarge => {
                "Context compaction failed; context is still too large."
            }
            ContextCompactionOutcome::FailedProviderError => {
                "Context compaction failed because the provider was unavailable."
            }
            ContextCompactionOutcome::FailedInternalError => {
                "Context compaction failed because of an internal error."
            }
        };
        prompt_assembly.diagnostics.session_history_compaction = Some(compaction.clone());
        if let Some(trigger) = context_compaction_trigger {
            emit_context_compaction_stream_event(
                stream_tx.as_ref(),
                trigger,
                terminal_stage,
                Some(terminal_outcome),
                Some(build_context_compaction_metrics(&compaction)),
                Some(terminal_message.to_string()),
            )
            .await;
        }
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
    let reasoning_effort = ctx
        .model_selection
        .resolve_reasoning(&ctx.selected_model_info)?;
    let speed = ctx.selected_model_info.model.speed;

    let tool_selection_request =
        build_tool_selection_request(&session_id, input, phase, kernel_config);
    let tool_definitions =
        tool_definition_provider.get_tool_definitions_for_request(&tool_selection_request);
    let maintenance_usage = token_usage_from_session_history_compaction(
        prompt_diagnostics.session_history_compaction.as_ref(),
    );

    let (messages, prompt_prefix_messages) =
        super::attachments::render_prompt_messages(&prompt_assembly, input, attachments)?;

    Ok(PreparedGeneralTurn {
        session_id: session_id.clone(),
        user_id: ctx.user_id.clone(),
        message_id,
        invocation_id,
        agent_id,
        project_root: ctx.project_root.clone(),
        selected_model,
        max_output_tokens,
        reasoning_effort,
        speed,
        attachment_tokens,
        prompt_prefix_messages,
        messages,
        maintenance_usage,
        tool_definitions,
        tool_selection_request,
        prompt_diagnostics,
        warnings,
        prompt_warning_state,
        stream_tx,
        message_rx: None,
    })
}
