use crate::{
    bootstrap, budget, diagnostics, BootstrapFileDiagnostic, BootstrapFileScope, BudgetTier,
    CacheScope, ContinuityAnswerMode, PromptAssembly, PromptBuilder, PromptDiagnostics, PromptMode,
    PromptPhase, PromptSection, PromptSectionKind, PromptWindowUsage, TruncationMode,
    WorkspaceBootstrapFile,
};
use talos_error::{TalosError, TalosResult};
use talos_memory::{Memory, SessionPromptContextConfig};

fn build_identity_core_prompt(builder: &PromptBuilder<'_>) -> String {
    format!(
        r#"Identity and mission:
- You are {name}, {description}.
- Operate as a long-running intelligent steward, not a one-shot chatbot.
- Move the task forward with minimal user supervision while staying evidence-driven.
- Prefer a small number of high-value tool calls over broad or repetitive exploration.
- Separate verified facts from inference and say when evidence is still incomplete.
- Finish with a concrete result, or clearly state the remaining blocker."#,
        name = builder.agent_name,
        description = builder.agent_description,
    )
}

fn build_runtime_session_context_prompt(builder: &PromptBuilder<'_>) -> String {
    let phase_label = match builder.phase {
        PromptPhase::Planning => "planning",
        PromptPhase::Execution => "execution",
    };
    let planner_mode = builder.planner_mode.as_str();
    let prompt_mode = match builder.mode {
        PromptMode::Full => "full",
        PromptMode::Minimal => "minimal",
        PromptMode::None => "none",
    };
    let environment_summary = builder
        .session_environment
        .as_ref()
        .map(|environment| {
            format!(
                "- session_directory: {}\n- connection_mode: {}",
                environment.directory.display(),
                environment.connection_mode
            )
        })
        .unwrap_or_else(|| "- session_directory: none\n- connection_mode: unknown".to_string());
    let project_root_summary = builder
        .project_root
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "unset".to_string());

    format!(
        r#"Runtime session context:
- phase: {phase}
- planner_mode: {planner_mode}
- prompt_mode: {prompt_mode}
- project_root: {project_root}
{environment_summary}"#,
        phase = phase_label,
        planner_mode = planner_mode,
        prompt_mode = prompt_mode,
        project_root = project_root_summary,
        environment_summary = environment_summary,
    )
}

fn build_evidence_tooling_policy_prompt() -> String {
    r#"Evidence and tooling policy:
- Treat source code, local files, command output, and tool results as primary evidence.
- Treat README files, architecture documents, and design notes as secondary evidence unless the user explicitly asks for documentation analysis.
- If the user asks for code-based analysis, first inspect code structure and only use documents as supplemental context.
- Prefer dedicated tools over shell commands for repository exploration.
- To search file contents or code symbols, use Grep instead of shell grep or rg.
- To read files, use Read instead of cat, head, tail, or ad hoc shell readers.
- To inspect directory or path shape, use Glob instead of shell ls or find.
- Use Grep to locate symbols, entrypoints, exports, routes, and module boundaries before reading files.
- Use Read only after the target file has been named by the user or narrowed by Grep or Glob.
- When a path might be a directory or you need to map a subtree, use Glob before calling Read.
- Reserve Bash for tests, builds, scripts, git, and shell-required operations. Do not use Bash as a substitute for Grep, Read, or Glob when dedicated tools can answer directly.
- Do not repeat an equivalent read-only tool call unless parameters materially narrow scope or verify changed state.
- Do not re-read the same file with the same arguments. If the earlier read was insufficient, change offset or limit and briefly explain why you need the second read.
- If a tool already provides enough evidence to answer, stop exploring and synthesize the answer."#
        .to_string()
}

fn build_tool_choice_priority_prompt() -> String {
    r#"Tool choice priority:
- If the user explicitly names a tool and that tool can satisfy the next verification step, honor that tool before switching to adjacent tools.
- For codebase analysis, prefer Grep first, then Read, then Glob.
- Use Glob first only when the next step is to confirm path shape, expand a subtree, or map an unfamiliar top-level layout.
- When analyzing architecture, ownership, exports, traits, routes, or entrypoints, start with Grep unless the exact file is already known.
- Use Bash only when shell execution itself is required.
- Do not replace Grep, Read, or Glob with shell grep, cat, find, or ls.
- Once enough evidence exists, stop exploring and answer."#
        .to_string()
}

fn build_failure_recovery_policy_prompt() -> String {
    r#"Failure recovery policy:
- Tool failures are evidence and must be reflected in the final answer.
- Do not stop after a failed tool call if existing evidence still supports a useful response.
- If the model cannot produce a valid final answer after tools, force a non-empty answer using available results.
- Any salvage pass must avoid reopening normal exploration unless explicitly required."#
        .to_string()
}

fn build_requested_tool_preference_template_prompt() -> String {
    r#"Requested tool preference policy:
- When the user explicitly names a tool and that tool can satisfy the next verification step, prefer it before adjacent tools.
- Ignore the requested tool only when it cannot answer the next step directly.
- After the requested tool narrows scope, switch to the next best dedicated tool instead of repeating low-value searches."#
        .to_string()
}

fn build_requested_tool_preference_hint_prompt(builder: &PromptBuilder<'_>) -> Option<String> {
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
        "Requested tool preference hint:\n- The user explicitly asked for these tools: {}.\n- Example: if the requested tool is Grep for code investigation, start with Grep rather than Glob when it can narrow the first files to inspect.",
        requested_tools.join(", "),
    ))
}

fn build_compact_tool_routing_prompt() -> String {
    r#"Compact tool routing:
- Grep: search file contents, symbols, exports, routes, and entrypoints; use this instead of shell grep or rg
- Read: read a specific file after narrowing; use this instead of cat, head, or tail
- Glob: inspect path shape, expand a subtree, or confirm matching files; use this instead of ls or find
- Bash: run tests, scripts, builds, git, or other shell-only tasks when dedicated tools do not fit"#
        .to_string()
}

fn build_task_protocol_library_prompt() -> String {
    r#"Task protocol library:
- Codebase analysis protocol:
  1. Start from code anchors such as workspace members, crate roots, exports, traits, entrypoints, and key module declarations.
  2. Prefer Grep before Read when you still need to narrow candidate files.
  3. If the user explicitly asks to use Grep, make Grep the first exploratory tool call unless the file path is already exact.
  4. After two to four structural Grep calls, switch to Read on Cargo.toml, crate root files, or the most relevant modules before opening broader searches.
  5. Read Cargo.toml, crate root files, and a small set of key modules before describing architecture.
  6. Use Glob before Grep only when path shape itself is the missing fact.
  7. Avoid repeated reads of the same file unless you need a different offset or limit to confirm a specific detail.
  8. Treat architecture documents as target design or background unless code confirms they are current.
  9. Cite concrete code evidence in the answer instead of giving a documentation-only summary.
  10. Stop once you can explain the actual implemented structure with enough evidence.
  11. Mark any extrapolation as inference instead of verified fact.
- Bug investigation protocol:
  1. Reproduce or localize the failure signal first.
  2. Narrow likely ownership with Grep, then inspect the smallest relevant files.
  3. Do not propose a fix until the failure path is evidenced.
- Change planning protocol:
  1. Inspect the current implementation before suggesting edits.
  2. Prefer existing abstractions and repository patterns over new layers.
  3. Call out missing information instead of inventing hidden requirements.
- Environment verification protocol:
  1. Use dedicated file or runtime tools to verify environment claims.
  2. Do not speculate about sandbox, mount, or path visibility without an observed tool result."#
        .to_string()
}

fn build_communication_policy_prompt() -> String {
    r#"Communication policy:
- All normal text is user-visible.
- Before the first tool call, say in one short sentence what you are about to check.
- During tool-assisted work, provide short progress updates only when you discover a load-bearing fact, change direction, or need to explain why you are doing the next step.
- Keep progress updates human-readable, concise, and free of log-like noise.
- If the user requires an exact word, phrase, tag, or output wrapper, include that literal text exactly as requested in the final answer.
- When the task is complete, end with a clear answer that distinguishes verified findings from inference.
- If evidence is incomplete, say what is known, what is uncertain, and what additional step would resolve it."#
        .to_string()
}

pub(crate) fn build_expanded_tool_catalog_prompt(builder: &PromptBuilder<'_>) -> Option<String> {
    let Some(tool_registry) = builder.tool_registry else {
        return None;
    };
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
        .collect::<Vec<_>>()
        .join("\n");
    if selection.deferred_specs().is_empty() {
        Some(format!("Available tools:\n{}", active_summary))
    } else {
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
            .collect::<Vec<_>>()
            .join("\n");
        Some(format!(
            "Available tools:\n{}\n\nSearchable deferred tools (discover with ToolSearch before using them in a later turn):\n{}",
            active_summary, deferred_summary
        ))
    }
}

fn build_static_prefix_sections(builder: &PromptBuilder<'_>) -> Vec<PromptSection> {
    let mut sections = vec![
        PromptSection {
            kind: PromptSectionKind::IdentityCore,
            title: Some("Identity core".to_string()),
            content: build_identity_core_prompt(builder),
            role: None,
            cache_scope: CacheScope::Static,
            tier: BudgetTier::Critical,
            required: true,
            estimated_tokens: 0,
            truncation_mode: TruncationMode::TrimTail,
        },
        PromptSection {
            kind: PromptSectionKind::EvidenceToolingPolicy,
            title: Some("Evidence and tooling policy".to_string()),
            content: build_evidence_tooling_policy_prompt(),
            role: None,
            cache_scope: CacheScope::Static,
            tier: BudgetTier::Critical,
            required: true,
            estimated_tokens: 0,
            truncation_mode: TruncationMode::TrimTail,
        },
        PromptSection {
            kind: PromptSectionKind::ToolChoicePriority,
            title: Some("Tool choice priority".to_string()),
            content: build_tool_choice_priority_prompt(),
            role: None,
            cache_scope: CacheScope::Static,
            tier: BudgetTier::Critical,
            required: true,
            estimated_tokens: 0,
            truncation_mode: TruncationMode::TrimTail,
        },
        PromptSection {
            kind: PromptSectionKind::RequestedToolPreferenceTemplate,
            title: Some("Requested tool preference policy".to_string()),
            content: build_requested_tool_preference_template_prompt(),
            role: None,
            cache_scope: CacheScope::Static,
            tier: BudgetTier::Critical,
            required: true,
            estimated_tokens: 0,
            truncation_mode: TruncationMode::TrimTail,
        },
        PromptSection {
            kind: PromptSectionKind::FailureRecoveryPolicy,
            title: Some("Failure recovery policy".to_string()),
            content: build_failure_recovery_policy_prompt(),
            role: None,
            cache_scope: CacheScope::Static,
            tier: BudgetTier::Critical,
            required: true,
            estimated_tokens: 0,
            truncation_mode: TruncationMode::TrimTail,
        },
        PromptSection {
            kind: PromptSectionKind::CompactToolRouting,
            title: Some("Compact tool routing".to_string()),
            content: build_compact_tool_routing_prompt(),
            role: None,
            cache_scope: CacheScope::Static,
            tier: BudgetTier::Critical,
            required: true,
            estimated_tokens: 0,
            truncation_mode: TruncationMode::TrimTail,
        },
        PromptSection {
            kind: PromptSectionKind::TaskProtocolLibrary,
            title: Some("Task protocol library".to_string()),
            content: build_task_protocol_library_prompt(),
            role: None,
            cache_scope: CacheScope::Static,
            tier: BudgetTier::Important,
            required: true,
            estimated_tokens: 0,
            truncation_mode: TruncationMode::TrimTail,
        },
        PromptSection {
            kind: PromptSectionKind::CommunicationPolicy,
            title: Some("Communication policy".to_string()),
            content: build_communication_policy_prompt(),
            role: None,
            cache_scope: CacheScope::Static,
            tier: BudgetTier::Supplemental,
            required: false,
            estimated_tokens: 0,
            truncation_mode: TruncationMode::TrimTail,
        },
    ];
    budget::enrich_sections(&mut sections);
    sections
}

fn build_recent_task_continuity_prompt(builder: &PromptBuilder<'_>) -> Option<String> {
    let continuity = builder.session_continuity.as_ref()?;
    let mut lines = Vec::new();

    if !continuity.recent_verified_paths.is_empty() {
        lines.push(format!(
            "- recent_verified_paths: {}",
            continuity.recent_verified_paths.join(", ")
        ));
    }
    if !continuity.recent_successful_tools.is_empty() {
        lines.push(format!(
            "- recent_successful_tools: {}",
            continuity.recent_successful_tools.join(", ")
        ));
    }
    if !continuity.recent_failed_tools.is_empty() {
        lines.push(format!(
            "- recent_failed_tools: {}",
            continuity.recent_failed_tools.join(", ")
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
            "- current_evidence_mode: {}",
            match mode {
                ContinuityAnswerMode::CodeFirst => "code_first",
                ContinuityAnswerMode::DocumentationFirst => "documentation_first",
                ContinuityAnswerMode::DirectAnswer => "direct_answer",
                ContinuityAnswerMode::Mixed => "mixed",
            }
        ));
    }

    if lines.is_empty() {
        None
    } else {
        Some(format!("Recent task continuity:\n{}", lines.join("\n")))
    }
}

pub(crate) async fn assemble(builder: &PromptBuilder<'_>) -> TalosResult<PromptAssembly> {
    let user_input = builder.user_input.clone().ok_or_else(|| {
        TalosError::ValidationError("PromptBuilder requires user input".to_string())
    })?;

    let static_prefix_sections = build_static_prefix_sections(builder);
    let mut dropped_sections = Vec::new();
    let truncated_sections = Vec::new();

    let (mut dynamic_suffix_sections, bootstrap_diagnostics) =
        load_dynamic_sections(builder).await?;
    budget::enrich_sections(&mut dynamic_suffix_sections);
    let static_prefix_tokens = budget::estimate_sections_tokens(&static_prefix_sections);
    let mut window_usage = resolve_window_usage(
        builder,
        static_prefix_tokens,
        budget::estimate_sections_tokens(&dynamic_suffix_sections),
    );
    if window_usage.window_resolved && !window_usage.within_effective_budget {
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

    let diagnostics = build_diagnostics(
        builder.mode,
        &static_prefix_sections,
        &dynamic_suffix_sections,
        bootstrap_diagnostics,
        dropped_sections,
        truncated_sections,
        window_usage,
    );

    Ok(PromptAssembly {
        static_prefix_sections,
        dynamic_suffix_sections,
        user_input,
        diagnostics,
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
            .map(|model| model.context_window as usize)
            .unwrap_or_default(),
        builder
            .model_info
            .as_ref()
            .map(|model| model.max_tokens as usize)
            .unwrap_or_default(),
        static_prefix_tokens,
        dynamic_suffix_tokens,
    )
}

fn format_bootstrap_heading(file: &WorkspaceBootstrapFile) -> String {
    if matches!(file.scope, BootstrapFileScope::ProjectRoot) {
        format!("## {} (project root)", file.kind.file_name())
    } else {
        format!("## {} (workspace)", file.kind.file_name())
    }
}

pub(crate) fn build_project_context_section(
    builder: &PromptBuilder<'_>,
    files: &[WorkspaceBootstrapFile],
) -> Option<(PromptSection, Vec<BootstrapFileDiagnostic>)> {
    if builder.mode == PromptMode::None || !builder.config.prompt.bootstrap.enabled {
        return None;
    }
    let per_file_limit = builder.config.prompt.bootstrap.max_chars_per_file.max(1);
    let total_limit = builder.config.prompt.bootstrap.max_chars_total.max(1);
    let mut total_chars = 0_usize;
    let mut sections = Vec::new();
    let mut diagnostics = Vec::new();

    for file in files {
        let Some(content) = file.content.as_ref() else {
            diagnostics.push(BootstrapFileDiagnostic {
                kind: file.kind,
                path: file.path.clone(),
                raw_chars: 0,
                injected_chars: 0,
                truncated: false,
                missing: true,
            });
            continue;
        };
        let trimmed = content.trim();
        if trimmed.is_empty() {
            diagnostics.push(BootstrapFileDiagnostic {
                kind: file.kind,
                path: file.path.clone(),
                raw_chars: 0,
                injected_chars: 0,
                truncated: false,
                missing: false,
            });
            continue;
        }
        let limited = bootstrap::truncate_chars(trimmed, per_file_limit);
        let injected_chars = limited.chars().count();
        let raw_chars = trimmed.chars().count();
        let file_block = format!("{}\n{}", format_bootstrap_heading(file), limited);
        let next_chars = file_block.chars().count() + 2;
        let accepted = total_chars + next_chars <= total_limit;
        diagnostics.push(BootstrapFileDiagnostic {
            kind: file.kind,
            path: file.path.clone(),
            raw_chars,
            injected_chars: if accepted { injected_chars } else { 0 },
            truncated: raw_chars != injected_chars || !accepted,
            missing: false,
        });
        if !accepted {
            break;
        }
        total_chars += next_chars;
        sections.push(file_block);
    }

    if sections.is_empty() {
        None
    } else {
        Some((
            PromptSection {
                kind: PromptSectionKind::ProjectContext,
                title: Some("Project context".to_string()),
                content: format!("# Project Context\n\n{}", sections.join("\n\n")),
                role: None,
                cache_scope: CacheScope::Workspace,
                tier: BudgetTier::Important,
                required: false,
                estimated_tokens: 0,
                truncation_mode: TruncationMode::TrimTail,
            },
            diagnostics,
        ))
    }
}

pub(crate) async fn load_dynamic_sections(
    builder: &PromptBuilder<'_>,
) -> TalosResult<(Vec<PromptSection>, Vec<BootstrapFileDiagnostic>)> {
    let mut sections = Vec::new();
    let include_summary = builder.config.load_policy.include_session_summary;
    let include_recent = builder.config.load_policy.include_recent_messages;
    let include_retrieved_memories = builder.config.load_policy.include_retrieved_memories;
    let summary_recent_count = builder.config.prompt.session_summary_recent_count;
    let summary_max_chars = builder.config.prompt.session_summary_max_chars;
    let context_target_messages = builder.config.load_policy.session_recent_limit;
    let retrieved_memory_max_items = builder.config.load_policy.retrieved_memory_max_items;
    let retrieved_memory_max_chars = builder.config.load_policy.retrieved_memory_max_chars;
    let mut bootstrap_files = if builder.memory_store.is_some() {
        builder.load_workspace_bootstrap_files().await?
    } else {
        Vec::new()
    };
    bootstrap_files.extend(builder.load_project_bootstrap_files().await?);
    let mut bootstrap_diagnostics = Vec::new();

    sections.push(PromptSection {
        kind: PromptSectionKind::RuntimeSessionContext,
        title: Some("Runtime session context".to_string()),
        content: build_runtime_session_context_prompt(builder),
        role: None,
        cache_scope: CacheScope::None,
        tier: BudgetTier::Important,
        required: false,
        estimated_tokens: 0,
        truncation_mode: TruncationMode::TrimTail,
    });

    if let Some(requested_tool_preference_hint) =
        build_requested_tool_preference_hint_prompt(builder)
    {
        sections.push(PromptSection {
            kind: PromptSectionKind::RequestedToolPreferenceHint,
            title: Some("Requested tool preference hint".to_string()),
            content: requested_tool_preference_hint,
            role: None,
            cache_scope: CacheScope::None,
            tier: BudgetTier::Important,
            required: false,
            estimated_tokens: 0,
            truncation_mode: TruncationMode::DropWhole,
        });
    }

    if let Some(planning_instruction) = builder.planning_instruction.as_deref() {
        if !planning_instruction.trim().is_empty() {
            sections.push(PromptSection {
                kind: PromptSectionKind::PlanningInstruction,
                title: Some("Planning instruction".to_string()),
                content: format!("Planning instruction:\n{}", planning_instruction.trim()),
                role: None,
                cache_scope: CacheScope::None,
                tier: BudgetTier::Critical,
                required: true,
                estimated_tokens: 0,
                truncation_mode: TruncationMode::TrimTail,
            });
        }
    }

    if let Some(continuity_prompt) = build_recent_task_continuity_prompt(builder) {
        sections.push(PromptSection {
            kind: PromptSectionKind::RecentTaskContinuity,
            title: Some("Recent task continuity".to_string()),
            content: continuity_prompt,
            role: None,
            cache_scope: CacheScope::None,
            tier: BudgetTier::Important,
            required: false,
            estimated_tokens: 0,
            truncation_mode: TruncationMode::DropWhole,
        });
    }

    if let Some(tooling_prompt) = build_expanded_tool_catalog_prompt(builder) {
        sections.push(PromptSection {
            kind: PromptSectionKind::ExpandedToolCatalog,
            title: Some("Expanded tool catalog".to_string()),
            content: tooling_prompt,
            role: None,
            cache_scope: CacheScope::None,
            tier: BudgetTier::Important,
            required: false,
            estimated_tokens: 0,
            truncation_mode: TruncationMode::DropWhole,
        });
    }

    if let Some((project_context, diagnostics)) =
        build_project_context_section(builder, &bootstrap_files)
    {
        bootstrap_diagnostics.extend(diagnostics);
        sections.push(project_context);
    }

    if let (Some(memory_store), Some(session_id)) = (builder.memory_store, builder.session_id) {
        let context = memory_store
            .build_session_prompt_context(
                session_id,
                &SessionPromptContextConfig {
                    recent_messages_limit: if include_recent {
                        context_target_messages
                    } else {
                        0
                    },
                    include_summary,
                    summary_recent_count,
                    summary_max_chars,
                },
            )
            .await?;
        if include_summary {
            for summary in context.summaries {
                let content = summary.content.trim().to_string();
                if !content.is_empty() {
                    sections.push(PromptSection {
                        kind: PromptSectionKind::SessionSummary,
                        title: Some(format!("Session summary {}", summary.header.id)),
                        content: format!(
                            "Session memory summary (id={}):\n{}",
                            summary.header.id, content
                        ),
                        role: None,
                        cache_scope: CacheScope::None,
                        tier: BudgetTier::Important,
                        required: false,
                        estimated_tokens: 0,
                        truncation_mode: TruncationMode::TrimTail,
                    });
                }
            }
        }
        if include_recent {
            for message in context.recent_messages {
                sections.push(PromptSection {
                    kind: PromptSectionKind::RecentMessages,
                    title: Some("Recent message".to_string()),
                    content: message.text,
                    role: Some(message.role),
                    cache_scope: CacheScope::None,
                    tier: BudgetTier::Supplemental,
                    required: false,
                    estimated_tokens: 0,
                    truncation_mode: TruncationMode::DropWhole,
                });
            }
        }
        if include_retrieved_memories && !builder.retrieved_memories.is_empty() {
            let mut combined = String::new();
            for item in builder
                .retrieved_memories
                .iter()
                .take(retrieved_memory_max_items.max(1))
            {
                let line = format!("- {}: {}", item.path, item.snippet.trim());
                if combined.len() + line.len() + 1 > retrieved_memory_max_chars.max(1) {
                    break;
                }
                if !combined.is_empty() {
                    combined.push('\n');
                }
                combined.push_str(&line);
            }
            if !combined.is_empty() {
                sections.push(PromptSection {
                    kind: PromptSectionKind::RetrievedMemories,
                    title: Some("Retrieved memories".to_string()),
                    content: format!("Retrieved memories:\n{}", combined),
                    role: None,
                    cache_scope: CacheScope::None,
                    tier: BudgetTier::Supplemental,
                    required: false,
                    estimated_tokens: 0,
                    truncation_mode: TruncationMode::DropWhole,
                });
            }
        }
    }

    Ok((sections, bootstrap_diagnostics))
}

pub(crate) fn build_diagnostics(
    mode: PromptMode,
    static_sections: &[PromptSection],
    dynamic_sections: &[PromptSection],
    bootstrap_diagnostics: Vec<crate::BootstrapFileDiagnostic>,
    dropped_sections: Vec<PromptSectionKind>,
    truncated_sections: Vec<PromptSectionKind>,
    window_usage: PromptWindowUsage,
) -> PromptDiagnostics {
    diagnostics::build_prompt_diagnostics(
        mode,
        static_sections,
        dynamic_sections,
        bootstrap_diagnostics,
        dropped_sections,
        truncated_sections,
        window_usage,
    )
}
