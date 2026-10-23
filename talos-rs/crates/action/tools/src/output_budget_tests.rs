use super::*;

#[test]
fn json_budget_counts_escaping_and_multibyte_text() {
    let value = json!({"text": "中文\n\"\\😀".repeat(20_000)});
    let result = bounded_json(value, 1024);
    assert!(result.len() <= 1024);
    assert!(serde_json::from_str::<Value>(&result).is_ok());
    assert!(result.contains("[truncated]"));
}

#[test]
fn final_bash_projection_keeps_the_error_at_the_end_of_the_preview() {
    let value = json!({"type": "bash_output", "stdout": format!("{}FINAL ERROR", "x".repeat(100000)),
        "stderr": "", "truncated": false});
    let text = bounded_json(value, 512);
    assert!(text.len() <= 512);
    let value: Value = serde_json::from_str(&text).unwrap();
    assert!(value["stdout"].as_str().unwrap().ends_with("FINAL ERROR"));
    assert_eq!(value["truncated"], true);
}

#[test]
fn final_file_and_search_crops_update_their_continuation_metadata() {
    let read = json!({"type": "file_read", "path": "/log", "offset": 0, "limit": 10,
        "total_lines": null, "nextByteOffset": 10000, "content": "x".repeat(10000), "truncated": false});
    let read: Value = serde_json::from_str(&bounded_json(read, 512)).unwrap();
    assert_eq!(read["truncated"], true);
    assert!(read["nextByteOffset"].is_null());
    let search = json!({"type": "search_results", "matches": vec![json!({"text": "x".repeat(100)}); 20],
        "more_available": false, "total_matches": 20});
    let search: Value = serde_json::from_str(&bounded_json(search, 512)).unwrap();
    assert!(search["matches"].as_array().unwrap().len() < 20);
    assert_eq!(search["more_available"], true);
}

#[test]
fn final_serialized_budget_holds_for_opaque_fields_arrays_and_control_characters() {
    let cases = [
        json!({"error": "\u{0000}\"\\中文😀".repeat(10000), "error_kind": "permission_rejection"}),
        json!({"path": "p".repeat(100000), "items": ["opaque"]}),
        json!({"items": vec![json!({"key": "value".repeat(1000)}); 100]}),
    ];
    for budget in [64, 256, 4096, MAX_MODEL_TOOL_BYTES] {
        for value in &cases {
            let text = bounded_json(value.clone(), budget);
            assert!(
                text.len() <= budget,
                "serialized result exceeded {budget} bytes"
            );
            serde_json::from_str::<Value>(&text).expect("valid JSON after limiting");
        }
        let mut items = vec![
            TalosContentItem::Text {
                text: "\u{0000}中文".repeat(1000)
            };
            100
        ];
        limit_content_items(&mut items, budget);
        assert!(serde_json::to_vec(&items).unwrap().len() <= budget);
    }
}

#[test]
fn structured_search_cannot_bypass_the_limit() {
    let mut items = vec![TalosContentItem::SearchResults {
        query: Some("needle".into()),
        matches: vec![talos_types::talos::TalosToolSearchMatch {
            path: "source.rs".into(),
            line: 1,
            column: None,
            text: "long match".repeat(100_000),
            before: Vec::new(),
            after: Vec::new(),
        }],
        offset: 0,
        limit: 100,
        total_matches: Some(1),
        more_available: false,
        partial: false,
    }];
    limit_content_items(&mut items, 2048);
    assert!(serde_json::to_vec(&items).unwrap().len() <= 2048);
    assert!(matches!(
        items[0],
        TalosContentItem::SearchResults {
            more_available: true,
            ..
        }
    ));
}
