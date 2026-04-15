use crate::{
    bootstrap, budget, diagnostics, BootstrapFileDiagnostic, BudgetTier, CacheScope,
    PromptAssembly, PromptBuilder, PromptDiagnostics, PromptMode, PromptPhase, PromptSection,
    PromptSectionKind, TruncationMode, WorkspaceBootstrapFile,
};
use talos_error::{TalosError, TalosResult};
use talos_memory::{Memory, SessionPromptContextConfig};

fn build_identity_mission_prompt(builder: &PromptBuilder<'_>) -> String {
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
        r#"Identity and mission:
- You are {name}, {description}.
- Operate as a long-running intelligent steward, not a one-shot chatbot.
- Move the task forward with minimal user supervision while staying evidence-driven.
- Prefer a small number of high-value tool calls over broad or repetitive exploration.
- Separate verified facts from inference and say when evidence is still incomplete.
- Finish with a concrete result, or clearly state the remaining blocker.

Runtime context:
- phase: {phase}
- planner_mode: {planner_mode}
- prompt_mode: {prompt_mode}
- project_root: {project_root}
{environment_summary}"#,
        name = builder.agent_name,
        description = builder.agent_description,
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
- Use grep_files to locate symbols, entrypoints, exports, routes, and module boundaries before reading files.
- Use read_file only after the target file has been named by the user or narrowed by grep_files or list_dir.
- When a path might be a directory, verify it with list_dir before calling read_file.
- Use list_dir to map unfamiliar directories or confirm high-level structure.
- Do not use exec_command as a substitute for grep_files, read_file, or list_dir when those tools can answer directly.
- Do not repeat an equivalent read-only tool call unless parameters materially narrow scope or verify changed state.
- Do not re-read the same file with the same arguments. If the earlier read was insufficient, change offset or limit and briefly explain why you need the second read.
- If a tool already provides enough evidence to answer, stop exploring and synthesize the answer."#
        .to_string()
}

fn build_task_protocol_library_prompt() -> String {
    r#"Task protocol library:
- Codebase analysis protocol:
  1. Start from code anchors such as workspace members, crate roots, exports, traits, entrypoints, and key module declarations.
  2. Prefer grep_files before read_file when you still need to narrow candidate files.
  3. Read Cargo.toml, crate root files, and a small set of key modules before describing architecture.
  4. Avoid repeated reads of the same file unless you need a different offset or limit to confirm a specific detail.
  5. Treat architecture documents as target design or background unless code confirms they are current.
  6. Cite concrete code evidence in the answer instead of giving a documentation-only summary.
  7. Stop once you can explain the actual implemented structure with enough evidence.
  8. Mark any extrapolation as inference instead of verified fact.
- Bug investigation protocol:
  1. Reproduce or localize the failure signal first.
  2. Narrow likely ownership with grep_files, then inspect the smallest relevant files.
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

pub(crate) fn build_tooling_prompt(builder: &PromptBuilder<'_>) -> Option<String> {
    let Some(tool_registry) = builder.tool_registry else {
        return None;
    };
    if tool_registry.get_tool_definitions().is_empty() {
        return None;
    }

    let tool_summary = PromptBuilder::render_tool_summary(tool_registry);
    Some(format!("Available tools:\n{}", tool_summary))
}

pub(crate) async fn assemble(builder: &PromptBuilder<'_>) -> TalosResult<PromptAssembly> {
    let user_input = builder.user_input.clone().ok_or_else(|| {
        TalosError::ValidationError("PromptBuilder requires user input".to_string())
    })?;

    let mut static_sections = vec![
        PromptSection {
            kind: PromptSectionKind::IdentityMission,
            title: Some("Identity and mission".to_string()),
            content: build_identity_mission_prompt(builder),
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
            tier: BudgetTier::Important,
            required: true,
            estimated_tokens: 0,
            truncation_mode: TruncationMode::TrimTail,
        },
    ];
    if let Some(tooling_prompt) = build_tooling_prompt(builder) {
        static_sections.push(PromptSection {
            kind: PromptSectionKind::Tooling,
            title: Some("Tooling".to_string()),
            content: tooling_prompt,
            role: None,
            cache_scope: CacheScope::Static,
            tier: BudgetTier::Important,
            required: true,
            estimated_tokens: 0,
            truncation_mode: TruncationMode::TrimTail,
        });
    }
    if let Some(planning_instruction) = builder.planning_instruction.as_deref() {
        if !planning_instruction.trim().is_empty() {
            static_sections.push(PromptSection {
                kind: PromptSectionKind::PlanningInstruction,
                title: Some("Planning instruction".to_string()),
                content: format!("Planning instruction:\n{}", planning_instruction.trim()),
                role: None,
                cache_scope: CacheScope::Static,
                tier: BudgetTier::Important,
                required: false,
                estimated_tokens: 0,
                truncation_mode: TruncationMode::TrimTail,
            });
        }
    }
    budget::enrich_sections(&mut static_sections);
    let mut dropped_sections = Vec::new();
    let mut truncated_sections = Vec::new();
    let static_budget = builder.config.budget.max_static_tokens.max(1);
    budget::truncate_sections_to_budget(
        &mut static_sections,
        static_budget,
        true,
        &mut dropped_sections,
        &mut truncated_sections,
    );

    let (mut dynamic_sections, bootstrap_diagnostics) = load_dynamic_sections(builder).await?;
    budget::enrich_sections(&mut dynamic_sections);
    let dynamic_budget = builder.config.budget.max_dynamic_tokens.max(1);
    budget::truncate_sections_to_budget(
        &mut dynamic_sections,
        dynamic_budget,
        false,
        &mut dropped_sections,
        &mut truncated_sections,
    );

    let diagnostics = build_diagnostics(
        builder.mode,
        &static_sections,
        &dynamic_sections,
        bootstrap_diagnostics,
        dropped_sections,
        truncated_sections,
    );

    Ok(PromptAssembly {
        system_sections: static_sections,
        context_sections: dynamic_sections,
        user_input,
        diagnostics,
    })
}

fn format_bootstrap_heading(file: &WorkspaceBootstrapFile) -> String {
    if file.kind.is_project_file() {
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
    let summary_recent_count = builder.config.memory.session_summary_recent_count;
    let summary_max_chars = builder.config.memory.session_summary_max_chars;
    let context_target_messages = builder.config.load_policy.session_recent_limit;
    let memory_enable_summary = builder.config.memory.session_summary_enabled;
    let retrieved_memory_max_items = builder.config.load_policy.retrieved_memory_max_items;
    let retrieved_memory_max_chars = builder.config.load_policy.retrieved_memory_max_chars;
    let mut bootstrap_files = if builder.memory_store.is_some() {
        builder.load_workspace_bootstrap_files().await?
    } else {
        Vec::new()
    };
    bootstrap_files.extend(builder.load_project_bootstrap_files().await?);
    let mut bootstrap_diagnostics = Vec::new();

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
                    include_summary: include_summary && memory_enable_summary,
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
) -> PromptDiagnostics {
    diagnostics::build_prompt_diagnostics(
        mode,
        static_sections,
        dynamic_sections,
        bootstrap_diagnostics,
        dropped_sections,
        truncated_sections,
    )
}
