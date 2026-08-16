use super::*;

fn prepared_with_budget(effective_prompt_budget: usize) -> PreparedGeneralTurn {
    PreparedGeneralTurn {
        session_id: "s1".to_string(),
        user_id: "u1".to_string(),
        message_id: "m1".to_string(),
        invocation_id: "turn-1".to_string(),
        agent_id: "talos".to_string(),
        project_root: None,
        selected_model: "test-model".to_string(),
        max_output_tokens: None,
        reasoning_effort: None,
        speed: Default::default(),
        attachment_tokens: 0,
        prompt_prefix_messages: Vec::new(),
        messages: vec![ChatMessage::user("current task")],
        maintenance_usage: TokenUsage::default(),
        tool_definitions: Vec::new(),
        tool_selection_request: talos_tools::ToolSelectionRequest::default(),
        prompt_diagnostics: PromptDiagnostics {
            mode: talos_prompt::PromptMode::Full,
            included_sections: Vec::new(),
            dropped_sections: Vec::new(),
            bootstrap_files: Vec::new(),
            total_estimated_tokens: 0,
            static_estimated_tokens: 0,
            dynamic_estimated_tokens: 0,
            window_usage: talos_prompt::PromptWindowUsage {
                window_resolved: true,
                effective_prompt_budget,
                within_effective_budget: true,
                ..Default::default()
            },
            session_history_compaction: None,
        },
        warnings: Vec::new(),
        prompt_warning_state: None,
        stream_tx: None,
        message_rx: None,
    }
}

#[test]
fn test_working_context_trim_preserves_recent_complete_tool_exchange() {
    let prepared = prepared_with_budget(80);
    let mut state = GeneralTurnLoopState::new(&prepared);
    let body = "The user prefers tea. ".repeat(80);
    let memory = talos_types::MemoryInfo {
        memory_id: "mem-1".to_string(),
        title: "Tea preference".to_string(),
        body,
        kind: talos_types::MemoryKind::Preference,
        importance: talos_types::MemoryImportance::High,
        occurred_at: 1,
        source: talos_types::MemorySourceInfo {
            date: "2026-07-20".to_string(),
            session_id: None,
            from_seq: None,
            until_seq: None,
        },
    };
    let batch =
        super::tool_batch::collect_general_turn_tool_batch(vec![crate::ToolExecutionOutput {
            call_id: "call-old".to_string(),
            tool_name: "MemoryGet".to_string(),
            arguments: serde_json::json!({"memory_id": "mem-1"}),
            outcome: ToolDispatchOutcome::executed(
                ToolOutputPayload::success_json(
                    &MemoryGetResponse {
                        memory: memory.clone(),
                    },
                    "serialize test memory",
                )
                .expect("memory payload"),
            ),
            is_error: false,
        }])
        .expect("collect retrieved memory batch");
    state.messages = vec![
        ChatMessage::assistant("old assistant text ".repeat(80)),
        ChatMessage::assistant_with_tool_calls(
            "old call",
            vec![FunctionCall::new(
                "call-old",
                "MemoryGet",
                serde_json::json!({}),
            )],
        ),
        batch.tool_messages[0].clone(),
        ChatMessage::assistant_with_tool_calls(
            "fresh call",
            vec![FunctionCall::new(
                "call-fresh",
                "Read",
                serde_json::json!({}),
            )],
        ),
        ChatMessage::tool("call-fresh", "Read", "fresh output"),
    ];

    let report = working_context::trim_working_context_to_budget(
        &prepared,
        &mut state,
        80,
        1,
        &AgentKernelConfig::default(),
    )
    .expect("working context should trim");

    assert!(report.trimmed_messages > 0);
    assert!(state.messages.iter().any(|message| {
        message.tool_call_id.as_deref() == Some("call-fresh")
            || message
                .tool_calls
                .as_ref()
                .is_some_and(|calls| calls.iter().any(|call| call.id == "call-fresh"))
    }));
    assert!(!state.messages.iter().any(|message| {
        message.tool_call_id.as_deref() == Some("call-old")
            || message
                .tool_calls
                .as_ref()
                .is_some_and(|calls| calls.iter().any(|call| call.id == "call-old"))
    }));
    assert_eq!(state.retrieved_memories, vec![memory]);
}

#[test]
fn test_working_context_trim_skips_failed_memory_get_result() {
    let prepared = prepared_with_budget(16);
    let mut state = GeneralTurnLoopState::new(&prepared);
    let batch =
        super::tool_batch::collect_general_turn_tool_batch(vec![crate::ToolExecutionOutput {
            call_id: "call-memory".to_string(),
            tool_name: "MemoryGet".to_string(),
            arguments: serde_json::json!({"memory_id": "missing"}),
            outcome: ToolDispatchOutcome::executed(ToolOutputPayload::failure_structured(
                "memory was not found ".repeat(20),
                None,
            )),
            is_error: true,
        }])
        .expect("collect failed memory batch");
    state.messages = vec![
        ChatMessage::assistant_with_tool_calls(
            "memory call",
            vec![FunctionCall::new(
                "call-memory",
                "MemoryGet",
                serde_json::json!({}),
            )],
        ),
        batch.tool_messages[0].clone(),
        ChatMessage::assistant_with_tool_calls(
            "fresh call",
            vec![FunctionCall::new(
                "call-fresh",
                "Read",
                serde_json::json!({}),
            )],
        ),
        ChatMessage::tool("call-fresh", "Read", "fresh output"),
    ];

    let report = working_context::trim_working_context_to_budget(
        &prepared,
        &mut state,
        16,
        1,
        &AgentKernelConfig::default(),
    )
    .expect("failed MemoryGet should be removable");

    assert!(report.trimmed_messages > 0);
    assert!(state.retrieved_memories.is_empty());
}

#[test]
fn test_working_context_trim_fails_when_latest_group_is_too_large() {
    let prepared = prepared_with_budget(16);
    let mut state = GeneralTurnLoopState::new(&prepared);
    state.messages = vec![ChatMessage::assistant("x".repeat(4_000))];

    let report = working_context::trim_working_context_to_budget(
        &prepared,
        &mut state,
        16,
        1,
        &AgentKernelConfig::default(),
    )
    .expect("working context should report the exhausted budget");

    assert!(report.post_tokens > 16);
    assert_eq!(state.messages.len(), 1);
}

#[test]
fn test_working_context_budget_includes_untrimmed_attachment_tokens() {
    let mut prepared = prepared_with_budget(16);
    prepared.attachment_tokens = 16;
    let mut state = GeneralTurnLoopState::new(&prepared);

    let report = working_context::trim_working_context_to_budget(
        &prepared,
        &mut state,
        16,
        1,
        &AgentKernelConfig::default(),
    )
    .expect("attachment budget should be reported without trimming the current input");

    assert!(report.post_tokens > 16);
    assert_eq!(state.messages.len(), 0);
}

#[test]
fn test_working_context_trim_rejects_invalid_memory_get_result() {
    let prepared = prepared_with_budget(16);
    let mut state = GeneralTurnLoopState::new(&prepared);
    state.messages = vec![
        ChatMessage::assistant_with_tool_calls(
            "memory call",
            vec![FunctionCall::new(
                "call-memory",
                "MemoryGet",
                serde_json::json!({}),
            )],
        ),
        ChatMessage::tool("call-memory", "MemoryGet", "invalid result"),
        ChatMessage::assistant_with_tool_calls(
            "fresh call",
            vec![FunctionCall::new(
                "call-fresh",
                "Read",
                serde_json::json!({}),
            )],
        ),
        ChatMessage::tool("call-fresh", "Read", "fresh output"),
    ];

    let error = working_context::trim_working_context_to_budget(
        &prepared,
        &mut state,
        16,
        1,
        &AgentKernelConfig::default(),
    )
    .expect_err("invalid MemoryGet result should fail context rebuild");

    assert!(error.is_internal());
}
