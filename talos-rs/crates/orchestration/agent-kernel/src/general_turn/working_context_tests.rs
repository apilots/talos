use super::*;

fn prepared_with_budget(effective_prompt_budget: usize) -> PreparedGeneralTurn {
    PreparedGeneralTurn {
        session_id: "s1".to_string(),
        message_id: "m1".to_string(),
        invocation_id: "turn-1".to_string(),
        agent_id: "talos".to_string(),
        project_root: None,
        selected_model: "test-model".to_string(),
        max_output_tokens: None,
        reasoning_effort: None,
        speed: Default::default(),
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
    }
}

#[test]
fn test_working_context_trim_preserves_recent_complete_tool_exchange() {
    let prepared = prepared_with_budget(80);
    let mut state = GeneralTurnLoopState::new(&prepared);
    state.messages = vec![
        ChatMessage::assistant("old assistant text ".repeat(80)),
        ChatMessage::assistant_with_tool_calls(
            "old call",
            vec![FunctionCall::new("call-old", "Read", serde_json::json!({}))],
        ),
        ChatMessage::tool("call-old", "Read", "old output ".repeat(80)),
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

    let report = working_context::trim_working_context_to_budget(&prepared, &mut state, 80, 1);

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
}

#[test]
fn test_working_context_trim_fails_when_latest_group_is_too_large() {
    let prepared = prepared_with_budget(16);
    let mut state = GeneralTurnLoopState::new(&prepared);
    state.messages = vec![ChatMessage::assistant("x".repeat(4_000))];

    let report = working_context::trim_working_context_to_budget(&prepared, &mut state, 16, 1);

    assert!(report.post_tokens > 16);
    assert_eq!(state.messages.len(), 1);
}
