use super::*;
use crate::general_turn::types::SessionCompactionStatus;
use crate::StateNamespace;
use talos_prompt::{
    CompactionTrimUnitKind, PromptSectionKind, SessionHistoryCompactionOutcome,
    SessionHistoryCompactionTrigger,
};
use talos_skills::SkillSelector;

#[tokio::test]
async fn test_prepare_general_turn_injects_selected_skill_prompt_context() {
    let provider = Arc::new(NoopLlmProvider);
    let tool_registry = Arc::new(ToolRegistry::new());
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
    let memory_store = Arc::new(MemoryStore::new(workspace_dir.clone()));
    let skill_root = workspace_dir.join("skills").join("review");
    std::fs::create_dir_all(&skill_root).expect("skill root should exist");
    std::fs::write(
        skill_root.join("SKILL.md"),
        "---\nname: review\ndescription: Review code changes.\n---\n\n# Review\n\nUse verified repository evidence.",
    )
    .expect("skill document should exist");
    let mut config = AgentKernelConfig::default();
    config.plugins.roots = vec![talos_config::PluginRootConfig {
        kind: talos_config::PluginRootKind::Skill,
        path: workspace_dir.join("skills").to_string_lossy().to_string(),
        read_only: true,
    }];
    let mut ctx = build_ctx_with_stream(None);

    let prepared = prepare_general_turn(
        &mut ctx,
        "talos",
        "assistant",
        "Use $review",
        provider,
        tool_registry.clone(),
        memory_store,
        &config,
        Some(skill_options_from_config_with_selected(
            &workspace_dir,
            &config,
            vec![SkillSelector::selection(
                "review",
                skill_root.join("SKILL.md"),
            )],
        )),
    )
    .await
    .expect("turn should prepare with selected skill");

    assert!(prepared
        .prompt_diagnostics
        .included_sections
        .contains(&PromptSectionKind::SkillCatalog));
    assert!(prepared
        .prompt_diagnostics
        .included_sections
        .contains(&PromptSectionKind::ActiveSkillInstructions));
    let rendered_prompt = prepared
        .messages
        .iter()
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered_prompt.contains("&lt;skill name=\"review\""));
    assert!(rendered_prompt.contains("Use verified repository evidence."));
}

#[tokio::test]
async fn test_prepare_general_turn_uses_configured_skill_prompt_budget() {
    let provider = Arc::new(NoopLlmProvider);
    let tool_registry = Arc::new(ToolRegistry::new());
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
    let memory_store = Arc::new(MemoryStore::new(workspace_dir.clone()));
    let skills_root = workspace_dir.join("skills");
    for name in ["review", "docs", "design", "testing"] {
        let skill_root = skills_root.join(name);
        std::fs::create_dir_all(&skill_root).expect("skill root should exist");
        std::fs::write(
            skill_root.join("SKILL.md"),
            format!(
                "---\nname: {name}\ndescription: This is a long skill description for budget testing.\n---\n\n# {name}\n"
            ),
        )
        .expect("skill document should exist");
    }
    let mut config = AgentKernelConfig::default();
    config.skills.max_available_summary_bytes = 96;
    config.plugins.roots = vec![talos_config::PluginRootConfig {
        kind: talos_config::PluginRootKind::Skill,
        path: skills_root.to_string_lossy().to_string(),
        read_only: true,
    }];
    let mut ctx = build_ctx_with_stream(None);

    let prepared = prepare_general_turn(
        &mut ctx,
        "talos",
        "assistant",
        "Which skills are available?",
        provider,
        tool_registry.clone(),
        memory_store,
        &config,
        Some(skill_options_from_config(&workspace_dir, &config)),
    )
    .await
    .expect("turn should prepare with configured skill budget");

    let rendered_prompt = prepared
        .messages
        .iter()
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered_prompt.contains("Available skills:"));
    assert!(rendered_prompt.contains("- ..."));
}

#[tokio::test]
async fn test_prepare_general_turn_compacts_session_history_when_prompt_exceeds_budget() {
    let provider = Arc::new(SummaryProvider {
        responses: std::sync::Mutex::new(std::collections::VecDeque::from(vec![
            "{\"title\":\"Prompt Compact\",\"facts\":[\"history condensed\"],\"open_tasks\":[\"continue the current turn\"]}".to_string(),
            "{\"decision\":\"session_only\",\"rationale\":\"near-term continuity only\"}"
                .to_string(),
        ])),
    });
    let tool_registry = Arc::new(ToolRegistry::new());
    register_core_local_tools(tool_registry.as_ref());
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
    let memory_store = Arc::new(MemoryStore::new(workspace_dir.clone()));
    for idx in 0..24 {
        let timestamp = Utc::now();
        memory_store
            .append_turn(&TalosTurn {
                turn_id: format!("turn-{idx}"),
            message_id: "msg-1".to_string(),
                session_id: "s1".to_string(),
                seq: memory_store
                    .next_turn_seq("s1")
                    .await
                    .expect("seq should load"),
                timestamp,
                items: vec![
                    TalosOutputItem::UserInput {
                        text: format!(
                            "historical user message {} requesting additional repository evidence and continuity details",
                            idx
                        ),
                    },
                    TalosOutputItem::OutputText {
                        text: format!(
                            "historical assistant message {} with enough content to stress the prompt window and preserve a larger review trail across repeated follow-up turns",
                            idx
                        ),
                    },
                ],
                usage: TalosUsage::default(),
                stop_reason: TalosApiStopReason::EndTurn,
            })
            .await
            .expect("append should succeed");
    }

    let mut config = AgentKernelConfig::default();
    config.workspace_dir = workspace_dir.to_string_lossy().to_string();
    configure_test_skill_root(&mut config, &workspace_dir);
    config.prompt.bootstrap.enabled = false;
    config.prompt.window.compaction_buffer_tokens = 32;
    config.prompt.window.reserved_output_tokens =
        talos_config::ReservedOutputPolicy::Fixed { tokens: 128 };
    config.output.max_tokens = 1_234;
    config.output.upper_limit = 2_000;
    config.compaction.recent_messages_keep = 24;

    let mut ctx = build_ctx_with_stream(None);
    ctx.selected_model_info.context_window = 3_000;

    let prepared = prepare_general_turn(
        &mut ctx,
        "talos",
        "assistant",
        "continue",
        provider,
        tool_registry,
        memory_store.clone(),
        &config,
        Some(skill_options_from_config(&workspace_dir, &config)),
    )
    .await
    .expect("prompt prepare should succeed");

    assert_eq!(prepared.max_output_tokens, Some(1_234));

    let compaction_state_value = ctx
        .session
        .state
        .get(StateNamespace::Session, SESSION_COMPACTION_STATE_KEY)
        .cloned()
        .expect("compaction state should be staged during prepare");
    let compaction_state: SessionCompactionState =
        serde_json::from_value(compaction_state_value).expect("compaction state should parse");
    assert_eq!(
        compaction_state.status,
        SessionCompactionStatus::Compressing
    );

    assert!(
        prepared.prompt_diagnostics.dropped_sections.is_empty(),
        "history compaction should run before section drop"
    );
    assert!(
        prepared.warnings.is_empty(),
        "successful compaction should avoid prompt drop warnings"
    );
    let compaction = prepared
        .prompt_diagnostics
        .session_history_compaction
        .as_ref()
        .expect("session history compaction should be recorded");
    assert_eq!(
        compaction.trigger,
        SessionHistoryCompactionTrigger::BudgetExceeded
    );
    assert_eq!(compaction.outcome, SessionHistoryCompactionOutcome::Applied);
    assert!(compaction.summary_sections_after > 0);
    assert_eq!(compaction.compaction_request_count, 2);
    assert_eq!(compaction.compaction_input_tokens, 26);
    assert_eq!(compaction.compaction_output_tokens, 14);
    assert_eq!(compaction.compaction_total_tokens, 40);
    assert!(compaction.trimmed_context_unit_count > 0);
    assert_eq!(
        compaction.trimmed_context_unit_kind,
        Some(CompactionTrimUnitKind::OldestHistoryItem)
    );
    assert!(!compaction.fallback_section_drop_applied);
    assert_eq!(compaction.compaction_cache_creation_input_tokens, Some(6));
    assert_eq!(compaction.compaction_cache_read_input_tokens, Some(22));
    assert!(
        prepared
            .prompt_diagnostics
            .window_usage
            .within_effective_budget,
        "compaction should restore the prompt to the effective budget"
    );
    assert!(prepared
        .prompt_diagnostics
        .included_sections
        .contains(&PromptSectionKind::SessionSummary));

    let summaries = memory_store
        .load_summaries("s1", 4)
        .await
        .expect("summary load should succeed");
    assert!(
        !summaries.is_empty(),
        "compaction should persist a session summary"
    );
}

#[tokio::test]
async fn test_prepare_general_turn_compacts_session_history_when_prompt_reaches_trigger_ratio() {
    let provider = Arc::new(SummaryProvider {
        responses: std::sync::Mutex::new(std::collections::VecDeque::from(vec![
            "{\"title\":\"Prompt Compact\",\"facts\":[\"history condensed\"],\"open_tasks\":[\"continue the current turn\"]}".to_string(),
            "{\"decision\":\"session_only\",\"rationale\":\"near-term continuity only\"}"
                .to_string(),
        ])),
    });
    let tool_registry = Arc::new(ToolRegistry::new());
    register_core_local_tools(tool_registry.as_ref());
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
    let memory_store = Arc::new(MemoryStore::new(workspace_dir.clone()));
    for idx in 0..4 {
        let timestamp = Utc::now();
        memory_store
            .append_turn(&TalosTurn {
                turn_id: format!("turn-{idx}"),
            message_id: "msg-1".to_string(),
                session_id: "s1".to_string(),
                seq: memory_store
                    .next_turn_seq("s1")
                    .await
                    .expect("seq should load"),
                timestamp,
                items: vec![
                    TalosOutputItem::UserInput {
                        text: format!("historical user message {idx} with continuity"),
                    },
                    TalosOutputItem::OutputText {
                        text: format!(
                            "historical assistant message {idx} with enough content to cross the preemptive compaction trigger without exhausting the full prompt budget"
                        ),
                    },
                ],
                usage: TalosUsage::default(),
                stop_reason: TalosApiStopReason::EndTurn,
            })
            .await
            .expect("append should succeed");
    }

    let mut config = AgentKernelConfig::default();
    config.workspace_dir = workspace_dir.to_string_lossy().to_string();
    configure_test_skill_root(&mut config, &workspace_dir);
    config.prompt.bootstrap.enabled = false;
    config.prompt.window.compaction_buffer_tokens = 32;
    config.prompt.window.reserved_output_tokens =
        talos_config::ReservedOutputPolicy::Fixed { tokens: 128 };
    config.compaction.recent_messages_keep = 8;
    config.compaction.trigger_ratio_percent = 1;
    config.compaction.hard_ratio_percent = 100;

    let mut ctx = build_ctx_with_stream(None);
    ctx.selected_model_info.context_window = 6_000;

    let prepared = prepare_general_turn(
        &mut ctx,
        "talos",
        "assistant",
        "continue",
        provider,
        tool_registry,
        memory_store,
        &config,
        Some(skill_options_from_config(&workspace_dir, &config)),
    )
    .await
    .expect("prompt prepare should succeed");

    let compaction = prepared
        .prompt_diagnostics
        .session_history_compaction
        .as_ref()
        .expect("threshold-based compaction should be recorded");
    assert_eq!(
        compaction.trigger,
        SessionHistoryCompactionTrigger::ThresholdExceeded
    );
    assert_eq!(compaction.outcome, SessionHistoryCompactionOutcome::Applied);
    assert!(
        prepared
            .prompt_diagnostics
            .window_usage
            .within_effective_budget
    );
}

#[tokio::test]
async fn test_run_general_turn_updates_compaction_state_to_recovery_after_success() {
    let provider = Arc::new(SummaryProvider::new(vec![
        "{\"title\":\"Prompt Compact\",\"facts\":[\"history condensed\"],\"open_tasks\":[\"continue the current turn\"]}".to_string(),
        "{\"decision\":\"session_only\",\"rationale\":\"near-term continuity only\"}"
            .to_string(),
        "ok".to_string(),
        "ok".to_string(),
    ]));
    let tool_registry = Arc::new(ToolRegistry::new());
    register_core_local_tools(tool_registry.as_ref());
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
    let memory_store = Arc::new(MemoryStore::new(workspace_dir.clone()));
    for idx in 0..24 {
        let timestamp = Utc::now();
        memory_store
            .append_turn(&TalosTurn {
                turn_id: format!("turn-{idx}"),
            message_id: "msg-1".to_string(),
                session_id: "s1".to_string(),
                seq: memory_store
                    .next_turn_seq("s1")
                    .await
                    .expect("seq should load"),
                timestamp,
                items: vec![
                    TalosOutputItem::UserInput {
                        text: format!("historical user message {}", idx),
                    },
                    TalosOutputItem::OutputText {
                        text: format!(
                            "historical assistant message {} with enough content to stress the prompt window and preserve continuity",
                            idx
                        ),
                    },
                ],
                usage: TalosUsage::default(),
                stop_reason: TalosApiStopReason::EndTurn,
            })
            .await
            .expect("append should succeed");
    }

    let mut config = AgentKernelConfig::default();
    config.workspace_dir = workspace_dir.to_string_lossy().to_string();
    configure_test_skill_root(&mut config, &workspace_dir);
    config.prompt.bootstrap.enabled = false;
    config.prompt.window.compaction_buffer_tokens = 32;
    config.prompt.window.reserved_output_tokens =
        talos_config::ReservedOutputPolicy::Fixed { tokens: 128 };
    config.compaction.recent_messages_keep = 24;

    let mut ctx = build_ctx_with_stream(None);
    ctx.selected_model_info.context_window = 2_400;

    let result = run_general_turn(
        &mut ctx,
        "talos",
        "assistant",
        "continue",
        provider,
        tool_registry.clone(),
        tool_registry,
        memory_store,
        &config,
        skill_options_from_config(&workspace_dir, &config),
    )
    .await
    .expect("general turn should succeed");

    assert_eq!(result.output_text, "ok");
    let compaction_state_value = ctx
        .session
        .state
        .get(StateNamespace::Session, SESSION_COMPACTION_STATE_KEY)
        .cloned()
        .expect("compaction state should be updated after successful run");
    let compaction_state: SessionCompactionState =
        serde_json::from_value(compaction_state_value).expect("compaction state should parse");
    assert!(compaction_state.is_recovery());
}

#[tokio::test]
async fn test_prepare_general_turn_drops_sections_only_after_compaction_exhausted() {
    let provider = Arc::new(SummaryProvider {
        responses: std::sync::Mutex::new(std::collections::VecDeque::from(vec![
            "{\"title\":\"Prompt Compact\",\"facts\":[\"history condensed\"],\"open_tasks\":[\"continue the current turn\"]}".to_string(),
            "{\"decision\":\"session_only\",\"rationale\":\"near-term continuity only\"}"
                .to_string(),
        ])),
    });
    let tool_registry = Arc::new(ToolRegistry::new());
    register_core_local_tools(tool_registry.as_ref());

    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
    let memory_store = Arc::new(MemoryStore::new(workspace_dir.clone()));
    for idx in 0..12 {
        let timestamp = Utc::now();
        memory_store
            .append_turn(&TalosTurn {
                turn_id: format!("turn-{idx}"),
            message_id: "msg-1".to_string(),
                session_id: "s1".to_string(),
                seq: memory_store
                    .next_turn_seq("s1")
                    .await
                    .expect("seq should load"),
                timestamp,
                items: vec![
                    TalosOutputItem::UserInput {
                        text: format!("historical user message {}", idx),
                    },
                    TalosOutputItem::OutputText {
                        text: format!(
                            "historical assistant message {} with enough content to stress the compacted prompt window",
                            idx
                        ),
                    },
                ],
                usage: TalosUsage::default(),
                stop_reason: TalosApiStopReason::EndTurn,
            })
            .await
            .expect("append should succeed");
    }

    let mut config = AgentKernelConfig::default();
    config.workspace_dir = workspace_dir.to_string_lossy().to_string();
    configure_test_skill_root(&mut config, &workspace_dir);
    config.prompt.bootstrap.enabled = false;
    config.prompt.window.compaction_buffer_tokens = 64;
    config.prompt.window.reserved_output_tokens =
        talos_config::ReservedOutputPolicy::Fixed { tokens: 128 };
    config.compaction.recent_messages_keep = 24;

    let mut ctx = build_ctx_with_stream(None);
    ctx.selected_model_info.context_window = 900;

    let prepared = prepare_general_turn(
        &mut ctx,
        "talos",
        "assistant",
        "continue",
        provider,
        tool_registry,
        memory_store,
        &config,
        Some(skill_options_from_config(&workspace_dir, &config)),
    )
    .await
    .expect("prompt prepare should succeed");

    let compaction = prepared
        .prompt_diagnostics
        .session_history_compaction
        .as_ref()
        .expect("compaction should still run before fallback trim");
    assert_eq!(
        compaction.trigger,
        SessionHistoryCompactionTrigger::BudgetExceeded
    );
    assert_eq!(compaction.outcome, SessionHistoryCompactionOutcome::Applied);
    assert!(compaction.fallback_section_drop_applied);
    assert!(
        prepared
            .prompt_diagnostics
            .dropped_sections
            .contains(&PromptSectionKind::ExpandedToolCatalog),
        "section drop should only happen after compaction cannot restore budget"
    );
}

#[tokio::test]
async fn test_prepare_general_turn_records_rejected_compaction_when_summary_is_missing() {
    let provider = Arc::new(SummaryProvider {
        responses: std::sync::Mutex::new(std::collections::VecDeque::from(vec![
            "{\"title\":\"   \",\"facts\":[\"history condensed\"]}".to_string(),
            "{\"title\":\"   \",\"facts\":[\"history condensed\"]}".to_string(),
            "{\"title\":\"   \",\"facts\":[\"history condensed\"]}".to_string(),
        ])),
    });
    let tool_registry = Arc::new(ToolRegistry::new());
    register_core_local_tools(tool_registry.as_ref());

    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
    let memory_store = Arc::new(MemoryStore::new(workspace_dir.clone()));
    for idx in 0..12 {
        let timestamp = Utc::now();
        memory_store
            .append_turn(&TalosTurn {
                turn_id: format!("turn-{idx}"),
            message_id: "msg-1".to_string(),
                session_id: "s1".to_string(),
                seq: memory_store
                    .next_turn_seq("s1")
                    .await
                    .expect("seq should load"),
                timestamp,
                items: vec![
                    TalosOutputItem::UserInput {
                        text: format!("historical user message {}", idx),
                    },
                    TalosOutputItem::OutputText {
                        text: format!(
                            "historical assistant message {} with enough content to stress the compacted prompt window",
                            idx
                        ),
                    },
                ],
                usage: TalosUsage::default(),
                stop_reason: TalosApiStopReason::EndTurn,
            })
            .await
            .expect("append should succeed");
    }

    let mut config = AgentKernelConfig::default();
    config.workspace_dir = workspace_dir.to_string_lossy().to_string();
    configure_test_skill_root(&mut config, &workspace_dir);
    config.prompt.bootstrap.enabled = false;
    config.prompt.window.compaction_buffer_tokens = 64;
    config.prompt.window.reserved_output_tokens =
        talos_config::ReservedOutputPolicy::Fixed { tokens: 128 };
    config.compaction.recent_messages_keep = 24;

    let mut ctx = build_ctx_with_stream(None);
    ctx.selected_model_info.context_window = 900;

    let prepared = prepare_general_turn(
        &mut ctx,
        "talos",
        "assistant",
        "continue",
        provider,
        tool_registry,
        memory_store,
        &config,
        Some(skill_options_from_config(&workspace_dir, &config)),
    )
    .await
    .expect("prompt prepare should succeed");

    let compaction = prepared
        .prompt_diagnostics
        .session_history_compaction
        .as_ref()
        .expect("rejected compaction attempt should still be recorded");
    assert_eq!(
        compaction.trigger,
        SessionHistoryCompactionTrigger::BudgetExceeded
    );
    assert_eq!(
        compaction.outcome,
        SessionHistoryCompactionOutcome::RejectedMissingSummary
    );
    assert_eq!(compaction.summary_sections_after, 0);
    assert!(compaction.fallback_section_drop_applied);
}

#[tokio::test]
async fn test_prepare_general_turn_rejects_blocked_session_state() {
    let provider = Arc::new(NoopLlmProvider);
    let tool_registry = Arc::new(ToolRegistry::new());
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
    let memory_store = Arc::new(MemoryStore::new(workspace_dir.clone()));
    let mut config = AgentKernelConfig::default();
    config.prompt.bootstrap.enabled = false;
    config.workspace_dir = workspace_dir.to_string_lossy().to_string();
    configure_test_skill_root(&mut config, &workspace_dir);

    let mut ctx = build_ctx_with_stream(None);
    ctx.session.state.insert(
        StateNamespace::Session,
        SESSION_COMPACTION_STATE_KEY.to_string(),
        serde_json::to_value(SessionCompactionState::blocked_prompt_budget_exhausted())
            .expect("blocked state should serialize"),
    );

    let error = prepare_general_turn(
        &mut ctx,
        "talos",
        "assistant",
        "continue",
        provider,
        tool_registry,
        memory_store,
        &config,
        Some(skill_options_from_config(&workspace_dir, &config)),
    )
    .await
    .expect_err("blocked session should reject new execution");

    assert!(error.to_string().contains("reset_session"), "{error}");
}

#[tokio::test]
async fn test_run_general_turn_blocks_session_when_budget_remains_exhausted() {
    let provider = Arc::new(SummaryProvider {
        responses: std::sync::Mutex::new(std::collections::VecDeque::from(vec![
            "{\"title\":\"Prompt Compact\",\"facts\":[\"history condensed\"],\"open_tasks\":[\"continue the current turn\"]}".to_string(),
            "{\"decision\":\"session_only\",\"rationale\":\"near-term continuity only\"}"
                .to_string(),
        ])),
    });
    let tool_registry = Arc::new(ToolRegistry::new());
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
    std::fs::write(
        workspace_dir.join("SOUL.md"),
        "# SOUL\n\nThis bootstrap file intentionally contains a very long stable prefix block to keep the static prompt above a tiny context window.\n\n"
            .repeat(64),
    )
    .expect("soul file should exist");
    let memory_store = Arc::new(MemoryStore::new(workspace_dir.clone()));
    seed_recent_user_message(memory_store.as_ref()).await;

    let mut config = AgentKernelConfig::default();
    config.workspace_dir = workspace_dir.to_string_lossy().to_string();
    configure_test_skill_root(&mut config, &workspace_dir);
    config.prompt.bootstrap.auto_create_missing_files = false;
    config.prompt.window.compaction_buffer_tokens = 8;
    config.prompt.window.reserved_output_tokens =
        talos_config::ReservedOutputPolicy::Fixed { tokens: 32 };

    let mut ctx = build_ctx_with_stream(None);
    ctx.selected_model_info.context_window = 128;

    let error = run_general_turn(
        &mut ctx,
        "talos",
        "assistant",
        "continue",
        provider,
        tool_registry.clone(),
        tool_registry,
        memory_store,
        &config,
        skill_options_from_config(&workspace_dir, &config),
    )
    .await
    .expect_err("unrecoverable prompt budget should block the session");

    assert!(error.to_string().contains("reset_session"), "{error}");
    let blocked_value = ctx
        .session
        .state
        .get(StateNamespace::Session, SESSION_COMPACTION_STATE_KEY)
        .cloned()
        .expect("blocked state should be applied to in-memory session state");
    let blocked_state: SessionCompactionState =
        serde_json::from_value(blocked_value).expect("blocked state should parse");
    assert!(blocked_state.is_blocked());
}

#[tokio::test]
async fn test_prepare_general_turn_uses_compacted_load_policy_during_recovery() {
    let provider = Arc::new(NoopLlmProvider);
    let tool_registry = Arc::new(ToolRegistry::new());
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
    let memory_store = Arc::new(MemoryStore::new(workspace_dir.clone()));
    for idx in 0..4 {
        let timestamp = Utc::now();
        memory_store
            .append_turn(&TalosTurn {
                turn_id: format!("turn-{idx}"),
                message_id: "msg-1".to_string(),
                session_id: "s1".to_string(),
                seq: memory_store
                    .next_turn_seq("s1")
                    .await
                    .expect("seq should load"),
                timestamp,
                items: vec![
                    TalosOutputItem::UserInput {
                        text: format!("historical user message {} {}", idx, "x".repeat(48)),
                    },
                    TalosOutputItem::OutputText {
                        text: format!("historical assistant message {} {}", idx, "y".repeat(48)),
                    },
                ],
                usage: TalosUsage::default(),
                stop_reason: TalosApiStopReason::EndTurn,
            })
            .await
            .expect("append should succeed");
    }

    let mut config = AgentKernelConfig::default();
    config.prompt.bootstrap.enabled = false;
    config.compaction.recent_messages_keep = 6;
    config.compaction.compacted_recent_messages_keep = 6;
    config.compaction.recent_messages_max_chars = 2_000;
    config.compaction.compacted_recent_messages_max_chars = 180;
    config.compaction.recent_summaries_keep = 2;
    config.prompt.window.reserved_output_tokens =
        talos_config::ReservedOutputPolicy::Fixed { tokens: 512 };
    config.workspace_dir = workspace_dir.to_string_lossy().to_string();
    configure_test_skill_root(&mut config, &workspace_dir);

    let mut normal_ctx = build_ctx_with_stream(None);
    normal_ctx.selected_model_info.context_window = 16_384;
    let normal_prepared = prepare_general_turn(
        &mut normal_ctx,
        "talos",
        "assistant",
        "continue",
        provider.clone(),
        tool_registry.clone(),
        memory_store.clone(),
        &config,
        Some(skill_options_from_config(&workspace_dir, &config)),
    )
    .await
    .expect("normal prepare should succeed");
    let normal_rendered = normal_prepared
        .messages
        .iter()
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(normal_rendered.contains("historical user message 1"));
    assert!(normal_rendered.contains("historical user message 3"));
    assert!(normal_rendered.contains("historical assistant message 1"));

    let mut recovery_ctx = build_ctx_with_stream(None);
    recovery_ctx.selected_model_info.context_window = 16_384;
    recovery_ctx.session.state.insert(
        StateNamespace::Session,
        SESSION_COMPACTION_STATE_KEY.to_string(),
        serde_json::to_value(SessionCompactionState::recovery())
            .expect("recovery state should serialize"),
    );
    let recovery_prepared = prepare_general_turn(
        &mut recovery_ctx,
        "talos",
        "assistant",
        "continue",
        provider,
        tool_registry,
        memory_store,
        &config,
        Some(skill_options_from_config(&workspace_dir, &config)),
    )
    .await
    .expect("recovery prepare should succeed");
    let recovery_rendered = recovery_prepared
        .messages
        .iter()
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!recovery_rendered.contains("historical user message 1"));
    assert!(recovery_rendered.contains("historical user message 3"));
    assert!(recovery_rendered.contains("historical assistant message 3"));
    assert!(!recovery_rendered.contains("historical assistant message 1"));
}
