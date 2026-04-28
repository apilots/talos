use super::*;
use crate::general_turn::types::SessionCompactionStatus;
use crate::StateNamespace;
use talos_prompt::{
    CompactionTrimUnitKind, PromptMode, PromptSectionKind, PromptWindowUsage, SessionContinuity,
    SessionHistoryCompactionOutcome, SessionHistoryCompactionTrigger,
};

#[test]
fn test_build_general_turn_step_request_respects_parallel_flag() {
    let prepared = PreparedGeneralTurn {
        session_id: "s1".to_string(),
        invocation_id: "i1".to_string(),
        agent_id: "a1".to_string(),
        project_root: None,
        selected_model: "model".to_string(),
        prompt_prefix_messages: Vec::new(),
        messages: Vec::new(),
        tool_definitions: Vec::new(),
        prompt_diagnostics: PromptDiagnostics {
            mode: PromptMode::Full,
            included_sections: Vec::new(),
            dropped_sections: Vec::new(),
            bootstrap_files: Vec::new(),
            total_estimated_tokens: 0,
            static_estimated_tokens: 0,
            dynamic_estimated_tokens: 0,
            window_usage: PromptWindowUsage::default(),
            session_history_compaction: None,
        },
        warnings: Vec::new(),
        prompt_warning_state: None,
        stream_tx: None,
    };
    let state = GeneralTurnLoopState::new(&prepared);

    let sequential_request = build_general_turn_step_request(&prepared, &state, false);
    assert!(!sequential_request.parallel_tool_calls);

    let parallel_request = build_general_turn_step_request(&prepared, &state, true);
    assert!(parallel_request.parallel_tool_calls);
}

#[tokio::test]
async fn test_run_general_turn_emits_prompt_warning_event() {
    let provider = Arc::new(NoopLlmProvider);
    let tool_registry = Arc::new(ToolRegistry::new());
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
    std::fs::write(
        workspace_dir.join("SOUL.md"),
        "# SOUL\n\n- Keep the run grounded in repo evidence.\n",
    )
    .expect("soul file should exist");
    let memory_store = Arc::new(MemoryStore::new(workspace_dir.clone()));
    seed_recent_user_message(memory_store.as_ref()).await;
    let mut config = AgentKernelConfig::default();
    config.prompt.bootstrap.auto_create_missing_files = false;
    config.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();
    let mut ctx = build_ctx_with_stream(None);

    let result = run_general_turn(
        &mut ctx,
        "talos",
        "assistant",
        "hello",
        provider,
        tool_registry.clone(),
        tool_registry,
        memory_store,
        &config,
    )
    .await
    .expect("general turn should succeed");

    assert!(!result.warnings.is_empty());
    assert!(ctx.events.iter().any(|event| {
        matches!(event.event_type, KernelEventType::PromptWarning)
            && matches!(event.label, KernelEventLabel::PromptWarning)
            && matches!(event.data, KernelEventData::PromptWarning(_))
    }));
}

#[tokio::test]
async fn test_run_general_turn_streams_prompt_warning_event() {
    let provider = Arc::new(NoopLlmProvider);
    let tool_registry = Arc::new(ToolRegistry::new());
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
    std::fs::write(
        workspace_dir.join("SOUL.md"),
        "# SOUL\n\n- Keep the run grounded in repo evidence.\n",
    )
    .expect("soul file should exist");
    let memory_store = Arc::new(MemoryStore::new(workspace_dir.clone()));
    seed_recent_user_message(memory_store.as_ref()).await;
    let mut config = AgentKernelConfig::default();
    config.prompt.bootstrap.auto_create_missing_files = false;
    config.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();
    let (tx, mut rx) = mpsc::channel(16);
    let mut ctx = build_ctx_with_stream(Some(tx));

    let _ = run_general_turn(
        &mut ctx,
        "talos",
        "assistant",
        "hello",
        provider,
        tool_registry.clone(),
        tool_registry,
        memory_store,
        &config,
    )
    .await
    .expect("general turn should succeed");

    let mut saw_prompt_warning = false;
    while let Ok(event) = rx.try_recv() {
        if matches!(event.event_type, KernelStreamEventType::PromptWarning)
            && matches!(event.data, KernelStreamEventData::PromptWarning(_))
        {
            saw_prompt_warning = true;
            break;
        }
    }
    assert!(saw_prompt_warning, "stream should include prompt.warning");
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
    config.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();
    config.prompt.bootstrap.enabled = false;
    config.prompt.window.compaction_buffer_tokens = 32;
    config.compaction.recent_messages_keep = 24;

    let mut ctx = build_ctx_with_stream(None);
    ctx.selected_model_info.context_window = 2_400;
    ctx.selected_model_info.max_tokens = 128;

    let prepared = prepare_general_turn(
        &mut ctx,
        "talos",
        "assistant",
        "continue",
        provider,
        tool_registry,
        memory_store.clone(),
        &config,
    )
    .await
    .expect("prompt prepare should succeed");

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
    config.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();
    config.prompt.bootstrap.enabled = false;
    config.prompt.window.compaction_buffer_tokens = 32;
    config.compaction.recent_messages_keep = 8;
    config.compaction.trigger_ratio_percent = 1;
    config.compaction.hard_ratio_percent = 100;

    let mut ctx = build_ctx_with_stream(None);
    ctx.selected_model_info.context_window = 6_000;
    ctx.selected_model_info.max_tokens = 128;

    let prepared = prepare_general_turn(
        &mut ctx,
        "talos",
        "assistant",
        "continue",
        provider,
        tool_registry,
        memory_store,
        &config,
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
    config.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();
    config.prompt.bootstrap.enabled = false;
    config.prompt.window.compaction_buffer_tokens = 32;
    config.compaction.recent_messages_keep = 24;

    let mut ctx = build_ctx_with_stream(None);
    ctx.selected_model_info.context_window = 2_400;
    ctx.selected_model_info.max_tokens = 128;

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
    config.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();
    config.prompt.bootstrap.enabled = false;
    config.prompt.window.compaction_buffer_tokens = 64;
    config.compaction.recent_messages_keep = 24;

    let mut ctx = build_ctx_with_stream(None);
    ctx.selected_model_info.context_window = 900;
    ctx.selected_model_info.max_tokens = 128;

    let prepared = prepare_general_turn(
        &mut ctx,
        "talos",
        "assistant",
        "continue",
        provider,
        tool_registry,
        memory_store,
        &config,
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
    config.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();
    config.prompt.bootstrap.enabled = false;
    config.prompt.window.compaction_buffer_tokens = 64;
    config.compaction.recent_messages_keep = 24;

    let mut ctx = build_ctx_with_stream(None);
    ctx.selected_model_info.context_window = 900;
    ctx.selected_model_info.max_tokens = 128;

    let prepared = prepare_general_turn(
        &mut ctx,
        "talos",
        "assistant",
        "continue",
        provider,
        tool_registry,
        memory_store,
        &config,
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
    config.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();

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
    config.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();
    config.prompt.bootstrap.auto_create_missing_files = false;
    config.prompt.window.compaction_buffer_tokens = 8;

    let mut ctx = build_ctx_with_stream(None);
    ctx.selected_model_info.context_window = 128;
    ctx.selected_model_info.max_tokens = 32;

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
    config.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();

    let mut normal_ctx = build_ctx_with_stream(None);
    normal_ctx.selected_model_info.context_window = 16_384;
    normal_ctx.selected_model_info.max_tokens = 512;
    let normal_prepared = prepare_general_turn(
        &mut normal_ctx,
        "talos",
        "assistant",
        "continue",
        provider.clone(),
        tool_registry.clone(),
        memory_store.clone(),
        &config,
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
    recovery_ctx.selected_model_info.max_tokens = 512;
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

#[tokio::test]
async fn test_run_general_turn_bash_then_task_output() {
    let provider = Arc::new(ExecLoopProvider::default());
    let tool_registry = Arc::new(ToolRegistry::new());
    register_core_local_tools(tool_registry.as_ref());

    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
    let memory_store = Arc::new(MemoryStore::new(workspace_dir.clone()));
    let mut config = AgentKernelConfig::default();
    config.prompt.bootstrap.enabled = false;
    config.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();
    let mut ctx = build_ctx_with_stream(None);

    let result = run_general_turn(
        &mut ctx,
        "talos",
        "assistant",
        "Use Bash, then continue with TaskOutput.",
        provider.clone(),
        tool_registry.clone(),
        tool_registry,
        memory_store,
        &config,
    )
    .await
    .expect("general turn should succeed");

    assert!(result.output_text.contains("done:ok"));
    assert!(result
        .output_items
        .iter()
        .any(|item| matches!(item, talos_types::talos::TalosOutputItem::ToolResult { .. })));
    assert!(provider.round.load(Ordering::SeqCst) >= 3);
}

#[tokio::test]
async fn test_run_general_turn_stops_on_pending_tool_confirmation() {
    let tool_registry = Arc::new(ToolRegistry::new());
    register_core_local_tools(tool_registry.as_ref());

    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
    let memory_store = Arc::new(MemoryStore::new(workspace_dir.clone()));
    let mut config = AgentKernelConfig::default();
    config.prompt.bootstrap.enabled = false;
    config.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();
    let mut ctx = build_ctx_with_stream(None);

    let provider = Arc::new(ApprovalLoopProvider::default());
    let result = run_general_turn(
        &mut ctx,
        "talos",
        "assistant",
        "Use Bash with escalated permissions.",
        provider.clone(),
        tool_registry.clone(),
        tool_registry,
        memory_store,
        &config,
    )
    .await
    .expect("general turn should succeed");

    assert_eq!(result.output_text, "Tool confirmation pending.");
    assert!(result.output_items.iter().any(|item| matches!(
        item,
        talos_types::talos::TalosOutputItem::ToolResult { is_error: true, .. }
    )));
    assert_eq!(provider.round.load(Ordering::SeqCst), 1);

    let continuity_value = ctx
        .session
        .state
        .get(StateNamespace::Session, SESSION_CONTINUITY_STATE_KEY)
        .cloned()
        .expect("session continuity should be staged after the turn");
    let continuity: SessionContinuity =
        serde_json::from_value(continuity_value).expect("continuity should parse");
    assert_eq!(
        continuity.recent_goal,
        "Use Bash with escalated permissions."
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

#[tokio::test]
async fn test_run_general_turn_forces_summary_when_final_response_is_empty_after_tools() {
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
    std::fs::write(workspace_dir.join("Cargo.toml"), "[workspace]\n")
        .expect("fixture should exist");

    let provider = Arc::new(EmptyFinalAfterToolsProvider {
        dir_path: workspace_dir.to_string_lossy().to_string(),
        ..Default::default()
    });
    let tool_registry = Arc::new(ToolRegistry::new());
    register_core_local_tools(tool_registry.as_ref());
    let memory_store = Arc::new(MemoryStore::new(workspace_dir.clone()));
    let mut config = AgentKernelConfig::default();
    config.prompt.bootstrap.enabled = false;
    config.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();
    let mut ctx = build_ctx_with_stream(None);

    let result = run_general_turn(
        &mut ctx,
        "talos",
        "assistant",
        "Analyze the project architecture from code.",
        provider.clone(),
        tool_registry.clone(),
        tool_registry,
        memory_store,
        &config,
    )
    .await
    .expect("general turn should succeed");

    assert_eq!(
        result.output_text,
        "Final architecture summary based on verified tool results."
    );
    assert!(
        !result.output_text.contains("Tool execution completed."),
        "forced summary should replace placeholder fallback"
    );
    assert!(provider.round.load(Ordering::SeqCst) >= 3);
}

#[tokio::test]
async fn test_run_general_turn_uses_salvage_call_when_forced_summary_is_still_empty() {
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
    std::fs::write(workspace_dir.join("Cargo.toml"), "[workspace]\n")
        .expect("fixture should exist");

    let provider = Arc::new(SalvageAfterEmptyFinalProvider {
        dir_path: workspace_dir.to_string_lossy().to_string(),
        ..Default::default()
    });
    let tool_registry = Arc::new(ToolRegistry::new());
    register_core_local_tools(tool_registry.as_ref());
    let memory_store = Arc::new(MemoryStore::new(workspace_dir.clone()));
    let mut config = AgentKernelConfig::default();
    config.prompt.bootstrap.enabled = false;
    config.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();
    let mut ctx = build_ctx_with_stream(None);

    let result = run_general_turn(
        &mut ctx,
        "talos",
        "assistant",
        "Analyze the project architecture from code.",
        provider.clone(),
        tool_registry.clone(),
        tool_registry,
        memory_store,
        &config,
    )
    .await
    .expect("general turn should succeed");

    assert_eq!(
        result.output_text,
        "Salvaged final answer from verified tool results."
    );
    assert!(
        !result.output_text.contains("Tool execution completed."),
        "salvage call should replace placeholder fallback"
    );
    assert!(provider.round.load(Ordering::SeqCst) >= 4);
}
