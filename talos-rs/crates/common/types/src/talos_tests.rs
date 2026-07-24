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
fn files_input_uses_one_ordered_file_id_array() {
    let input = TalosInputItem::Files {
        file_ids: vec!["file-2".to_string(), "file-1".to_string()],
    };

    assert_eq!(
        serde_json::to_value(input).expect("files input should serialize"),
        serde_json::json!({
            "type": "files",
            "fileIds": ["file-2", "file-1"]
        })
    );
}

#[test]
fn user_input_artifact_defaults_missing_file_ids_to_empty() {
    let decoded = serde_json::from_value::<RunItem>(serde_json::json!({
        "type": "user_input",
        "text": "hello"
    }))
    .expect("legacy user input artifact should deserialize");

    assert_eq!(
        decoded,
        RunItem::UserInput {
            text: "hello".to_string(),
            file_ids: Vec::new(),
        }
    );
}
