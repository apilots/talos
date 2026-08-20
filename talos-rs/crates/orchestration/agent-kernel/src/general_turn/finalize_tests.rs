use super::*;
use std::time::{Duration, Instant};

#[test]
fn build_turn_execution_metrics_includes_elapsed_duration() {
    let state = GeneralTurnLoopState {
        started_at: Instant::now() - Duration::from_millis(5),
        messages: Vec::new(),
        round: 0,
        output_items: Vec::new(),
        usage: TokenUsage::default(),
        last_model_usage: TokenUsage {
            input_tokens: 42,
            output_tokens: 8,
            total_tokens: 50,
            ..TokenUsage::default()
        },
        reasoning_log: Vec::new(),
        tool_call_log: Vec::new(),
        tool_result_log: Vec::new(),
        retrieved_memories: Vec::new(),
        tool_call_count: 0,
        tool_result_count: 0,
        tool_error_count: 0,
        pending_user_action: None,
        final_answer_streamed: false,
        execution_summary_streamed: false,
        elapsed_duration_ms: 0,
        active_turn_attachment_tokens: 0,
    };

    let metrics = build_turn_execution_metrics(&state, StopReason::EndTurn);

    assert!(metrics.duration_ms.is_some());
}
