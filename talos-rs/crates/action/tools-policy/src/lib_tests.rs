use super::*;

#[test]
fn test_allow_policy() {
    let policy = ToolPolicy::allow_list(vec!["read", "write"]);

    assert!(policy.is_allowed("read"));
    assert!(policy.is_allowed("write"));
    assert!(!policy.is_allowed("exec"));
    assert!(!policy.is_allowed("delete"));
}

#[test]
fn test_deny_policy() {
    let policy = ToolPolicy::deny_list(vec!["exec", "delete"]);

    assert!(policy.is_allowed("read"));
    assert!(policy.is_allowed("write"));
    assert!(!policy.is_allowed("exec"));
    assert!(!policy.is_allowed("delete"));
}

#[test]
fn test_disabled_policy() {
    let mut policy = ToolPolicy::allow_list(vec!["read"]);
    policy.disable();

    assert!(policy.is_allowed("read")); // 读取被允许，因为策略已禁用
    assert!(policy.is_allowed("exec")); // 执行也被允许，因为策略已禁用
}

#[test]
fn test_policy_modification() {
    let mut policy = ToolPolicy::allow_list(vec![]);

    // 添加工具
    policy.add_tool("read");
    assert!(policy.is_allowed("read"));

    // 移除工具
    policy.remove_tool("read");
    assert!(!policy.is_allowed("read"));
}

#[test]
fn test_policy_checker() {
    let policy = ToolPolicy::allow_list(vec!["read", "write"]);
    let checker = ToolPolicyChecker::new(policy);

    // Allowed tools.
    assert!(checker.check_tool_access("read").is_ok());
    assert!(checker.check_tool_access("write").is_ok());

    // Denied tool.
    assert!(checker.check_tool_access("exec").is_err());
}

#[test]
fn test_batch_check() {
    let policy = ToolPolicy::allow_list(vec!["read", "write"]);
    let checker = ToolPolicyChecker::new(policy);

    // All tools are allowed.
    assert!(checker
        .check_tools_access(&["read".to_string(), "write".to_string()])
        .is_ok());

    // One tool is denied.
    assert!(checker
        .check_tools_access(&["read".to_string(), "exec".to_string()])
        .is_err());
}

#[test]
fn test_default_policy_allows_builtin_tools() {
    let policy = ToolPolicy::default();

    assert!(policy.is_allowed("MemorySearch"));
    assert!(policy.is_allowed("MemoryGet"));
    assert!(policy.is_allowed("Bash"));
    assert!(policy.is_allowed("Read"));
    assert!(policy.is_allowed("Grep"));
    for tool in [
        "spawn_agent",
        "send_message",
        "followup_task",
        "wait_agent",
        "interrupt_agent",
        "list_agents",
    ] {
        assert!(policy.is_allowed(tool));
    }
    assert!(!policy.is_allowed("Agent"));
    assert!(!policy.is_allowed("Skill"));
}
