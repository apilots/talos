use super::*;
use talos_llm_provider::{ChatMessage, ModelSpeed, ReasoningEffort};
use talos_prompt::{PromptMode, PromptWindowUsage};

#[test]
fn test_build_general_turn_step_request_respects_parallel_flag() {
    let prepared = PreparedGeneralTurn {
        session_id: "s1".to_string(),
        message_id: "msg-1".to_string(),
        invocation_id: "i1".to_string(),
        agent_id: "a1".to_string(),
        project_root: None,
        selected_model: "model".to_string(),
        max_output_tokens: Some(2_048),
        reasoning_effort: Some(ReasoningEffort::Medium),
        speed: ModelSpeed::Fast,
        prompt_prefix_messages: Vec::new(),
        messages: vec![ChatMessage::user("current user input")],
        maintenance_usage: TokenUsage::default(),
        tool_definitions: Vec::new(),
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
        warnings: Vec::new(),
        prompt_warning_state: None,
        stream_tx: None,
    };
    let state = GeneralTurnLoopState::new(&prepared);

    let sequential_request = build_general_turn_step_request(&prepared, &state, Vec::new(), false);
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

    let parallel_request = build_general_turn_step_request(&prepared, &state, Vec::new(), true);
    assert!(parallel_request.parallel_tool_calls);
    assert_eq!(parallel_request.speed, ModelSpeed::Fast);
}
