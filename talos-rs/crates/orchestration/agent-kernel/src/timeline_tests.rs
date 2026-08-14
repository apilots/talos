use super::*;
use talos_types::talos::TalosRunErrorCode;

fn item_id(event: &AgentEvent) -> String {
    match &event.payload {
        AgentEventPayload::ItemStarted(payload) => payload.item.item_id.clone(),
        _ => panic!("expected item started event"),
    }
}

#[test]
fn timeline_builder_orders_reasoning_tool_progress_and_final_answer() {
    let mut builder = TimelineBuilder::new("request-1", "turn-1", "message-1", "session-1");
    let turn_started = builder.start_turn();
    assert_eq!(turn_started.sequence, 0);

    let reasoning_started = builder.start_reasoning();
    let reasoning_id = item_id(&reasoning_started);
    builder
        .append_delta(
            &reasoning_id,
            TalosTimelineDelta::Text {
                text: "think".to_string(),
            },
        )
        .expect("reasoning delta should apply");
    builder
        .complete_item(&reasoning_id, TalosTimelineItemStatus::Completed, None)
        .expect("reasoning should complete");

    let tool_started = builder.start_tool_call("call-1", "read", "{}");
    let tool_id = item_id(&tool_started);
    let progress_event = builder
        .append_delta(
            &tool_id,
            TalosTimelineDelta::ToolProgress {
                progress_kind: TalosToolProgressKind::Stdout,
                text_delta: "reading file".to_string(),
            },
        )
        .expect("tool progress should apply");
    builder
        .append_delta(
            &tool_id,
            TalosTimelineDelta::ToolResult {
                content: vec![TalosContentItem::Text {
                    text: "done".to_string(),
                }],
                is_error: false,
            },
        )
        .expect("tool result should apply");
    builder
        .complete_item(&tool_id, TalosTimelineItemStatus::Completed, None)
        .expect("tool should complete");

    let final_started = builder.start_final_answer();
    let final_id = item_id(&final_started);
    builder
        .append_delta(
            &final_id,
            TalosTimelineDelta::Text {
                text: "final".to_string(),
            },
        )
        .expect("final answer delta should apply");

    assert_eq!(
        builder.item(&reasoning_id).expect("reasoning exists").phase,
        TalosTimelinePhase::Thinking
    );
    assert_eq!(
        builder.item(&tool_id).expect("tool exists").kind,
        TalosTimelineItemKind::ToolCall
    );
    assert_eq!(
        builder.item(&final_id).expect("final exists").phase,
        TalosTimelinePhase::FinalAnswer
    );

    match progress_event.payload {
        AgentEventPayload::ItemDelta(payload) => {
            assert_eq!(payload.item_id, tool_id);
        }
        _ => panic!("expected item delta payload"),
    }
    match &builder.item(&tool_id).expect("tool exists").content {
        TalosTimelineContent::ToolCall {
            progress, result, ..
        } => {
            assert_eq!(progress.len(), 1);
            assert!(result.is_some());
        }
        _ => panic!("expected tool call content"),
    }
}

#[test]
fn timeline_builder_preserves_structured_user_input() {
    let mut builder = TimelineBuilder::new("request-1", "turn-1", "message-1", "session-1");

    let started = builder.start_user_input(String::new(), vec!["file-1".to_string()]);

    let AgentEventPayload::ItemStarted(payload) = started.payload else {
        panic!("expected item started event");
    };
    assert_eq!(
        payload.item.content,
        TalosTimelineContent::UserInput {
            text: String::new(),
            file_ids: vec!["file-1".to_string()],
        }
    );
}

#[test]
fn timeline_builder_continues_item_ids_after_tracking_a_resumed_tool() {
    let mut builder = TimelineBuilder::new("request-2", "turn-1", "message-1", "session-1");
    builder.track_existing_tool_call("tool-3", "call-1", "Bash", "{}");

    let reasoning = builder.start_reasoning();

    assert_eq!(item_id(&reasoning), "reasoning-4");
}

#[test]
fn timeline_builder_rejects_delta_without_active_item() {
    let mut builder = TimelineBuilder::new("request-1", "turn-1", "message-1", "session-1");
    let err = builder
        .append_delta(
            "missing",
            TalosTimelineDelta::Text {
                text: "orphan".to_string(),
            },
        )
        .expect_err("missing timeline item should fail");

    assert!(err.to_string().contains("timeline item not found"));
}

#[test]
fn timeline_builder_rejects_delta_after_completion() {
    let mut builder = TimelineBuilder::new("request-1", "turn-1", "message-1", "session-1");
    let started = builder.start_commentary();
    let id = item_id(&started);
    builder
        .complete_item(&id, TalosTimelineItemStatus::Completed, None)
        .expect("item should complete");

    let err = builder
        .append_delta(
            &id,
            TalosTimelineDelta::Text {
                text: "late".to_string(),
            },
        )
        .expect_err("completed timeline item should reject delta");

    assert!(err.to_string().contains("timeline item is not active"));
}

#[test]
fn timeline_builder_closes_running_items_before_turn_terminal() {
    let mut builder = TimelineBuilder::new("request-1", "turn-1", "message-1", "session-1");
    let reasoning = builder.start_reasoning();
    let reasoning_id = item_id(&reasoning);
    let summary = builder.start_item(
        TalosTimelineItemKind::ExecutionSummary,
        TalosTimelinePhase::System,
        Some("Execution summary".to_string()),
        TalosTimelineContent::ExecutionSummary {
            summary: "done".to_string(),
        },
    );
    let summary_id = item_id(&summary);

    let completions = builder
        .close_active_items(TalosTimelineItemStatus::Completed)
        .expect("active items should close");

    assert_eq!(completions.len(), 2);
    assert_eq!(
        builder
            .item(&reasoning_id)
            .expect("reasoning exists")
            .status,
        TalosTimelineItemStatus::Completed
    );
    assert_eq!(
        builder.item(&summary_id).expect("summary exists").status,
        TalosTimelineItemStatus::Completed
    );
    assert!(completions
        .iter()
        .all(|event| matches!(event.payload, AgentEventPayload::ItemCompleted(_))));
}

#[test]
fn successful_closure_does_not_complete_active_tool_items() {
    let mut builder = TimelineBuilder::new("request-1", "turn-1", "message-1", "session-1");
    let reasoning_id = item_id(&builder.start_reasoning());
    let tool_id = item_id(&builder.start_tool_call("call-1", "Bash", "{}"));

    let completions = builder
        .close_active_non_tool_items(TalosTimelineItemStatus::Completed)
        .expect("non-tool items should close");

    assert_eq!(completions.len(), 1);
    assert_eq!(
        builder
            .item(&reasoning_id)
            .expect("reasoning exists")
            .status,
        TalosTimelineItemStatus::Completed
    );
    assert_eq!(
        builder.item(&tool_id).expect("tool exists").status,
        TalosTimelineItemStatus::Running
    );
    assert_eq!(builder.active_tool_item_ids(), vec![tool_id]);
}

#[test]
fn timeline_builder_uses_structured_context_budget_turn_errors() {
    let mut builder = TimelineBuilder::new("request-1", "turn-1", "message-1", "session-1");

    let event = builder.fail_turn(&TalosError::ContextWindowExceeded(
        "estimated_tokens=100, effective_prompt_budget=80".to_string(),
    ));

    match event.payload {
        AgentEventPayload::TurnError(payload) => {
            let error_info = payload.error_info.expect("error info should be classified");
            assert_eq!(error_info.code, TalosRunErrorCode::ContextWindowExceeded);
            assert!(!error_info.message.trim().is_empty());
        }
        _ => panic!("expected turn error payload"),
    }
}

#[test]
fn timeline_builder_uses_structured_compaction_blocked_turn_errors() {
    let mut builder = TimelineBuilder::new("request-1", "turn-1", "message-1", "session-1");

    let event = builder.fail_turn(&TalosError::ContextCompactionFailed(
        "session `session-1` remains blocked after compaction".to_string(),
    ));

    match event.payload {
        AgentEventPayload::TurnError(payload) => {
            let error_info = payload.error_info.expect("error info should be classified");
            assert_eq!(error_info.code, TalosRunErrorCode::ContextCompactionFailed);
            assert!(!error_info.message.trim().is_empty());
        }
        _ => panic!("expected turn error payload"),
    }
}

#[test]
fn timeline_builder_leaves_unknown_turn_errors_unclassified() {
    let mut builder = TimelineBuilder::new("request-1", "turn-1", "message-1", "session-1");

    let event = builder.fail_turn_message("provider returned a transient unknown failure");

    match event.payload {
        AgentEventPayload::TurnError(payload) => {
            assert!(payload.error_info.is_none());
        }
        _ => panic!("expected turn error payload"),
    }
}
