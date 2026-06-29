use super::*;

#[tokio::test]
async fn test_echo_tool() {
    let tool = EchoTestTool;
    let input = serde_json::json!("hello world");
    let result = tool
        .execute_payload(input, ToolContext::default())
        .await
        .unwrap();

    match result {
        ToolOutputPayload::Function {
            success,
            structured_content,
            ..
        } => {
            assert!(success);
            assert_eq!(
                structured_content,
                Some(serde_json::json!({"result": "hello world"}))
            );
        }
        other => panic!("expected function payload, got {other:?}"),
    }
}
