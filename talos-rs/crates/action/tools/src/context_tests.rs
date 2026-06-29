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
fn test_context_reuses_session_state() {
    let context_a = ToolContext::new("shared-session", "user-1");
    let context_b = ToolContext::new("shared-session", "user-1");

    context_a.set_permission_mode(PermissionMode::Plan);

    assert_eq!(context_b.permission_mode(), PermissionMode::Plan);
}
