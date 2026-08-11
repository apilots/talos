use super::*;

#[test]
fn test_context_typed_runtime_state() {
    let context = ToolContext::new("session-1", "user-1");

    context.set_dynamic_tool_result(
        "call-1",
        StoredDynamicToolResult {
            output: "ok".to_string(),
            success: true,
        },
    );

    let result = context
        .get_dynamic_tool_result("call-1")
        .expect("missing dynamic result");
    assert_eq!(result.output, "ok");
    assert!(result.success);
}

#[test]
fn test_context_carries_trusted_turn_identity() {
    let context = ToolContext::new("session-1", "user-1").with_turn_id("turn-1");

    assert_eq!(context.turn_id(), Some("turn-1"));
}
