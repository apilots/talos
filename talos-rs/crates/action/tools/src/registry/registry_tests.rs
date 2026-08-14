use super::*;
use crate::register_core_local_tools;
use crate::trait_impl::EchoTestTool;
use crate::{
    AbsolutePathBuf, MemoryGetTool, MemorySearchTool, ToolPending, ToolSandboxEnvironment,
    ToolSearchTool,
};
use async_trait::async_trait;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use talos_config::{AgentKernelConfig, ToolSandboxModeConfig};
use talos_error::{TalosError, TalosToolErrorKind};
use talos_tools_policy::ToolPolicy;
use talos_types::{
    SandboxApprovalDecision, SandboxApprovalPresentation, SandboxFileOperation, UserActionResponse,
};

struct LargeErrorTool;
struct ValidationGateTool {
    executes: Arc<AtomicUsize>,
}
struct PermissionGateTool {
    executes: Arc<AtomicUsize>,
}
struct FakeMcpHandler;

fn assert_tool_failure(
    error: TalosError,
    expected_kind: TalosToolErrorKind,
    expected_gate_source: &str,
    expected_fragment: &str,
) {
    match error {
        TalosError::ToolFailure {
            kind,
            message,
            gate_source,
        } => {
            assert_eq!(kind, expected_kind);
            assert_eq!(gate_source.as_deref(), Some(expected_gate_source));
            assert!(
                message.contains(expected_fragment),
                "missing expected fragment '{expected_fragment}': {message}"
            );
        }
        other => panic!("expected ToolFailure, got {other:?}"),
    }
}

fn test_kernel_config_with_output_limit(limit: usize) -> Arc<AgentKernelConfig> {
    Arc::new(AgentKernelConfig {
        tool_result_max_bytes: limit,
        ..Default::default()
    })
}

fn full_access_kernel_config() -> Arc<AgentKernelConfig> {
    let mut config = AgentKernelConfig::default();
    config.tools.sandbox_policy.default_mode = ToolSandboxModeConfig::DangerFullAccess;
    Arc::new(config)
}

fn workspace_write_kernel_config() -> Arc<AgentKernelConfig> {
    let mut config = AgentKernelConfig::default();
    config.tools.sandbox_policy.user_permission_mode =
        talos_config::ToolUserPermissionModeConfig::AskForApproval;
    config.tools.sandbox_policy.default_mode = ToolSandboxModeConfig::WorkspaceWrite;
    Arc::new(config)
}

fn local_tool_context(
    session_id: &str,
    user_id: &str,
    config: Arc<AgentKernelConfig>,
    root: &std::path::Path,
) -> ToolContext {
    let root = root
        .canonicalize()
        .expect("sandbox root should canonicalize");
    let root = AbsolutePathBuf::new(root).expect("sandbox root should be absolute");
    let project_root = root.as_path().to_path_buf();
    let environment = ToolSandboxEnvironment::local(vec![root], Vec::new())
        .expect("sandbox environment should be valid");
    ToolContext::new_with_kernel_config(session_id, user_id, Some(config))
        .with_project_root(project_root)
        .with_sandbox_environment(environment)
}

#[async_trait]
impl Tool for LargeErrorTool {
    fn name(&self) -> &str {
        "large_error"
    }

    fn description(&self) -> &str {
        "Returns a large error string for testing"
    }

    async fn execute_payload(
        &self,
        _input: Value,
        _context: ToolContext,
    ) -> TalosResult<ToolOutputPayload> {
        Ok(ToolOutputPayload::failure_structured(
            String::new(),
            Some(json!({ "error": "e".repeat(256) })),
        ))
    }
}

#[async_trait]
impl Tool for ValidationGateTool {
    fn name(&self) -> &str {
        "validation_gate"
    }

    fn description(&self) -> &str {
        "Fails validation before execute"
    }

    fn validate_input(&self, _input: &Value) -> TalosResult<()> {
        Err(TalosError::ValidationError(
            "validation gate rejected input".to_string(),
        ))
    }

    async fn execute_payload(
        &self,
        _input: Value,
        _context: ToolContext,
    ) -> TalosResult<ToolOutputPayload> {
        self.executes.fetch_add(1, Ordering::SeqCst);
        Ok(ToolOutputPayload::success_text("should not run"))
    }
}

#[async_trait]
impl Tool for PermissionGateTool {
    fn name(&self) -> &str {
        "permission_gate"
    }

    fn description(&self) -> &str {
        "Fails semantic permission before execute"
    }

    fn check_permissions(&self, _input: &Value, _context: &ToolContext) -> TalosResult<()> {
        Err(TalosError::AuthorizationDenied)
    }

    async fn execute_payload(
        &self,
        _input: Value,
        _context: ToolContext,
    ) -> TalosResult<ToolOutputPayload> {
        self.executes.fetch_add(1, Ordering::SeqCst);
        Ok(ToolOutputPayload::success_text("should not run"))
    }
}

#[async_trait]
impl ToolHandler for FakeMcpHandler {
    fn kind(&self) -> ToolSource {
        ToolSource::Mcp
    }

    fn matches(&self, _invocation: &ToolInvocation) -> bool {
        true
    }

    async fn handle(
        &self,
        _invocation: ToolInvocation,
        _context: ToolDispatchContext,
    ) -> TalosResult<ToolDispatchOutcome> {
        Ok(ToolDispatchOutcome::executed(
            ToolOutputPayload::success_text("mcp ok"),
        ))
    }
}

#[tokio::test]
async fn test_registry_register_and_execute() {
    let registry = ToolRegistry::new();
    let echo_tool = Arc::new(EchoTestTool);
    registry.register(echo_tool);

    let input = serde_json::json!("hello");
    let invocation = registry
        .build_function_invocation("call-register", "echo_test", input)
        .unwrap();
    let result = registry
        .execute_invocation_with_context(invocation, ToolContext::default())
        .await
        .unwrap();

    assert!(!result.is_error());
    assert_eq!(result.content(), "hello");
}

#[tokio::test]
async fn test_registry_tool_not_found() {
    let registry = ToolRegistry::new();
    let input = serde_json::json!("test");
    let result = registry.build_function_invocation("call-missing", "nonexistent_tool", input);

    assert!(result.is_err());
}

#[tokio::test]
async fn test_registry_policy_enforcement() {
    let policy = ToolPolicy::allow_list(vec!["read"]);
    let registry = ToolRegistry::new_with_policy(policy);

    let echo_tool = Arc::new(EchoTestTool);
    registry.register(echo_tool);

    let input = serde_json::json!("hello");
    let invocation = registry
        .build_function_invocation("call-policy", "echo_test", input)
        .unwrap();
    let result = registry
        .execute_invocation_with_context(invocation, ToolContext::default())
        .await;

    assert_tool_failure(
        result.unwrap_err(),
        TalosToolErrorKind::PermissionRejection,
        GATE_SOURCE_ALLOW_LIST,
        "authorization denied",
    );
}

#[tokio::test]
async fn test_registry_policy_allowed_tool() {
    let policy = ToolPolicy::allow_list(vec!["echo_test"]);
    let registry = ToolRegistry::new_with_policy(policy);

    let echo_tool = Arc::new(EchoTestTool);
    registry.register(echo_tool);

    let input = serde_json::json!("hello");
    let invocation = registry
        .build_function_invocation("call-allowed", "echo_test", input)
        .unwrap();
    let result = registry
        .execute_invocation_with_context(invocation, ToolContext::default())
        .await
        .unwrap();

    assert!(!result.is_error());
    assert_eq!(result.content(), "hello");
}

#[tokio::test]
async fn test_mcp_tools_bypass_static_allow_list() {
    let policy = ToolPolicy::allow_list(vec!["Read"]);
    let registry = ToolRegistry::new_with_policy(policy);
    let tool_name = "mcp__playwright__browser_navigate";
    let spec = RegisteredToolSpec::new(
        ToolDefinition::new(tool_name, "MCP browser navigation", Some(json!({}))),
        ToolSource::Mcp,
        crate::spec::ToolPayloadKind::Mcp,
        crate::spec::ToolParallelism::ParallelSafe,
        false,
        default_governance_for_source(ToolSource::Mcp),
    );
    registry.register_handler(tool_name, spec, Arc::new(FakeMcpHandler));

    assert!(registry
        .list_allowed_tool_names()
        .contains(&tool_name.to_string()));
    assert!(registry.is_tool_available(tool_name));

    let invocation = registry
        .build_function_invocation(
            "call-mcp",
            tool_name,
            json!({ "url": "https://news.ycombinator.com/news" }),
        )
        .expect("MCP invocation should route through the registry");
    let result = registry
        .execute_invocation_with_context(invocation, ToolContext::default())
        .await
        .expect("MCP source tools should bypass static allow-list");

    assert_eq!(result.content(), "mcp ok");
}

#[tokio::test]
async fn test_list_allowed_tools() {
    let policy = ToolPolicy::allow_list(vec!["read", "write"]);
    let registry = ToolRegistry::new_with_policy(policy);

    registry.register(Arc::new(EchoTestTool));

    let allowed_tools = registry.list_allowed_tool_names();
    assert!(!allowed_tools.contains(&"echo_test".to_string()));
}

#[tokio::test]
async fn test_is_tool_available() {
    let policy = ToolPolicy::allow_list(vec!["read"]);
    let registry = ToolRegistry::new_with_policy(policy);

    registry.register(Arc::new(EchoTestTool));
    assert!(!registry.is_tool_available("echo_test"));

    let policy_allow_echo = ToolPolicy::allow_list(vec!["echo_test"]);
    let registry2 = ToolRegistry::new_with_policy(policy_allow_echo);
    registry2.register(Arc::new(EchoTestTool));
    assert!(registry2.is_tool_available("echo_test"));
}

#[tokio::test]
async fn test_registry_execute_invocation_with_context() {
    let policy = ToolPolicy::allow_list(vec!["echo_test"]);
    let registry = ToolRegistry::new_with_policy(policy);
    registry.register(Arc::new(EchoTestTool));

    let context = ToolContext::new("session-ctx", "user-ctx");
    let invocation = registry
        .build_function_invocation("call-ctx", "echo_test", serde_json::json!("hello"))
        .unwrap();

    let result = registry
        .execute_invocation_with_context(invocation, context)
        .await
        .unwrap();
    assert!(!result.is_error());
    assert_eq!(result.content(), "hello");
}

#[tokio::test]
async fn test_registry_pre_execute_validation_gate_runs_before_execute() {
    let executes = Arc::new(AtomicUsize::new(0));
    let tool = Arc::new(ValidationGateTool {
        executes: executes.clone(),
    });
    let policy = ToolPolicy::allow_list(vec!["validation_gate"]);
    let registry = ToolRegistry::new_with_policy(policy);
    registry.register(tool);

    let invocation = registry
        .build_function_invocation(
            "call-validation-gate",
            "validation_gate",
            serde_json::json!({"path": "demo.txt"}),
        )
        .unwrap();

    let error = registry
        .execute_invocation_with_context(invocation, ToolContext::default())
        .await
        .unwrap_err();

    assert_tool_failure(
        error,
        TalosToolErrorKind::ValidationFailure,
        GATE_SOURCE_TOOL_SEMANTIC,
        "validation gate rejected input",
    );
    assert_eq!(executes.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn test_registry_pre_execute_permission_gate_runs_after_allow_list() {
    let executes = Arc::new(AtomicUsize::new(0));
    let tool = Arc::new(PermissionGateTool {
        executes: executes.clone(),
    });
    let policy = ToolPolicy::allow_list(vec!["permission_gate"]);
    let registry = ToolRegistry::new_with_policy(policy);
    registry.register(tool);

    let invocation = registry
        .build_function_invocation(
            "call-permission-gate",
            "permission_gate",
            serde_json::json!({"path": "demo.txt"}),
        )
        .unwrap();

    let error = registry
        .execute_invocation_with_context(invocation, ToolContext::default())
        .await
        .unwrap_err();

    assert_tool_failure(
        error,
        TalosToolErrorKind::PermissionRejection,
        GATE_SOURCE_TOOL_SEMANTIC,
        "authorization denied",
    );
    assert_eq!(executes.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn test_read_tool_validation_returns_structured_failure() {
    let policy = ToolPolicy::allow_list(vec!["Read"]);
    let registry = ToolRegistry::new_with_policy(policy);
    register_core_local_tools(&registry);

    let invocation = registry
        .build_function_invocation("call-read-validation", "Read", serde_json::json!({}))
        .unwrap();

    let error = registry
        .execute_invocation_with_context(invocation, ToolContext::default())
        .await
        .unwrap_err();

    assert_tool_failure(
        error,
        TalosToolErrorKind::ValidationFailure,
        GATE_SOURCE_TOOL_SEMANTIC,
        "path is required",
    );
}

#[tokio::test]
async fn test_edit_tool_validation_returns_structured_failure() {
    let policy = ToolPolicy::allow_list(vec!["Edit"]);
    let registry = ToolRegistry::new_with_policy(policy);
    register_core_local_tools(&registry);

    let invocation = registry
        .build_function_invocation(
            "call-edit-validation",
            "Edit",
            serde_json::json!({"path": "demo.txt"}),
        )
        .unwrap();

    let error = registry
        .execute_invocation_with_context(invocation, ToolContext::default())
        .await
        .unwrap_err();

    assert_tool_failure(
        error,
        TalosToolErrorKind::ValidationFailure,
        GATE_SOURCE_TOOL_SEMANTIC,
        "find/old_string is required",
    );
}

#[tokio::test]
async fn sandbox_approval_allow_once_replays_the_same_bash_invocation_once() {
    let workspace = tempfile::tempdir().expect("temp dir should create");
    let workspace_path = std::fs::canonicalize(workspace.path()).expect("workspace should resolve");
    let marker = workspace_path.join("approval-marker");
    let registry = ToolRegistry::new_with_policy(ToolPolicy::allow_list(vec!["Bash"]));
    register_core_local_tools(&registry);
    let invocation = registry
        .build_function_invocation(
            "call-bash-approval",
            "Bash",
            serde_json::json!({
                "command": format!("printf x >> '{}'", marker.display())
            }),
        )
        .expect("Bash invocation should build");
    let context = local_tool_context(
        "session-bash-approval",
        "user-bash-approval",
        workspace_write_kernel_config(),
        &workspace_path,
    );

    let pending = registry
        .execute_invocation_with_context(invocation.clone(), context.clone())
        .await
        .expect("approval should become pending");
    let ToolDispatchOutcome::Pending {
        pending:
            ToolPending::SandboxApproval {
                params,
                request_fingerprint,
            },
        ..
    } = pending
    else {
        panic!("Bash escalation should request sandbox approval");
    };
    assert_eq!(
        params.available_decisions,
        vec![
            SandboxApprovalDecision::AllowOnce,
            SandboxApprovalDecision::Deny,
            SandboxApprovalDecision::Cancel,
        ]
    );
    assert!(!marker.exists());

    context.set_sandbox_approval_fingerprint("call-bash-approval", request_fingerprint);
    context.set_user_action_response(
        "call-bash-approval",
        UserActionResponse::SandboxApproval(SandboxApprovalDecision::AllowOnce),
    );
    let outcome = registry
        .execute_invocation_with_context(invocation, context)
        .await
        .expect("matching one-time approval should execute");

    let ToolDispatchOutcome::Executed { payload } = outcome else {
        panic!("approved Bash invocation should execute");
    };
    assert!(
        matches!(&payload, ToolOutputPayload::Function { success: true, .. }),
        "approved Bash invocation failed: {payload:?}"
    );
    assert!(
        marker.exists(),
        "approved Bash side effect is missing: {payload:?}"
    );
    assert_eq!(std::fs::read_to_string(marker).unwrap(), "x");
}

#[tokio::test]
async fn sandbox_approval_rejects_a_changed_scope_without_side_effect() {
    let workspace = tempfile::tempdir().expect("temp dir should create");
    let marker = workspace.path().join("must-not-exist");
    let registry = ToolRegistry::new_with_policy(ToolPolicy::allow_list(vec!["Bash"]));
    register_core_local_tools(&registry);
    let invocation = registry
        .build_function_invocation(
            "call-bash-scope",
            "Bash",
            serde_json::json!({
                "command": format!("touch '{}'", marker.display()),
                "dangerously_disable_sandbox": true
            }),
        )
        .expect("Bash invocation should build");
    let context = local_tool_context(
        "session-bash-scope",
        "user-bash-scope",
        workspace_write_kernel_config(),
        workspace.path(),
    );
    context.set_sandbox_approval_fingerprint("call-bash-scope", "sar_wrong_scope");
    context.set_user_action_response(
        "call-bash-scope",
        UserActionResponse::SandboxApproval(SandboxApprovalDecision::AllowOnce),
    );

    let error = registry
        .execute_invocation_with_context(invocation, context)
        .await
        .expect_err("changed approval scope should fail closed");

    assert_tool_failure(
        error,
        TalosToolErrorKind::PermissionRejection,
        "sandbox_policy",
        "scope does not match",
    );
    assert!(!marker.exists());
}

#[tokio::test]
async fn sandbox_approval_deny_returns_a_tool_failure_without_execution() {
    let workspace = tempfile::tempdir().expect("temp dir should create");
    let marker = workspace.path().join("must-not-exist");
    let registry = ToolRegistry::new_with_policy(ToolPolicy::allow_list(vec!["Bash"]));
    register_core_local_tools(&registry);
    let invocation = registry
        .build_function_invocation(
            "call-bash-deny",
            "Bash",
            serde_json::json!({
                "command": format!("touch '{}'", marker.display()),
                "dangerously_disable_sandbox": true
            }),
        )
        .expect("Bash invocation should build");
    let context = local_tool_context(
        "session-bash-deny",
        "user-bash-deny",
        workspace_write_kernel_config(),
        workspace.path(),
    );
    context.set_user_action_response(
        "call-bash-deny",
        UserActionResponse::SandboxApproval(SandboxApprovalDecision::Deny),
    );

    let outcome = registry
        .execute_invocation_with_context(invocation, context)
        .await
        .expect("denial should be returned to the model as a tool result");
    let ToolDispatchOutcome::Executed {
        payload: ToolOutputPayload::Function { success, .. },
    } = outcome
    else {
        panic!("denial should produce a function tool result");
    };
    assert!(!success);
    assert!(!marker.exists());
}

#[tokio::test]
async fn test_registry_binds_full_access_permit_for_bash() {
    let workspace = tempfile::tempdir().expect("temp dir should create");
    let registry = ToolRegistry::new_with_policy(ToolPolicy::allow_list(vec!["Bash"]));
    register_core_local_tools(&registry);
    let invocation = registry
        .build_function_invocation(
            "call-bash-sandbox-permit",
            "Bash",
            serde_json::json!({"command": "printf sandbox-permit-bound"}),
        )
        .expect("Bash invocation should build");
    let context = local_tool_context(
        "session-bash-sandbox",
        "user-bash-sandbox",
        full_access_kernel_config(),
        workspace.path(),
    );

    let outcome = registry
        .execute_invocation_with_context(invocation, context)
        .await
        .expect("Bash should execute through the bound permit");
    let ToolDispatchOutcome::Executed { payload } = outcome else {
        panic!("Bash should execute synchronously");
    };
    assert!(payload.content().contains("sandbox-permit-bound"));
}

#[tokio::test]
async fn test_registry_binds_full_access_permit_for_grep() {
    let workspace = tempfile::tempdir().expect("temp dir should create");
    std::fs::write(workspace.path().join("sample.txt"), "sandbox grep target\n")
        .expect("fixture should write");
    let registry = ToolRegistry::new_with_policy(ToolPolicy::allow_list(vec!["Grep"]));
    register_core_local_tools(&registry);
    let invocation = registry
        .build_function_invocation(
            "call-grep-sandbox-permit",
            "Grep",
            serde_json::json!({
                "pattern": "sandbox grep target",
                "path": ".",
                "output_mode": "content"
            }),
        )
        .expect("Grep invocation should build");
    let context = local_tool_context(
        "session-grep-sandbox",
        "user-grep-sandbox",
        full_access_kernel_config(),
        workspace.path(),
    );

    let outcome = registry
        .execute_invocation_with_context(invocation, context)
        .await
        .expect("Grep should execute through the bound permit");
    let ToolDispatchOutcome::Executed { payload } = outcome else {
        panic!("Grep should execute synchronously");
    };
    assert!(payload.content().contains("sandbox grep target"));
}

#[tokio::test]
async fn test_registry_binds_workspace_write_permit_for_file_tool() {
    let workspace = tempfile::tempdir().expect("temp dir should create");
    let target = workspace.path().join("nested/output.txt");
    let registry = ToolRegistry::new_with_policy(ToolPolicy::allow_list(vec!["Write"]));
    register_core_local_tools(&registry);
    let invocation = registry
        .build_function_invocation(
            "call-write-sandbox-permit",
            "Write",
            serde_json::json!({"path": target, "content": "workspace write"}),
        )
        .expect("Write invocation should build");
    let context = local_tool_context(
        "session-write-sandbox",
        "user-write-sandbox",
        workspace_write_kernel_config(),
        workspace.path(),
    );

    registry
        .execute_invocation_with_context(invocation, context)
        .await
        .expect("workspace Write should execute through the bound permit");
    assert_eq!(
        std::fs::read_to_string(target).expect("workspace output should exist"),
        "workspace write"
    );
}

#[tokio::test]
async fn test_registry_approves_file_write_outside_workspace_before_side_effect() {
    let parent = tempfile::tempdir().expect("temp dir should create");
    let workspace = parent.path().join("workspace");
    std::fs::create_dir(&workspace).expect("workspace should create");
    let marker = parent.path().join("must-not-exist");
    let registry = ToolRegistry::new_with_policy(ToolPolicy::allow_list(vec!["Write"]));
    register_core_local_tools(&registry);
    let invocation = registry
        .build_function_invocation(
            "call-write-outside-workspace",
            "Write",
            serde_json::json!({"path": marker, "content": "denied"}),
        )
        .expect("Write invocation should build");
    let context = local_tool_context(
        "session-write-denied",
        "user-write-denied",
        workspace_write_kernel_config(),
        &workspace,
    );

    let pending = registry
        .execute_invocation_with_context(invocation.clone(), context.clone())
        .await
        .expect("outside write should request approval");
    let ToolDispatchOutcome::Pending {
        pending:
            ToolPending::SandboxApproval {
                params,
                request_fingerprint,
            },
        ..
    } = pending
    else {
        panic!("outside write should request sandbox approval");
    };
    assert_eq!(
        params.presentation,
        SandboxApprovalPresentation::FileSystem {
            operation: SandboxFileOperation::Write,
            paths: vec![std::fs::canonicalize(parent.path())
                .expect("outside parent should resolve")
                .display()
                .to_string()],
        }
    );
    assert!(!marker.exists());

    context.set_sandbox_approval_fingerprint(
        "call-write-outside-workspace",
        request_fingerprint.clone(),
    );
    context.set_user_action_response(
        "call-write-outside-workspace",
        UserActionResponse::SandboxApproval(SandboxApprovalDecision::Cancel),
    );
    let cancelled = registry
        .execute_invocation_with_context(invocation.clone(), context.clone())
        .await
        .expect_err("cancelled outside write should stop execution");
    assert!(cancelled.to_string().contains("approval was cancelled"));
    assert!(!marker.exists());

    context.set_sandbox_approval_fingerprint(
        "call-write-outside-workspace",
        request_fingerprint.clone(),
    );
    context.set_user_action_response(
        "call-write-outside-workspace",
        UserActionResponse::SandboxApproval(SandboxApprovalDecision::Deny),
    );
    let denied = registry
        .execute_invocation_with_context(invocation.clone(), context.clone())
        .await
        .expect("denied outside write should return a tool result");
    assert!(matches!(denied, ToolDispatchOutcome::Executed { .. }));
    assert!(!marker.exists());

    context.set_sandbox_approval_fingerprint("call-write-outside-workspace", request_fingerprint);
    context.set_user_action_response(
        "call-write-outside-workspace",
        UserActionResponse::SandboxApproval(SandboxApprovalDecision::AllowOnce),
    );
    registry
        .execute_invocation_with_context(invocation, context)
        .await
        .expect("approved outside write should execute");
    assert_eq!(
        std::fs::read_to_string(marker).expect("approved marker should exist"),
        "denied"
    );
}

#[tokio::test]
async fn test_registry_requests_scoped_approval_for_file_read_outside_workspace() {
    let parent = tempfile::tempdir().expect("temp dir should create");
    let workspace = parent.path().join("workspace");
    std::fs::create_dir(&workspace).expect("workspace should create");
    let outside = parent.path().join("outside.txt");
    std::fs::write(&outside, "outside").expect("fixture should write");
    let registry = ToolRegistry::new_with_policy(ToolPolicy::allow_list(vec!["Read"]));
    register_core_local_tools(&registry);
    let invocation = registry
        .build_function_invocation(
            "call-read-outside-workspace",
            "Read",
            serde_json::json!({"path": outside}),
        )
        .expect("Read invocation should build");
    let context = local_tool_context(
        "session-read-denied",
        "user-read-denied",
        workspace_write_kernel_config(),
        &workspace,
    );

    let outcome = registry
        .execute_invocation_with_context(invocation.clone(), context.clone())
        .await
        .expect("outside read should request approval");
    let ToolDispatchOutcome::Pending {
        pending:
            ToolPending::SandboxApproval {
                params,
                request_fingerprint,
            },
        ..
    } = outcome
    else {
        panic!("outside read should request sandbox approval");
    };
    assert_eq!(
        params.presentation,
        SandboxApprovalPresentation::FileSystem {
            operation: SandboxFileOperation::Read,
            paths: vec![std::fs::canonicalize(outside)
                .expect("outside file should resolve")
                .display()
                .to_string()],
        }
    );

    context.set_sandbox_approval_fingerprint("call-read-outside-workspace", request_fingerprint);
    context.set_user_action_response(
        "call-read-outside-workspace",
        UserActionResponse::SandboxApproval(SandboxApprovalDecision::AllowOnce),
    );
    let approved = registry
        .execute_invocation_with_context(invocation, context)
        .await
        .expect("approved outside read should execute");
    assert!(matches!(approved, ToolDispatchOutcome::Executed { .. }));
}

#[tokio::test]
async fn test_outer_allow_list_rejects_before_tool_dispatch() {
    let allow_list_registry = ToolRegistry::new_with_policy(ToolPolicy::allow_list(vec!["Read"]));
    register_core_local_tools(&allow_list_registry);
    let allow_list_invocation = allow_list_registry
        .build_function_invocation(
            "call-bash-allow-list",
            "Bash",
            serde_json::json!({"command": "echo hi"}),
        )
        .unwrap();

    let allow_list_error = allow_list_registry
        .execute_invocation_with_context(allow_list_invocation, ToolContext::default())
        .await
        .unwrap_err();
    assert_tool_failure(
        allow_list_error,
        TalosToolErrorKind::PermissionRejection,
        GATE_SOURCE_ALLOW_LIST,
        "authorization denied",
    );
}

#[tokio::test]
async fn test_default_registry_allows_memory_search() {
    let registry = ToolRegistry::new();
    registry.register(Arc::new(MemorySearchTool));

    let workspace_dir = std::env::temp_dir().join(format!(
        "talos-memory-registry-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time should be valid")
            .as_nanos()
    ));
    tokio::fs::create_dir_all(&workspace_dir)
        .await
        .expect("workspace dir should create");
    tokio::fs::write(workspace_dir.join("MEMORY.md"), "Search target memory\n")
        .await
        .expect("root memory should write");

    let config = AgentKernelConfig {
        workspace_dir: workspace_dir.to_string_lossy().to_string(),
        ..Default::default()
    };
    let context =
        ToolContext::new_with_kernel_config("session-ctx", "user-ctx", Some(Arc::new(config)));
    let invocation = registry
        .build_function_invocation(
            "call-memory",
            "MemorySearch",
            serde_json::json!({ "query": "target" }),
        )
        .unwrap();

    let error = registry
        .execute_invocation_with_context(invocation, context)
        .await
        .expect_err("MemorySearch should fail without injected memory store");

    assert!(matches!(error, talos_error::TalosError::ToolError(_)));
}

#[test]
fn test_get_tool_definitions() {
    let registry = ToolRegistry::new();
    registry.register(Arc::new(EchoTestTool));
    let defs = registry.get_tool_definitions();
    assert_eq!(defs.len(), 1);
    assert_eq!(defs[0].name, "echo_test");
}

#[test]
fn test_get_tool_definitions_uses_stable_priority_order() {
    use crate::{BashTool, GlobTool, GrepTool, ReadTool};

    let registry = ToolRegistry::new();
    registry.register(Arc::new(GlobTool));
    registry.register(Arc::new(BashTool));
    registry.register(Arc::new(ReadTool));
    registry.register(Arc::new(GrepTool));

    let defs = registry.get_tool_definitions();
    let names = defs.iter().map(|def| def.name.as_str()).collect::<Vec<_>>();
    assert_eq!(names, vec!["Grep", "Read", "Glob", "Bash"]);
}

#[test]
fn test_tool_catalog_tracks_parallel_metadata() {
    let registry = ToolRegistry::new();
    registry.register(Arc::new(MemorySearchTool));

    assert!(registry.tool_supports_parallel("MemorySearch"));
    assert!(!registry.tool_supports_parallel("exec"));
}

#[test]
fn core_local_tools_declare_explicit_sandbox_operations() {
    let registry = ToolRegistry::new();
    register_core_local_tools(&registry);
    let catalog = registry.tool_catalog();

    for (name, operation) in [
        (
            "Bash",
            talos_tools_sandbox::ToolSandboxOperationKind::SpawnProcess,
        ),
        (
            "Grep",
            talos_tools_sandbox::ToolSandboxOperationKind::SpawnProcess,
        ),
        (
            "Read",
            talos_tools_sandbox::ToolSandboxOperationKind::FileSystem,
        ),
        (
            "Write",
            talos_tools_sandbox::ToolSandboxOperationKind::FileSystem,
        ),
        (
            "Edit",
            talos_tools_sandbox::ToolSandboxOperationKind::FileSystem,
        ),
        (
            "Glob",
            talos_tools_sandbox::ToolSandboxOperationKind::FileSystem,
        ),
    ] {
        assert_eq!(
            catalog
                .find(name)
                .expect("core tool should be registered")
                .sandbox_operation,
            Some(operation),
            "{name} must declare its sandbox operation"
        );
    }
}

#[test]
fn test_request_time_selection_activates_memory_tools_only_for_memory_requests() {
    let registry = ToolRegistry::new();
    registry.register(Arc::new(MemorySearchTool));
    registry.register(Arc::new(MemoryGetTool));

    let default_request = ToolSelectionRequest::default();
    let default_names = registry
        .get_tool_definitions_for_request(&default_request)
        .into_iter()
        .map(|definition| definition.name)
        .collect::<Vec<_>>();
    assert!(!default_names.contains(&"MemorySearch".to_string()));
    assert!(!default_names.contains(&"MemoryGet".to_string()));

    let memory_request = ToolSelectionRequest::default()
        .with_session_id("session-selection")
        .with_user_input("search memory for prior decisions");
    let memory_names = registry
        .get_tool_definitions_for_request(&memory_request)
        .into_iter()
        .map(|definition| definition.name)
        .collect::<Vec<_>>();
    assert!(memory_names.contains(&"MemorySearch".to_string()));
    assert!(memory_names.contains(&"MemoryGet".to_string()));
}

#[tokio::test]
async fn test_external_dynamic_tools_require_tool_search_activation() {
    use crate::spec::{
        default_governance_for_source, RegisteredToolSpec, ToolParallelism, ToolPayloadKind,
        ToolSource,
    };
    use talos_llm_provider::ToolDefinition;

    let registry = ToolRegistry::new();
    registry.register(Arc::new(ToolSearchTool));
    registry.register_external_dynamic_tool(RegisteredToolSpec::new(
        ToolDefinition::new("deferred_greet", "Greet the user", None),
        ToolSource::Dynamic,
        ToolPayloadKind::Function,
        ToolParallelism::ParallelSafe,
        false,
        default_governance_for_source(ToolSource::Dynamic),
    ));

    let session_id = "session-selection";
    let request = ToolSelectionRequest::default().with_session_id(session_id);
    let names = registry
        .get_tool_definitions_for_request(&request)
        .into_iter()
        .map(|definition| definition.name)
        .collect::<Vec<_>>();

    assert!(names.contains(&"ToolSearch".to_string()));
    assert!(!names.contains(&"deferred_greet".to_string()));

    let context = registry.build_context(session_id, "user-1", None);
    let invocation = registry
        .build_function_invocation(
            "call-search",
            "ToolSearch",
            json!({ "query": "deferred_greet" }),
        )
        .expect("tool search invocation should build");
    registry
        .execute_invocation_with_context(invocation, context)
        .await
        .expect("tool search should activate the dynamic tool");

    let activated_names = registry
        .get_tool_definitions_for_request(&request)
        .into_iter()
        .map(|definition| definition.name)
        .collect::<Vec<_>>();
    assert!(activated_names.contains(&"deferred_greet".to_string()));
}

#[tokio::test]
async fn test_mcp_tools_require_tool_search_activation() {
    use crate::spec::{
        default_governance_for_source, RegisteredToolSpec, ToolParallelism, ToolPayloadKind,
        ToolSource,
    };
    use talos_llm_provider::ToolDefinition;

    let registry = ToolRegistry::new();
    registry.register(Arc::new(ToolSearchTool));
    let tool_name = "mcp__playwright__browser_navigate";
    let spec = RegisteredToolSpec::new(
        ToolDefinition::new(
            tool_name,
            "Navigate a browser through Playwright MCP",
            Some(json!({ "type": "object" })),
        ),
        ToolSource::Mcp,
        ToolPayloadKind::Mcp,
        ToolParallelism::ParallelSafe,
        false,
        default_governance_for_source(ToolSource::Mcp),
    );
    registry.register_handler(tool_name, spec, Arc::new(FakeMcpHandler));

    let session_id = "session-mcp-selection";
    let request = ToolSelectionRequest::default().with_session_id(session_id);
    let names = registry
        .get_tool_definitions_for_request(&request)
        .into_iter()
        .map(|definition| definition.name)
        .collect::<Vec<_>>();
    assert!(names.contains(&"ToolSearch".to_string()));
    assert!(!names.contains(&tool_name.to_string()));

    let context = registry.build_context(session_id, "user-1", None);
    let invocation = registry
        .build_function_invocation(
            "call-search",
            "ToolSearch",
            json!({ "query": "playwright" }),
        )
        .expect("tool search invocation should build");
    registry
        .execute_invocation_with_context(invocation, context)
        .await
        .expect("tool search should activate the MCP tool");

    let activated_names = registry
        .get_tool_definitions_for_request(&request)
        .into_iter()
        .map(|definition| definition.name)
        .collect::<Vec<_>>();
    assert!(activated_names.contains(&tool_name.to_string()));
}

#[test]
fn test_register_dynamic_tool_stays_in_catalog_but_not_default_model_context() {
    use crate::handlers::dynamic::DynamicToolFn;
    use crate::output::ToolOutputPayload;
    use crate::spec::{
        default_governance_for_source, RegisteredToolSpec, ToolParallelism, ToolPayloadKind,
        ToolSource,
    };
    use talos_llm_provider::ToolDefinition;

    let registry = ToolRegistry::new();
    let spec = RegisteredToolSpec::new(
        ToolDefinition::new("dynamic_greet", "Greet the user", None),
        ToolSource::Dynamic,
        ToolPayloadKind::Function,
        ToolParallelism::ParallelSafe,
        false,
        default_governance_for_source(ToolSource::Dynamic),
    );
    let func: DynamicToolFn = Arc::new(|_args, _ctx| {
        Box::pin(async move { Ok(ToolOutputPayload::success_text("Hello!")) })
    });
    registry.register_dynamic_tool(spec, func);

    // External tools are searchable catalog entries, but they are not loaded into model context
    // until ToolSearch activates them for a session.
    let defs = registry.get_tool_definitions();
    assert!(!defs.iter().any(|d| d.name == "dynamic_greet"));
    assert!(registry
        .list_tool_names()
        .contains(&"dynamic_greet".to_string()));
}

#[tokio::test]
async fn test_registry_dispatch_function_invocation() {
    let registry = ToolRegistry::new();
    registry.register(Arc::new(EchoTestTool));
    let context = ToolContext::new("session", "user");
    let invocation = registry
        .build_function_invocation("call-1", "echo_test", serde_json::json!({"value": "hello"}))
        .expect("invocation should build");

    let output = registry
        .execute_invocation_with_context(invocation, context)
        .await
        .expect("dispatch should succeed");

    assert!(!output.is_error());
    assert_eq!(output.content(), "{\"value\":\"hello\"}");
}

#[tokio::test]
async fn test_registry_truncate_large_output_with_context_limit() {
    let registry = ToolRegistry::new();
    registry.register(Arc::new(EchoTestTool));

    let context = ToolContext::new_with_kernel_config(
        "session",
        "user",
        Some(test_kernel_config_with_output_limit(64)),
    );

    let large_text = "a".repeat(256);
    let invocation = registry
        .build_function_invocation("call-large", "echo_test", serde_json::json!(large_text))
        .unwrap();
    let result = registry
        .execute_invocation_with_context(invocation, context)
        .await
        .unwrap();

    assert!(!result.is_error());
    assert!(result.content().len() <= 64);
    assert!(result.content().contains("[truncated]"));
}

#[tokio::test]
async fn test_registry_truncate_large_error_with_context_limit() {
    let registry = ToolRegistry::new_with_policy(ToolPolicy::allow_list(vec!["large_error"]));
    registry.register(Arc::new(LargeErrorTool));

    let context = ToolContext::new_with_kernel_config(
        "session",
        "user",
        Some(test_kernel_config_with_output_limit(80)),
    );

    let invocation = registry
        .build_function_invocation("call-error", "large_error", serde_json::json!(null))
        .unwrap();
    let result = registry
        .execute_invocation_with_context(invocation, context)
        .await
        .unwrap();

    assert!(result.is_error());
    let err = result
        .payload()
        .and_then(|payload| payload.error_message())
        .unwrap_or_default();
    assert!(err.len() <= 80);
    assert!(err.contains("[truncated]"));
}
