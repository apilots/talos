use super::*;

#[test]
fn reduce_tool_execution_output_includes_next_turn_failure_payload() {
    let output = ToolExecutionOutput {
        call_id: "call-1".to_string(),
        tool_name: "Read".to_string(),
        outcome: ToolDispatchOutcome::executed(ToolOutputPayload::Function {
            content: String::new(),
            success: false,
            structured_content: Some(serde_json::json!({
                "error": "path is required",
                "error_kind": "validation_failure",
                "retryable": true,
                "suggested_action": "fix the arguments and retry",
                "gate_source": "tool_semantic"
            })),
            content_items: vec![TalosContentItem::Text {
                text: "path is required".to_string(),
            }],
        }),
        is_error: true,
    };

    let reduced = reduce_tool_execution_output(&output);
    assert!(reduced.is_error);
    assert_eq!(
        reduced.envelope.status,
        talos_tools::ToolResultStatus::Failure
    );
    assert!(reduced
        .semantic_events
        .contains(&"tool_use_error".to_string()));
    let next_turn_failure = reduced
        .next_turn_failure
        .expect("next_turn_failure should be present");
    assert_eq!(
        next_turn_failure.get("strategy").and_then(Value::as_str),
        Some("retry")
    );
    assert_eq!(
        next_turn_failure.get("gate_source").and_then(Value::as_str),
        Some("tool_semantic")
    );
}

#[tokio::test]
async fn emit_tool_result_stream_event_includes_envelope_and_semantic_events() {
    let output = ToolExecutionOutput {
        call_id: "call-2".to_string(),
        tool_name: "Bash".to_string(),
        outcome: ToolDispatchOutcome::executed(ToolOutputPayload::Function {
            content: String::new(),
            success: false,
            structured_content: Some(serde_json::json!({
                "error": "authorization denied",
                "error_kind": "permission_rejection",
                "retryable": false,
                "suggested_action": "choose a less privileged tool or provide a dynamic tool result",
                "gate_source": "allow_list"
            })),
            content_items: vec![TalosContentItem::Text {
                text: "authorization denied".to_string(),
            }],
        }),
        is_error: true,
    };
    let (tx, mut rx) = mpsc::channel(4);

    emit_tool_result_stream_event(Some(&tx), &output).await;
    let event = rx.recv().await.expect("tool result event should exist");

    match event.data {
        KernelStreamEventData::ToolResult(data) => {
            assert!(data.is_error);
            assert_eq!(
                data.semantic_events,
                vec![
                    "tool_use_rejected".to_string(),
                    "tool_use_complete".to_string()
                ]
            );
            assert!(data.envelope.is_some());
            assert_eq!(
                data.next_turn_failure
                    .as_ref()
                    .and_then(|value| value.get("strategy"))
                    .and_then(Value::as_str),
                Some("reject")
            );
        }
        other => panic!("expected tool result event, got {other:?}"),
    }
}

#[test]
fn reduce_tool_execution_output_uses_canonical_success_payload() {
    let output = ToolExecutionOutput {
        call_id: "call-3".to_string(),
        tool_name: "Read".to_string(),
        outcome: ToolDispatchOutcome::executed(ToolOutputPayload::success_text(
            "plain text result",
        )),
        is_error: false,
    };

    let reduced = reduce_tool_execution_output(&output);
    assert!(!reduced.is_error);
    assert_eq!(
        reduced.payload,
        serde_json::json!({
            "type": "text",
            "text": "plain text result",
            "summary": "plain text result"
        })
    );
    assert_eq!(
        reduced.content_items,
        vec![TalosContentItem::Text {
            text: "plain text result".to_string()
        }]
    );
    assert_eq!(
        reduced.memory_payload,
        serde_json::json!({"result": "plain text result"})
    );
}

#[test]
fn reduce_tool_execution_output_rejects_structured_only_success_payload() {
    let output = ToolExecutionOutput {
        call_id: "call-4".to_string(),
        tool_name: "BadTool".to_string(),
        outcome: ToolDispatchOutcome::executed(ToolOutputPayload::Function {
            content: "{}".to_string(),
            success: true,
            structured_content: Some(serde_json::json!({"result": "hidden"})),
            content_items: Vec::new(),
        }),
        is_error: false,
    };

    let reduced = reduce_tool_execution_output(&output);
    assert!(reduced.is_error);
    assert_eq!(
        reduced.content_items,
        vec![TalosContentItem::Text {
            text: "canonical tool result requires content_items".to_string()
        }]
    );
    assert_ne!(
        reduced.payload,
        serde_json::json!({"result": "hidden"}),
        "model projection must not use structured_content as the primary payload"
    );
}
