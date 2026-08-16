use crate::sandbox_labels::{sandbox_mode_label, user_permission_mode_label};
use crate::{
    bootstrap, budget, diagnostics, regions, BootstrapFileDiagnostic, ContinuityAnswerMode,
    ContinuityToolOutcome, DynamicTrimPolicy, PromptAssembly, PromptBuilder, PromptCacheState,
    PromptDiagnostics, PromptMode, PromptPhase, PromptSection, PromptSectionKind,
    PromptWindowUsage, RequestContextInjection, WorkspaceBootstrapFile,
};
use chrono::Utc;
use talos_config::parse_timezone;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_skills::{SkillPromptContext, SkillPromptInjection};

#[path = "assembler/git_repository_state.rs"]
mod git_repository_state;

use git_repository_state::inspect_git_repository_state;

fn section(kind: PromptSectionKind, payload: impl Into<String>) -> PromptSection {
    PromptSection {
        kind,
        payload: payload.into(),
        role: None,
        estimated_tokens: 0,
    }
}

fn build_identity_core_payload(builder: &PromptBuilder<'_>) -> String {
    format!(
        "- agent_name: {}\n- agent_role: {}\n- mission: operate as a long-running intelligent steward\n- evidence_discipline: prefer verified facts over confident guesses\n- completion_rule: finish with a concrete result or clearly state the blocker",
        builder.agent_name, builder.agent_description
    )
}

fn build_evidence_tooling_policy_payload() -> String {
    [
        "- evidence_order: code -> files -> tool output -> summaries",
        "- dedicated_tool_priority: Grep > Read > Glob > Bash",
        "- shell_fallback: use shell only when dedicated tools cannot answer directly",
        "- verification_rule: stop exploring once enough evidence exists to answer",
    ]
    .join("\n")
}

fn build_tool_choice_priority_payload() -> String {
    [
        "- honor_explicit_user_tool_when_viable: true",
        "- grep_before_read: true",
        "- glob_only_when_path_shape_is_missing_fact: true",
        "- bash_only_for_shell_required_operations: true",
        "- grep_use_case: search file contents, symbols, exports, routes, and entrypoints",
        "- read_use_case: read exact files after narrowing",
        "- glob_use_case: inspect path shape or expand subtrees",
        "- bash_use_case: shell-required operations only",
    ]
    .join("\n")
}

fn build_requested_tool_preference_template_payload() -> String {
    [
        "- respect_explicit_user_tool_when_it_can_answer_next_step: true",
        "- ignore_requested_tool_only_when_not_viable: true",
        "- after_requested_tool_narrows_scope_switch_to_best_dedicated_tool: true",
    ]
    .join("\n")
}

fn build_failure_recovery_policy_payload() -> String {
    [
        "- failed_actions_are_evidence: true",
        "- salvage_answer_if_possible: true",
        "- report_partial_evidence_in_final_answer: true",
    ]
    .join("\n")
}

fn build_task_protocol_library_payload() -> String {
    [
        "- codebase_analysis_protocol: start with code anchors, narrow with Grep, then read exact files",
        "- bug_investigation_protocol: localize failure before proposing a fix",
        "- change_planning_protocol: inspect current implementation before proposing edits",
        "- environment_verification_protocol: verify environment claims with direct evidence",
    ]
    .join("\n")
}

fn build_communication_policy_payload() -> String {
    [
        "- progress_updates: short and milestone-based",
        "- final_answer_must_distinguish_evidence_from_inference: true",
        "- exact_output_requests_must_be_honored: true",
    ]
    .join("\n")
}

fn build_requested_tool_preference_hint_payload(builder: &PromptBuilder<'_>) -> Option<String> {
    let user_input = builder.user_input.as_deref()?.to_ascii_lowercase();
    let tool_registry = builder.tool_registry?;
    let selection_request = builder.tool_selection_request();
    let mut requested_tools = tool_registry
        .get_tool_definitions_for_request(&selection_request)
        .into_iter()
        .filter_map(|tool| {
            let name = tool.name;
            user_input
                .contains(name.to_ascii_lowercase().as_str())
                .then_some(name)
        })
        .collect::<Vec<_>>();
    requested_tools.sort();
    requested_tools.dedup();
    if requested_tools.is_empty() {
        return None;
    }

    Some(format!(
        "- requested_tools: {}\n- next_step_bias: honor the explicitly named tool if it can answer the next verification step",
        requested_tools.join(", ")
    ))
}

pub(crate) fn build_expanded_tool_catalog_payload(builder: &PromptBuilder<'_>) -> Option<String> {
    let tool_registry = builder.tool_registry?;
    let selection_request = builder.tool_selection_request();
    let selection = tool_registry.get_tool_selection(&selection_request);
    if selection.model_definitions().is_empty() {
        return None;
    }

    let active_summary = selection
        .visible_specs()
        .into_iter()
        .map(|spec| {
            format!(
                "- {}: {}",
                spec.definition.name,
                spec.definition.description.trim()
            )
        })
        .collect::<Vec<_>>();

    let deferred_summary = selection
        .deferred_specs()
        .iter()
        .map(|spec| {
            format!(
                "- {}: {}",
                spec.definition.name,
                spec.definition.description.trim()
            )
        })
        .collect::<Vec<_>>();

    let mut lines = Vec::new();
    lines.extend(active_summary);
    if !deferred_summary.is_empty() {
        lines.push("- deferred_tools:".to_string());
        lines.extend(deferred_summary);
    }

    Some(lines.join("\n"))
}

fn build_planning_instruction_payload(builder: &PromptBuilder<'_>) -> Option<String> {
    builder
        .planning_instruction
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(|text| format!("- current_goal: {}", text))
}

fn load_skill_prompt_context(
    builder: &PromptBuilder<'_>,
) -> TalosResult<Option<SkillPromptContext>> {
    if let Some(context) = builder.skill_prompt_context.clone() {
        return Ok(Some(context));
    }
    let (Some(provider), Some(request)) = (
        builder.skill_provider.clone(),
        builder.skill_prompt_request.clone(),
    ) else {
        return Ok(None);
    };
    provider.prompt_context(request).map(Some)
}

fn build_skill_injection_payload(injections: &[SkillPromptInjection]) -> Option<String> {
    if injections.is_empty() {
        return None;
    }
    let mut blocks = Vec::new();
    for injection in injections {
        blocks.push(format!(
            "<skill name=\"{}\" root=\"{}\" path=\"{}\">\nRelative paths mentioned by this skill are relative to the skill root.\nDo not read bundled resources unless the skill instructs you to and the task needs them.\n\n{}\n</skill>",
            injection.name,
            injection.root.display(),
            injection.path.display(),
            injection.contents.trim()
        ));
    }
    Some(blocks.join("\n\n"))
}

fn build_recent_task_continuity_payload(builder: &PromptBuilder<'_>) -> Option<String> {
    let continuity = builder.session_continuity.as_ref()?;
    let mut lines = Vec::new();

    if !continuity.recent_goal.trim().is_empty() {
        lines.push(format!("- recent_goal: {}", continuity.recent_goal.trim()));
    }
    if !continuity.verified_paths.is_empty() {
        lines.push(format!(
            "- recent_verified_paths: {}",
            continuity
                .verified_paths
                .iter()
                .map(|evidence| evidence.path.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let successful_tools = continuity
        .tool_summaries
        .iter()
        .filter(|summary| summary.outcome == ContinuityToolOutcome::Success)
        .map(|summary| summary.tool_name.as_str())
        .collect::<Vec<_>>();
    if !successful_tools.is_empty() {
        lines.push(format!(
            "- recent_successful_tools: {}",
            successful_tools.join(", ")
        ));
    }
    let failed_tools = continuity
        .tool_summaries
        .iter()
        .filter(|summary| summary.outcome == ContinuityToolOutcome::Failure)
        .map(|summary| summary.tool_name.as_str())
        .collect::<Vec<_>>();
    if !failed_tools.is_empty() {
        lines.push(format!(
            "- recent_failed_tools: {}",
            failed_tools.join(", ")
        ));
    }
    if !continuity.pending_tasks.is_empty() {
        lines.push(format!(
            "- pending_tasks: {}",
            continuity.pending_tasks.join("; ")
        ));
    }
    if !continuity.recent_unhelpful_patterns.is_empty() {
        lines.push(format!(
            "- recent_unhelpful_patterns: {}",
            continuity.recent_unhelpful_patterns.join("; ")
        ));
    }
    if let Some(mode) = continuity.recent_answer_mode {
        lines.push(format!(
            "- recent_answer_mode: {}",
            match mode {
                ContinuityAnswerMode::CodeFirst => "code_first",
                ContinuityAnswerMode::DocumentationFirst => "documentation_first",
                ContinuityAnswerMode::DirectAnswer => "direct_answer",
                ContinuityAnswerMode::Mixed => "mixed",
            }
        ));
    }
    if let Some(last_failure) = continuity.last_failure.as_deref() {
        let normalized = last_failure.trim();
        if !normalized.is_empty() {
            lines.push(format!("- last_failure: {}", normalized));
        }
    }
    if let Some(next_step) = continuity.suggested_next_step.as_deref() {
        let normalized = next_step.trim();
        if !normalized.is_empty() {
            lines.push(format!("- suggested_next_step: {}", normalized));
        }
    }

    (!lines.is_empty()).then(|| lines.join("\n"))
}

fn build_session_summary_payload(summary_id: &str, content: &str) -> String {
    format!(
        "- summary_id: {}\n- summary: {}",
        summary_id,
        content.trim()
    )
}

fn role_label(role: &crate::TalosMessageRole) -> &'static str {
    match role {
        crate::TalosMessageRole::User => "user",
        crate::TalosMessageRole::Assistant => "assistant",
        crate::TalosMessageRole::System => "system",
    }
}

fn build_recent_message_payload(role: &crate::TalosMessageRole, text: &str) -> String {
    format!("- {}: {}", role_label(role), text.trim())
}

fn truncate_to_char_limit(value: &str, max_chars: usize) -> String {
    let trimmed = value.trim();
    if max_chars == 0 || trimmed.is_empty() {
        return String::new();
    }

    let total_chars = trimmed.chars().count();
    if total_chars <= max_chars {
        return trimmed.to_string();
    }

    if max_chars <= 3 {
        return trimmed.chars().take(max_chars).collect();
    }

    let prefix = trimmed.chars().take(max_chars - 3).collect::<String>();
    format!("{prefix}...")
}

fn build_capped_list_payload(lines: Vec<String>, max_chars: usize) -> Option<String> {
    if max_chars == 0 {
        return None;
    }
    let mut selected = Vec::new();
    let mut combined_chars = 0usize;
    for line in lines.into_iter().rev() {
        let normalized = line.trim();
        if normalized.is_empty() {
            continue;
        }
        let separator_chars = usize::from(!selected.is_empty());
        if combined_chars + separator_chars >= max_chars {
            break;
        }
        let remaining_chars = max_chars - combined_chars - separator_chars;
        let capped_line = truncate_to_char_limit(normalized, remaining_chars);
        if capped_line.is_empty() {
            break;
        }
        let line_was_truncated = capped_line.chars().count() < normalized.chars().count();
        combined_chars += separator_chars + capped_line.chars().count();
        selected.push(capped_line);
        if line_was_truncated {
            break;
        }
    }
    if selected.is_empty() {
        return None;
    }
    selected.reverse();
    Some(selected.join("\n"))
}

fn build_recent_messages_payload(
    messages: &[crate::SessionPromptMessage],
    max_chars: usize,
) -> Option<String> {
    let lines = messages
        .iter()
        .map(|message| build_recent_message_payload(&message.role, &message.text))
        .collect::<Vec<_>>();
    build_capped_list_payload(lines, max_chars)
}

fn build_recent_tool_history_payload(
    exchanges: &[crate::SessionToolExchangeContext],
    max_chars: usize,
) -> Option<String> {
    let lines = exchanges
        .iter()
        .map(|exchange| {
            let outcome = match exchange.outcome {
                crate::SessionToolExchangeOutcome::Succeeded => "succeeded",
                crate::SessionToolExchangeOutcome::Failed => "failed",
                crate::SessionToolExchangeOutcome::Pending => "pending",
            };
            let mut line = format!(
                "- tool: {} | outcome: {} | args: {}",
                exchange.tool_name,
                outcome,
                exchange.arguments_preview.trim()
            );
            if let Some(output_preview) = exchange
                .output_preview
                .as_ref()
                .map(|value| value.trim())
                .filter(|value| !value.is_empty())
            {
                let suffix = if exchange.output_truncated {
                    " [truncated]"
                } else {
                    ""
                };
                line.push_str(&format!(" | preview{suffix}: {output_preview}"));
            }
            line
        })
        .collect::<Vec<_>>();
    build_capped_list_payload(lines, max_chars)
}

fn build_recent_tool_outputs_payload(
    results: &[crate::SessionToolResultContext],
    max_chars: usize,
) -> Option<String> {
    let lines = results
        .iter()
        .map(|result| {
            let status = if result.is_error { "error" } else { "ok" };
            let suffix = if result.truncated { " [truncated]" } else { "" };
            format!(
                "- tool: {} | status: {} | output{}: {}",
                result.tool_name,
                status,
                suffix,
                result.content.trim()
            )
        })
        .collect::<Vec<_>>();
    build_capped_list_payload(lines, max_chars)
}

pub(crate) fn build_retrieved_memories_section(
    memories: &[talos_types::MemoryInfo],
    max_items: usize,
    max_chars: usize,
) -> Option<PromptSection> {
    let lines = memories
        .iter()
        .take(max_items)
        .map(|memory| {
            format!(
                "- {}: {}\n  {}",
                memory.memory_id,
                memory.title.trim(),
                memory.body.trim()
            )
        })
        .collect::<Vec<_>>();
    build_capped_list_payload(lines, max_chars)
        .map(|payload| section(PromptSectionKind::RetrievedMemories, payload))
}

fn build_appended_system_context(builder: &PromptBuilder<'_>) -> Option<String> {
    let mut lines = Vec::new();

    if let Some(project_root) = builder.project_root.as_ref() {
        lines.push(format!("- project_root: {}", project_root.display()));
        if let Some(repository_state) = inspect_git_repository_state(project_root) {
            lines.push(format!("- git_branch: {}", repository_state.branch));
            lines.push(format!(
                "- git_status_summary: {}",
                repository_state.status_summary
            ));
            lines.push(format!(
                "- has_uncommitted_changes: {}",
                repository_state.has_uncommitted_changes
            ));
        }
    }

    lines.push(format!(
        "- phase: {}",
        match builder.phase {
            PromptPhase::Planning => "planning",
            PromptPhase::Execution => "execution",
        }
    ));
    lines.push(format!("- planner_mode: {}", builder.planner_mode.as_str()));
    lines.push(format!(
        "- prompt_mode: {}",
        match builder.mode {
            PromptMode::Full => "full",
            PromptMode::Minimal => "minimal",
            PromptMode::None => "none",
        }
    ));
    lines.push(format!(
        "- configured_sandbox_request: {}",
        sandbox_mode_label(&builder.config.tools.sandbox_policy.default_mode)
    ));
    lines.push(format!(
        "- configured_user_permission_mode: {}",
        user_permission_mode_label(&builder.config.tools.sandbox_policy.user_permission_mode)
    ));

    if let Some(environment) = builder.session_environment.as_ref() {
        lines.push(format!(
            "- session_directory: {}",
            environment.directory.display()
        ));
        lines.push(format!(
            "- connection_mode: {}",
            environment.connection_mode
        ));
        lines.push("- workspace_state: session environment attached".to_string());
    } else if builder.project_root.is_some() {
        lines.push("- workspace_state: local repository context attached".to_string());
    }

    if lines.is_empty() {
        return None;
    }

    Some(format!(
        "[AppendedSystemContext]\nSystem runtime context for the current request:\n{}",
        lines.join("\n")
    ))
}

fn build_prepended_user_context_message(builder: &PromptBuilder<'_>) -> TalosResult<String> {
    let timezone = parse_timezone(&builder.config.timezone)?;
    let current = Utc::now().with_timezone(&timezone);
    Ok(format!(
        "[PrependedUserContextMessage]\n<system-reminder>\nRequest-local reminder and current time are shown below.\nUse them as high-priority request metadata.\n\n<request-local-reminder>\n- Use absolute dates when clarifying relative time references.\n</request-local-reminder>\n\n<current-date>\n{}\n</current-date>\n\n<current-timezone>\n{}\n</current-timezone>\n</system-reminder>",
        current.format("%Y-%m-%d"),
        builder.config.timezone
    ))
}

fn build_static_prefix_sections(
    builder: &PromptBuilder<'_>,
    bootstrap_files: &[WorkspaceBootstrapFile],
) -> TalosResult<(Vec<PromptSection>, Vec<BootstrapFileDiagnostic>)> {
    let mut sections = vec![
        section(
            PromptSectionKind::IdentityCore,
            build_identity_core_payload(builder),
        ),
        section(
            PromptSectionKind::EvidenceToolingPolicy,
            build_evidence_tooling_policy_payload(),
        ),
        section(
            PromptSectionKind::ToolChoicePriority,
            build_tool_choice_priority_payload(),
        ),
        section(
            PromptSectionKind::RequestedToolPreferenceTemplate,
            build_requested_tool_preference_template_payload(),
        ),
        section(
            PromptSectionKind::FailureRecoveryPolicy,
            build_failure_recovery_policy_payload(),
        ),
        section(
            PromptSectionKind::TaskProtocolLibrary,
            build_task_protocol_library_payload(),
        ),
        section(
            PromptSectionKind::CommunicationPolicy,
            build_communication_policy_payload(),
        ),
    ];

    let mut diagnostics = Vec::new();
    if builder.mode != PromptMode::None && builder.config.prompt.bootstrap.enabled {
        let per_file_limit = builder.config.prompt.bootstrap.max_chars_per_file.max(1);
        for file in bootstrap_files {
            let file_limit = if file.kind == crate::WorkspaceBootstrapFileKind::Memory {
                let raw_chars = file
                    .content
                    .as_deref()
                    .map(|content| content.chars().count())
                    .unwrap_or(0);
                if raw_chars > talos_types::MEMORY_FILE_MAX_CHARS {
                    return Err(TalosError::public(
                        ErrorCode::InvalidRequest,
                        format!(
                            "MEMORY.md exceeds the {} character limit",
                            talos_types::MEMORY_FILE_MAX_CHARS
                        ),
                    ));
                }
                talos_types::MEMORY_FILE_MAX_CHARS
            } else {
                per_file_limit
            };
            let (payload, diagnostic) = build_bootstrap_payload(file, file_limit);
            diagnostics.push(diagnostic);
            sections.push(section(regions::bootstrap_section_kind(file.kind), payload));
        }
    }

    sections.sort_by_key(|section| {
        regions::STATIC_PREFIX_ORDER
            .iter()
            .position(|kind| kind == &section.kind)
            .unwrap_or(usize::MAX)
    });

    Ok((sections, diagnostics))
}

fn build_bootstrap_payload(
    file: &WorkspaceBootstrapFile,
    per_file_limit: usize,
) -> (String, BootstrapFileDiagnostic) {
    let template = regions::region_template(regions::bootstrap_section_kind(file.kind));
    match file.content.as_ref() {
        None => (
            "- file_state: absent".to_string(),
            BootstrapFileDiagnostic {
                kind: file.kind,
                path: file.path.clone(),
                raw_chars: 0,
                injected_chars: 0,
                truncated: false,
                missing: true,
            },
        ),
        Some(content) => {
            let trimmed = content.trim();
            if trimmed.is_empty() {
                return (
                    "- file_state: empty".to_string(),
                    BootstrapFileDiagnostic {
                        kind: file.kind,
                        path: file.path.clone(),
                        raw_chars: 0,
                        injected_chars: 0,
                        truncated: false,
                        missing: false,
                    },
                );
            }

            let limited = bootstrap::truncate_chars(trimmed, per_file_limit);
            let raw_chars = trimmed.chars().count();
            let injected_chars = limited.chars().count();
            let payload = limited
                .lines()
                .map(str::trim_end)
                .filter(|line| !line.is_empty())
                .map(|line| format!("- {}", line))
                .collect::<Vec<_>>()
                .join("\n");
            (
                if payload.is_empty() {
                    format!("- file_state: present\n- source_file: {}", template.title)
                } else {
                    payload
                },
                BootstrapFileDiagnostic {
                    kind: file.kind,
                    path: file.path.clone(),
                    raw_chars,
                    injected_chars,
                    truncated: raw_chars != injected_chars,
                    missing: false,
                },
            )
        }
    }
}

async fn load_dynamic_sections(builder: &PromptBuilder<'_>) -> TalosResult<Vec<PromptSection>> {
    let mut sections = Vec::new();

    if let Some(payload) = build_requested_tool_preference_hint_payload(builder) {
        sections.push(section(
            PromptSectionKind::RequestedToolPreferenceHint,
            payload,
        ));
    }

    if let Some(payload) = build_planning_instruction_payload(builder) {
        sections.push(section(PromptSectionKind::PlanningInstruction, payload));
    }

    if let Some(payload) = build_recent_task_continuity_payload(builder) {
        sections.push(section(PromptSectionKind::RecentTaskContinuity, payload));
    }

    if let Some(payload) = build_expanded_tool_catalog_payload(builder) {
        sections.push(section(PromptSectionKind::ExpandedToolCatalog, payload));
    }

    if let Some(skill_context) = load_skill_prompt_context(builder)? {
        if !skill_context.available_summary.trim().is_empty() {
            sections.push(section(
                PromptSectionKind::SkillCatalog,
                skill_context.available_summary,
            ));
        }
        if let Some(payload) = build_skill_injection_payload(&skill_context.injections) {
            sections.push(section(PromptSectionKind::ActiveSkillInstructions, payload));
        }
        if !skill_context.warnings.is_empty() {
            let payload = skill_context
                .warnings
                .iter()
                .map(|warning| format!("- {}: {}", warning.code, warning.message))
                .collect::<Vec<_>>()
                .join("\n");
            sections.push(section(PromptSectionKind::ActiveSkillInstructions, payload));
        }
    }

    if let Some(context) = builder.session_prompt_context.as_ref() {
        let include_summary = builder.config.load_policy.include_session_summary;
        let include_recent = builder.config.load_policy.include_recent_messages;
        let include_recent_tool_history = builder.config.load_policy.include_recent_tool_history;
        let include_recent_tool_outputs = builder.config.load_policy.include_recent_tool_outputs;
        if include_summary {
            for summary in &context.summaries {
                let content = summary.content.trim();
                if !content.is_empty() {
                    sections.push(section(
                        PromptSectionKind::SessionSummary,
                        build_session_summary_payload(&summary.id, content),
                    ));
                }
            }
        }

        if include_recent {
            if let Some(payload) = build_recent_messages_payload(
                &context.recent_messages,
                builder.config.load_policy.session_recent_max_chars,
            ) {
                sections.push(section(PromptSectionKind::RecentMessages, payload));
            }
        }

        if include_recent_tool_history {
            if let Some(payload) = build_recent_tool_history_payload(
                &context.recent_tool_exchanges,
                builder.config.load_policy.recent_tool_history_max_chars,
            ) {
                sections.push(section(PromptSectionKind::RecentToolHistory, payload));
            }
        }

        if include_recent_tool_outputs {
            if let Some(payload) = build_recent_tool_outputs_payload(
                &context.recent_tool_results,
                builder.config.load_policy.recent_tool_outputs_max_chars,
            ) {
                sections.push(section(PromptSectionKind::RecentToolOutputs, payload));
            }
        }
    }

    sections.sort_by_key(|section| {
        regions::DYNAMIC_SUFFIX_ORDER
            .iter()
            .position(|kind| kind == &section.kind)
            .unwrap_or(usize::MAX)
    });

    Ok(sections)
}

fn build_cache_state(
    static_prefix_sections: &[PromptSection],
    dynamic_suffix_sections: &[PromptSection],
    builder: &PromptBuilder<'_>,
) -> PromptCacheState {
    let static_parts = static_prefix_sections
        .iter()
        .map(render_plain_section)
        .collect::<Vec<_>>();
    let bootstrap_parts = static_prefix_sections
        .iter()
        .filter(|section| section.kind.stable_bootstrap_kind().is_some())
        .map(render_plain_section)
        .collect::<Vec<_>>();
    let tool_catalog_fingerprint = dynamic_suffix_sections
        .iter()
        .find(|section| section.kind == PromptSectionKind::ExpandedToolCatalog)
        .map(|section| crate::compute_prompt_fingerprint(&[section.payload.as_str()]));
    let model_fingerprint = builder.model_info.as_ref().map(|model| {
        crate::compute_prompt_fingerprint(&[
            model.model.id.as_str(),
            model.provider.as_str(),
            &model.model.context_window.to_string(),
        ])
    });

    PromptCacheState {
        prompt_version: regions::PROMPT_ARCHITECTURE_VERSION.to_string(),
        static_prefix_fingerprint: crate::compute_prompt_fingerprint(
            &static_parts.iter().map(String::as_str).collect::<Vec<_>>(),
        ),
        bootstrap_fingerprint: (!bootstrap_parts.is_empty()).then(|| {
            crate::compute_prompt_fingerprint(
                &bootstrap_parts
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
            )
        }),
        tool_catalog_fingerprint,
        model_fingerprint,
        last_break_reason: None,
    }
}

fn render_plain_section(section: &PromptSection) -> String {
    let mut parts = vec![
        section.title().to_string(),
        section.framing_prompt().to_string(),
    ];
    if !section.payload.trim().is_empty() {
        parts.push(section.payload.trim().to_string());
    }
    parts.join("\n\n")
}

fn build_diagnostics(
    mode: PromptMode,
    static_sections: &[PromptSection],
    dynamic_sections: &[PromptSection],
    bootstrap_diagnostics: Vec<BootstrapFileDiagnostic>,
    dropped_sections: Vec<PromptSectionKind>,
    window_usage: PromptWindowUsage,
) -> PromptDiagnostics {
    diagnostics::build_prompt_diagnostics(
        mode,
        static_sections,
        dynamic_sections,
        bootstrap_diagnostics,
        dropped_sections,
        window_usage,
    )
}

pub(crate) async fn assemble(builder: &PromptBuilder<'_>) -> TalosResult<PromptAssembly> {
    let user_input = builder.user_input.clone().ok_or_else(|| {
        TalosError::public(
            ErrorCode::InvalidRequest,
            "PromptBuilder requires user input".to_string(),
        )
    })?;

    let mut bootstrap_files = builder.load_workspace_bootstrap_files().await?;
    bootstrap_files.extend(builder.load_project_bootstrap_files().await?);

    let (mut static_prefix_sections, bootstrap_diagnostics) =
        build_static_prefix_sections(builder, &bootstrap_files)?;
    let mut dynamic_suffix_sections = load_dynamic_sections(builder).await?;
    budget::enrich_sections(&mut static_prefix_sections);
    budget::enrich_sections(&mut dynamic_suffix_sections);

    let static_prefix_tokens = budget::estimate_sections_tokens(&static_prefix_sections);
    let mut dropped_sections = Vec::new();
    let mut window_usage = resolve_window_usage(
        builder,
        static_prefix_tokens,
        budget::estimate_sections_tokens(&dynamic_suffix_sections),
    );
    if matches!(builder.dynamic_trim_policy, DynamicTrimPolicy::TrimToBudget)
        && window_usage.window_resolved
        && !window_usage.within_effective_budget
    {
        let dynamic_budget = window_usage
            .effective_prompt_budget
            .saturating_sub(static_prefix_tokens);
        budget::trim_dynamic_suffix_to_budget(
            &mut dynamic_suffix_sections,
            dynamic_budget,
            &mut dropped_sections,
        );
        window_usage = resolve_window_usage(
            builder,
            static_prefix_tokens,
            budget::estimate_sections_tokens(&dynamic_suffix_sections),
        );
    }

    let request_context_injection = RequestContextInjection {
        appended_system_context: build_appended_system_context(builder),
        prepended_user_context_message: Some(build_prepended_user_context_message(builder)?),
    };
    let diagnostics = build_diagnostics(
        builder.mode,
        &static_prefix_sections,
        &dynamic_suffix_sections,
        bootstrap_diagnostics,
        dropped_sections,
        window_usage,
    );
    let cache_state = build_cache_state(&static_prefix_sections, &dynamic_suffix_sections, builder);

    Ok(PromptAssembly {
        static_prefix_sections,
        dynamic_suffix_sections,
        request_context_injection,
        user_input,
        diagnostics,
        cache_state,
    })
}

fn resolve_window_usage(
    builder: &PromptBuilder<'_>,
    static_prefix_tokens: usize,
    dynamic_suffix_tokens: usize,
) -> PromptWindowUsage {
    PromptWindowUsage::resolve(
        &builder.config.prompt,
        builder
            .model_info
            .as_ref()
            .map(|model| model.model.context_window as usize)
            .unwrap_or_default(),
        static_prefix_tokens,
        dynamic_suffix_tokens,
    )
}

trait PromptSectionKindExt {
    fn stable_bootstrap_kind(self) -> Option<crate::WorkspaceBootstrapFileKind>;
}

impl PromptSectionKindExt for PromptSectionKind {
    fn stable_bootstrap_kind(self) -> Option<crate::WorkspaceBootstrapFileKind> {
        regions::region_template(self).bootstrap_kind
    }
}
