use crate::{
    bootstrap, budget, diagnostics, BootstrapFileDiagnostic, BudgetTier, PromptAssembly,
    PromptBuilder, PromptDiagnostics, PromptMode, PromptPhase, PromptSection, PromptSectionKind,
    TruncationMode, WorkspaceBootstrapFile, WorkspaceBootstrapFileKind,
};
use talos_error::{TalosError, TalosResult};
use talos_memory::{Memory, SessionPromptContextConfig};

pub(crate) fn build_identity_prompt(builder: &PromptBuilder<'_>) -> String {
    let phase_label = match builder.phase {
        PromptPhase::Planning => "planning",
        PromptPhase::Execution => "execution",
    };
    let phase_instruction = match builder.phase {
        PromptPhase::Planning => {
            "Focus on clear decomposition, constraints, and an executable next-step plan."
        }
        PromptPhase::Execution => {
            "Use native tool calling when needed, then produce the final response."
        }
    };
    let planner_mode = builder.planner_mode.as_str();
    let prompt_mode = match builder.mode {
        PromptMode::Full => "full",
        PromptMode::Minimal => "minimal",
        PromptMode::None => "none",
    };

    let tool_summary = builder
        .tool_registry
        .map(PromptBuilder::render_tool_summary)
        .unwrap_or_else(|| "none".to_string());

    format!(
        r#"You are {}, {}

Phase: {}
Planner mode: {}
Prompt mode: {}
{}

Available tools:
{}"#,
        builder.agent_name,
        builder.agent_description,
        phase_label,
        planner_mode,
        prompt_mode,
        phase_instruction,
        tool_summary
    )
}

pub(crate) async fn assemble(builder: &PromptBuilder<'_>) -> TalosResult<PromptAssembly> {
    let user_input = builder.user_input.clone().ok_or_else(|| {
        TalosError::ValidationError("PromptBuilder requires user input".to_string())
    })?;

    let mut static_sections = vec![PromptSection {
        kind: PromptSectionKind::Identity,
        title: Some("Identity".to_string()),
        content: build_identity_prompt(builder),
        role: None,
        tier: BudgetTier::Critical,
        required: true,
        estimated_tokens: 0,
        truncation_mode: TruncationMode::TrimTail,
    }];
    if let Some(planning_instruction) = builder.planning_instruction.as_deref() {
        if !planning_instruction.trim().is_empty() {
            static_sections.push(PromptSection {
                kind: PromptSectionKind::PlanningInstruction,
                title: Some("Planning instruction".to_string()),
                content: format!("Planning instruction:\n{}", planning_instruction.trim()),
                role: None,
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

    for file in files.iter().filter(|file| file.kind.is_project_context()) {
        let Some(content) = file.content.as_ref() else {
            diagnostics.push(BootstrapFileDiagnostic {
                kind: file.kind,
                path: file.path.clone(),
                raw_chars: 0,
                injected_chars: 0,
                truncated: false,
                missing: file.missing,
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
        let file_block = format!("## {}\n{}", file.kind.file_name(), limited);
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
                title: Some("Project Context".to_string()),
                content: format!("# Project Context\n\n{}", sections.join("\n\n")),
                role: None,
                tier: BudgetTier::Critical,
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
    let include_root_memory = builder.config.load_policy.include_root_memory;
    let include_retrieved_memories = builder.config.load_policy.include_retrieved_memories;
    let summary_recent_count = builder.config.memory.session_summary_recent_count;
    let summary_max_chars = builder.config.memory.session_summary_max_chars;
    let context_target_messages = builder.config.load_policy.session_recent_limit;
    let memory_enable_summary = builder.config.memory.session_summary_enabled;
    let retrieved_memory_max_items = builder.config.load_policy.retrieved_memory_max_items;
    let retrieved_memory_max_chars = builder.config.load_policy.retrieved_memory_max_chars;
    let bootstrap_files = if builder.memory_store.is_some() {
        builder.load_workspace_bootstrap_files().await?
    } else {
        Vec::new()
    };
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
                    tier: BudgetTier::Supplemental,
                    required: false,
                    estimated_tokens: 0,
                    truncation_mode: TruncationMode::DropWhole,
                });
            }
        }
        if include_root_memory {
            let bootstrap_memory = bootstrap_files
                .iter()
                .find(|file| file.kind == WorkspaceBootstrapFileKind::Memory)
                .and_then(|file| file.content.as_ref())
                .cloned();
            let root_memory_content = match bootstrap_memory {
                Some(content) => Some(content),
                None => memory_store.load_root_memory().await?.content,
            };
            if let Some(content) = root_memory_content {
                let trimmed = content.trim();
                if !trimmed.is_empty() {
                    sections.push(PromptSection {
                        kind: PromptSectionKind::DurableMemory,
                        title: Some("Durable memory".to_string()),
                        content: format!("Durable memory (MEMORY.md):\n{}", trimmed),
                        role: None,
                        tier: BudgetTier::Important,
                        required: false,
                        estimated_tokens: 0,
                        truncation_mode: TruncationMode::TrimTail,
                    });
                }
            }
        }
        if include_retrieved_memories && !builder.retrieved_memories.is_empty() {
            let mut combined = String::new();
            for item in builder
                .retrieved_memories
                .iter()
                .take(retrieved_memory_max_items.max(1))
            {
                let line = format!(
                    "- {}{}: {}",
                    item.path,
                    item.citation
                        .as_ref()
                        .map(|citation| format!(" ({citation})"))
                        .unwrap_or_default(),
                    item.snippet.trim()
                );
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
