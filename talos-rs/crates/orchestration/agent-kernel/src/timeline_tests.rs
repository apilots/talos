use super::*;

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
