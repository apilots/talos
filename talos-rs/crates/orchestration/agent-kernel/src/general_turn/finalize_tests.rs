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
        reasoning_log: Vec::new(),
        tool_call_log: Vec::new(),
        tool_result_log: Vec::new(),
        retrieved_memories: Vec::new(),
        tool_call_count: 0,
        tool_error_count: 0,
        pending_dynamic_requests: Vec::new(),
        final_answer_streamed: false,
        execution_summary_streamed: false,
    };

    let metrics = build_turn_execution_metrics(&state, StopReason::EndTurn);

    assert!(metrics.duration_ms.is_some());
}
