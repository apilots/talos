use super::*;
use talos_prompt::{PromptMode, PromptWindowUsage};

#[test]
fn test_build_general_turn_step_request_respects_parallel_flag() {
    let prepared = PreparedGeneralTurn {
        session_id: "s1".to_string(),
        invocation_id: "i1".to_string(),
        agent_id: "a1".to_string(),
        project_root: None,
        selected_model: "model".to_string(),
        max_output_tokens: Some(2_048),
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
    assert_eq!(
        sequential_request.into_chat_request().max_tokens,
        Some(2_048)
    );

    let parallel_request = build_general_turn_step_request(&prepared, &state, true);
    assert!(parallel_request.parallel_tool_calls);
}
