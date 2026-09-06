use super::TalosToolErrorKind;
use super::*;

#[test]
fn tool_result_envelope_preserves_structured_success_payload() {
    let envelope =
        ToolOutputPayload::success_structured(serde_json::json!({"result": "ok"})).envelope();
    assert_eq!(envelope.status, ToolResultStatus::Success);
    assert_eq!(
        envelope.structured_content,
        Some(serde_json::json!({"result": "ok"}))
    );
}

#[test]
fn tool_result_envelope_wraps_plain_text_success_payload() {
    let envelope = ToolOutputPayload::success_text("plain text result").envelope();
    assert_eq!(envelope.status, ToolResultStatus::Success);
    assert_eq!(
        envelope.structured_content,
        Some(serde_json::json!({"result": "plain text result"}))
    );
    assert_eq!(
        envelope.content_items,
        vec![TalosContentItem::Text {
            text: "plain text result".to_string()
        }]
    );
    assert_eq!(
        envelope.compact_summary.as_deref(),
        Some("plain text result")
    );
}

#[test]
fn function_payload_rejects_structured_content_without_content_items() {
    let payload = ToolOutputPayload::Function {
        content: "{}".to_string(),
        success: true,
        structured_content: Some(serde_json::json!({"result": "hidden"})),
        content_items: Vec::new(),
    };

    assert!(payload.validate_canonical_result().is_err());
}

#[test]
fn tool_content_item_serializes_as_flat_json() {
    let value = serde_json::to_value(TalosContentItem::Text {
        text: "hello".to_string(),
    })
    .expect("content item should serialize");

    assert_eq!(value, serde_json::json!({"type": "text", "text": "hello"}));
    let web_reference = serde_json::to_value(TalosContentItem::WebReference {
        url: "https://example.com".to_string(),
        title: Some("Example".to_string()),
        citation_id: Some("web-1".to_string()),
        snippet: Some("Example snippet".to_string()),
        source_domain: Some("example.com".to_string()),
        published_at: Some("2026-06-15".to_string()),
        provider_rank: Some(1),
    })
    .expect("web reference should serialize");

    assert_eq!(
        web_reference,
        serde_json::json!({
            "type": "web_reference",
            "url": "https://example.com",
            "title": "Example",
            "citation_id": "web-1",
            "snippet": "Example snippet",
            "source_domain": "example.com",
            "published_at": "2026-06-15",
            "provider_rank": 1
        })
    );
}

#[test]
fn tool_dispatch_outcome_pending_maps_to_pending_meta() {
    let outcome = ToolDispatchOutcome::pending_dynamic(
        "ask_user",
        serde_json::json!({"question": "continue?"}),
    );

    let envelope = outcome.envelope();
    assert_eq!(envelope.status, ToolResultStatus::Pending);
    let pending = envelope.pending.expect("pending metadata should exist");
    assert_eq!(pending.pending_kind, "dynamic");
    assert_eq!(pending.prompt, None);
}

#[test]
fn structured_error_builds_next_turn_failure_payload() {
    let envelope = ToolOutputPayload::Function {
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
    }
    .envelope();
    assert_eq!(envelope.status, ToolResultStatus::Failure);
    let next_turn_failure = envelope
        .next_turn_failure_payload()
        .expect("next turn payload should exist");
    assert_eq!(
        next_turn_failure.error_kind,
        TalosToolErrorKind::ValidationFailure
    );
    assert!(next_turn_failure.retryable);
    assert_eq!(next_turn_failure.strategy, "retry");
    assert_eq!(
        next_turn_failure.gate_source.as_deref(),
        Some("tool_semantic")
    );
}

#[test]
fn unstructured_error_message_defaults_to_runtime_failure() {
    let error = ToolResultError::from_message("path is required");

    assert_eq!(error.error_kind, TalosToolErrorKind::RuntimeFailure);
    assert!(error.retryable);
}
