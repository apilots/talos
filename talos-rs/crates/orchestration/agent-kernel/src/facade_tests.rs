use std::collections::HashSet;

use talos_prompt::{PromptDiagnostics, PromptMode, PromptWindowUsage};
use talos_types::{TokenUsage, ToolArguments, TurnExecutionMetrics};

use super::*;

#[test]
fn missing_tool_events_only_fill_unstreamed_result_items() {
    let result = GeneralTurnResult {
        last_model_usage: TokenUsage::default(),
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
        outcome: GeneralTurnOutcome::Completed {
            output_items: vec![
                KernelRunItem::ToolCall {
                    call_id: "call-1".to_owned(),
                    name: "search".to_owned(),
                    arguments: ToolArguments::from(serde_json::json!({"query": "Talos"})),
                },
                KernelRunItem::ToolResult {
                    call_id: "call-1".to_owned(),
                    tool_name: "search".to_owned(),
                    content: Vec::new(),
                    is_error: false,
                },
            ],
            stop_reason: StopReason::EndTurn,
            usage: TokenUsage::default(),
            metrics: TurnExecutionMetrics::default(),
        },
    };

    let events = missing_tool_events(&result, &HashSet::new(), &HashSet::new());

    assert_eq!(events.len(), 2);
    assert!(matches!(
        events[0].payload,
        AgentKernelEventPayload::ToolCallStart(_)
    ));
    assert!(matches!(
        events[1].payload,
        AgentKernelEventPayload::ToolResult(_)
    ));
}
