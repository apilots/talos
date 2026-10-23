use super::*;
use serde_json::json;

#[test]
fn oversized_error_projection_keeps_rejection_and_retry_semantics() {
    let outcome = enforce_output_limits(
        "denied",
        ToolDispatchOutcome::executed(ToolOutputPayload::failure_structured(
            "denied",
            Some(
                json!({"error": "denied", "error_kind": "permission_rejection",
                "retryable": false, "suggested_action": "request permission", "unused": "x".repeat(10000)}),
            ),
        )),
        256,
    );
    let envelope = outcome.envelope();
    assert_eq!(envelope.status, crate::ToolResultStatus::Rejected);
    let error = envelope.error.unwrap();
    assert_eq!(
        error.error_kind,
        crate::TalosToolErrorKind::PermissionRejection
    );
    assert!(!error.retryable);
}

#[test]
fn escaped_error_projection_stays_within_the_serialized_limit() {
    let outcome = enforce_output_limits(
        "escaped-error",
        ToolDispatchOutcome::executed(ToolOutputPayload::failure_structured(
            "failed",
            Some(json!({"error": "\u{0000}".repeat(1000)})),
        )),
        256,
    );
    let ToolOutputPayload::Function {
        structured_content: Some(value),
        ..
    } = outcome.payload().unwrap()
    else {
        panic!("bounded error projection")
    };
    assert!(serde_json::to_vec(value).unwrap().len() <= 256);
    assert!(outcome.is_error());
}

#[test]
fn mcp_error_limit_removes_oversized_duplicate_projections() {
    let content = "e".repeat(20);
    let expected_structured = json!({ "detail": "s".repeat(30) });
    let content_items = vec![talos_types::TalosContentItem::Text {
        text: "i".repeat(20),
    }];
    assert!(content.len() < 80);
    assert!(serde_json::to_vec(&expected_structured).unwrap().len() < 80);
    assert!(serde_json::to_vec(&content_items).unwrap().len() < 80);
    let result = enforce_output_limits(
        "mcp__fixture__fail",
        ToolDispatchOutcome::executed(ToolOutputPayload::Mcp {
            content: content.clone(),
            is_error: true,
            structured_content: Some(expected_structured.clone()),
            content_items,
        }),
        80,
    );

    let ToolOutputPayload::Mcp {
        content,
        structured_content,
        content_items,
        ..
    } = result.payload().unwrap()
    else {
        panic!("expected MCP output")
    };
    assert_eq!(content, &"e".repeat(20));
    assert_eq!(structured_content, &Some(expected_structured));
    assert!(content_items.is_empty());
    assert!(content.len() + serde_json::to_vec(structured_content).unwrap().len() <= 80);
}
