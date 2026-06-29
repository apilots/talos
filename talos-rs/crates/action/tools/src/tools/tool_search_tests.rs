use super::*;
use uuid::Uuid;

#[tokio::test]
async fn tool_search_uses_catalog_snapshot() {
    let session_id = format!("session-{}", Uuid::new_v4());
    let context = ToolContext::new(session_id, "user-1").with_catalog_snapshot(vec![
        CatalogToolEntry {
            public_name: "WebSearch".to_string(),
            description: "Search the web".to_string(),
            deferred: false,
            available: true,
            unavailable_reason: None,
        },
        CatalogToolEntry {
            public_name: "http_request".to_string(),
            description: "Legacy HTTP tool".to_string(),
            deferred: false,
            available: true,
            unavailable_reason: None,
        },
    ]);

    let tool = ToolSearchTool;
    let result = tool
        .execute_payload(
            serde_json::json!({
                "query": "web"
            }),
            context.clone(),
        )
        .await
        .expect("tool_search should succeed");
    let ToolOutputPayload::Function {
        structured_content, ..
    } = result
    else {
        panic!("expected function payload");
    };
    let matches = structured_content
        .as_ref()
        .and_then(|value| value.get("matches"))
        .and_then(|value| value.as_array())
        .expect("structured tool search response should expose matches");
    assert_eq!(matches.len(), 1);
    let matched_tool = matches[0].as_object().expect("tool item should be object");
    assert_eq!(matched_tool["public_name"], "WebSearch");
    assert_eq!(
        context.deferred_tool_state().list(),
        vec!["WebSearch".to_string()]
    );
    let activated = structured_content
        .as_ref()
        .and_then(|value| value.get("activated_tools"))
        .and_then(|value| value.as_array())
        .expect("activated tools should be exposed");
    assert_eq!(activated.len(), 1);
}
