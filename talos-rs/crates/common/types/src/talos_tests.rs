use super::*;

#[test]
fn tool_content_item_rejects_unknown_type() {
    let error = serde_json::from_value::<TalosContentItem>(serde_json::json!({
        "type": "future_item",
        "value": {}
    }))
    .expect_err("unknown content item type should fail fast");

    assert!(error.to_string().contains("future_item"));
}

#[test]
fn semantic_json_values_serialize_as_their_payload_without_an_envelope() {
    let value = serde_json::json!({"memoryId": "memory-1"});

    assert_eq!(
        serde_json::to_value(ToolArguments::from(value.clone())).unwrap(),
        value
    );
}
