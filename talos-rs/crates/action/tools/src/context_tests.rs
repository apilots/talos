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

#[test]
fn test_context_carries_resolved_sandbox_environment() {
    let root = tempfile::tempdir().expect("sandbox root should be created");
    let root = talos_tools_sandbox::AbsolutePathBuf::new(root.path().to_path_buf())
        .expect("sandbox root should be absolute");
    let environment =
        talos_tools_sandbox::ToolSandboxEnvironment::local(vec![root.clone()], Vec::new())
            .expect("sandbox environment should be valid");

    let context =
        ToolContext::new("session-1", "user-1").with_sandbox_environment(environment.clone());

    assert_eq!(context.sandbox_environment(), Some(&environment));
}
