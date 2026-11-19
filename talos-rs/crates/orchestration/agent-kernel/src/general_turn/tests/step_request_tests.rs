use super::*;
use talos_llm_provider::{ChatMessage, ModelSpeed, ReasoningEffort};
use talos_prompt::{PromptMode, PromptWindowUsage};

#[tokio::test]
async fn test_build_general_turn_step_request() {
    let mut prepared = PreparedGeneralTurn {
        registration: None,
        trajectory: None,
        browser_uploads: Vec::new(),
        session_id: "s1".to_string(),
        user_id: "u1".to_string(),
        invocation_id: "i1".to_string(),
        agent_id: "a1".to_string(),
        workspace_id: "default".to_string(),
        project_root: None,
        selected_model: "model".to_string(),
        max_output_tokens: Some(2_048),
        reasoning_effort: Some(ReasoningEffort::Medium),
        speed: ModelSpeed::Fast,
        attachment_tokens: 0,
        image_input_enabled: false,
        messages: vec![ChatMessage::user("current user input")],
        maintenance_usage: TokenUsage::default(),
        tool_selection_request: talos_tools::ToolSelectionRequest::default().with_session_id("s1"),
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
        prompt_warning_state: None,
        stream_tx: None,
        message_rx: None,
        run_tasks: crate::test_run_tasks(),
    };
    let mut state = GeneralTurnLoopState::new(&prepared);

    let sequential_request =
        build_general_turn_step_request(&prepared, &state, Vec::new(), false, 4, 2_000);
    assert!(!sequential_request.parallel_tool_calls);
    assert_eq!(sequential_request.messages.len(), 1);
    assert_eq!(sequential_request.messages[0].content, "current user input");
    let sequential_chat_request = sequential_request.into_chat_request();
    assert_eq!(sequential_chat_request.max_tokens, Some(2_048));
    assert_eq!(
        sequential_chat_request.reasoning_effort,
        Some(ReasoningEffort::Medium)
    );
    assert_eq!(sequential_chat_request.speed, Some(ModelSpeed::Fast));

    let parallel_request =
        build_general_turn_step_request(&prepared, &state, Vec::new(), true, 4, 2_000);
    assert!(parallel_request.parallel_tool_calls);
    assert_eq!(parallel_request.speed, ModelSpeed::Fast);
    state.retrieved_memories.push(talos_types::MemoryInfo {
        memory_id: "mem-1".to_string(),
        title: "Tea preference".to_string(),
        body: "The user prefers tea.".to_string(),
        kind: talos_types::MemoryKind::Preference,
        importance: talos_types::MemoryImportance::High,
        occurred_at: 1,
        source: talos_types::MemorySourceInfo {
            date: "2026-07-20".to_string(),
            session_id: None,
            from_seq: None,
            until_seq: None,
        },
    });

    let request = build_general_turn_step_request(&prepared, &state, Vec::new(), false, 4, 2_000);

    assert!(request.messages.iter().any(|message| {
        message.content.contains("retrieved_memories")
            && message.content.contains("The user prefers tea.")
    }));

    let tool_output = ChatMessage::tool("lookup", "WebFetch", "Verified dates");
    state.messages.push(tool_output.clone());
    let short = build_general_turn_step_request(&prepared, &state, Vec::new(), false, 0, 0);
    assert_eq!(short.messages.last().unwrap().content, "Verified dates");

    state.started_at -= std::time::Duration::from_secs(31);
    let long = build_general_turn_step_request(&prepared, &state, Vec::new(), false, 0, 0);
    assert_eq!(long.messages.len(), short.messages.len() + 1);
    assert!(long
        .messages
        .last()
        .unwrap()
        .content
        .contains("user-visible progress update"));
    assert_eq!(state.messages.len(), 1);

    prepared.prompt_diagnostics.window_usage.window_resolved = true;
    prepared
        .prompt_diagnostics
        .window_usage
        .effective_prompt_budget = 32;
    let budgeted = build_general_turn_step_request(&prepared, &state, Vec::new(), false, 0, 0);
    assert_eq!(budgeted.messages.last().unwrap().content, "Verified dates");
    prepared.prompt_diagnostics.window_usage.window_resolved = false;

    state
        .messages
        .insert(0, ChatMessage::assistant("The dates are confirmed."));
    let reported = build_general_turn_step_request(&prepared, &state, Vec::new(), false, 0, 0);
    assert_eq!(
        reported.messages.last().unwrap().content,
        tool_output.content
    );
}
