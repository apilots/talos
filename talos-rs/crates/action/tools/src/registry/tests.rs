use super::*;
use crate::register_core_local_tools;
use crate::trait_impl::EchoTestTool;
use crate::{MemoryGetTool, MemorySearchTool, SkillTool, ToolSearchTool};
use async_trait::async_trait;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use talos_config::AgentKernelConfig;
use talos_error::TalosError;
use talos_tools_policy::ToolPolicy;

struct LargeErrorTool;
struct ValidationGateTool {
    executes: Arc<AtomicUsize>,
}
struct PermissionGateTool {
    executes: Arc<AtomicUsize>,
}

fn assert_tagged_validation_error(
    error: TalosError,
    expected_gate_source: &str,
    expected_fragment: &str,
) {
    match error {
        TalosError::ValidationError(message) => {
            assert!(
                message.contains("[error_kind=validation_failure]"),
                "missing validation error_kind tag: {message}"
            );
            assert!(
                message.contains(&format!("[gate_source={expected_gate_source}]")),
                "missing gate_source tag: {message}"
            );
            assert!(
                message.contains(expected_fragment),
                "missing expected fragment '{expected_fragment}': {message}"
            );
        }
        other => panic!("expected ValidationError, got {other:?}"),
    }
}

fn assert_tagged_permission_error(error: TalosError, expected_gate_source: &str) {
    match error {
        TalosError::ToolError(message) => {
            assert!(
                message.contains("[error_kind=permission_rejection]"),
                "missing permission error_kind tag: {message}"
            );
            assert!(
                message.contains(&format!("[gate_source={expected_gate_source}]")),
                "missing gate_source tag: {message}"
            );
            assert!(
                message.contains("authorization denied"),
                "missing authorization fragment: {message}"
            );
        }
        other => panic!("expected ToolError, got {other:?}"),
    }
}

fn test_kernel_config_with_output_limit(limit: usize) -> Arc<AgentKernelConfig> {
    let mut config = AgentKernelConfig::default();
    config.tool_result_max_bytes = limit;
    Arc::new(config)
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
        Ok(ToolOutputPayload::Function {
            content: String::new(),
            success: false,
            structured_content: Some(json!({ "error": "e".repeat(256) })),
        })
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

    assert_tagged_permission_error(result.unwrap_err(), GATE_SOURCE_ALLOW_LIST);
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

    assert_tagged_validation_error(
        error,
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

    assert_tagged_permission_error(error, GATE_SOURCE_TOOL_SEMANTIC);
    assert_eq!(executes.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn test_read_tool_validation_returns_tagged_metadata() {
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

    assert_tagged_validation_error(error, GATE_SOURCE_TOOL_SEMANTIC, "path is required");
}

#[tokio::test]
async fn test_edit_tool_validation_returns_tagged_metadata() {
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

    assert_tagged_validation_error(
        error,
        GATE_SOURCE_TOOL_SEMANTIC,
        "find/old_string is required",
    );
}

#[tokio::test]
async fn test_bash_tool_semantic_permission_returns_tagged_metadata() {
    let policy = ToolPolicy::allow_list(vec!["Bash"]);
    let registry = ToolRegistry::new_with_policy(policy);
    register_core_local_tools(&registry);

    let invocation = registry
        .build_function_invocation(
            "call-bash-semantic-permission",
            "Bash",
            serde_json::json!({
                "command": "echo hi",
                "dangerously_disable_sandbox": true
            }),
        )
        .unwrap();

    let error = registry
        .execute_invocation_with_context(invocation, ToolContext::default())
        .await
        .unwrap_err();

    assert_tagged_permission_error(error, GATE_SOURCE_TOOL_SEMANTIC);
}

#[tokio::test]
async fn test_outer_allow_list_and_semantic_gate_sources_are_distinguishable() {
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
    assert_tagged_permission_error(allow_list_error, GATE_SOURCE_ALLOW_LIST);

    let semantic_registry = ToolRegistry::new_with_policy(ToolPolicy::allow_list(vec!["Bash"]));
    register_core_local_tools(&semantic_registry);
    let semantic_invocation = semantic_registry
        .build_function_invocation(
            "call-bash-semantic",
            "Bash",
            serde_json::json!({
                "command": "echo hi",
                "dangerously_disable_sandbox": true
            }),
        )
        .unwrap();

    let semantic_error = semantic_registry
        .execute_invocation_with_context(semantic_invocation, ToolContext::default())
        .await
        .unwrap_err();
    assert_tagged_permission_error(semantic_error, GATE_SOURCE_TOOL_SEMANTIC);
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

    let mut config = AgentKernelConfig::default();
    config.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();
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

#[test]
fn test_request_time_selection_includes_tool_search_for_deferred_tools() {
    let registry = ToolRegistry::new();
    registry.register(Arc::new(ToolSearchTool));
    registry.register(Arc::new(SkillTool));

    let request = ToolSelectionRequest::default().with_session_id("session-selection");
    let names = registry
        .get_tool_definitions_for_request(&request)
        .into_iter()
        .map(|definition| definition.name)
        .collect::<Vec<_>>();

    assert!(names.contains(&"ToolSearch".to_string()));
    assert!(!names.contains(&"Skill".to_string()));
}

#[test]
fn test_register_dynamic_tool_appears_in_catalog() {
    use crate::handlers::dynamic::DynamicToolFn;
    use crate::output::ToolOutputPayload;
    use crate::spec::{
        default_governance_for_source, RegisteredToolSpec, ToolParallelism, ToolPayloadKind,
        ToolSource,
    };
    use talos_llm_provider::ToolDefinition;

    let registry = ToolRegistry::new();
    let spec = RegisteredToolSpec::new(
        ToolDefinition::new("skill_greet", "Greet the user", None),
        ToolSource::Dynamic,
        ToolPayloadKind::Function,
        ToolParallelism::ParallelSafe,
        false,
        default_governance_for_source(ToolSource::Dynamic),
    );
    let func: DynamicToolFn = Arc::new(|_args, _ctx| {
        Box::pin(async move {
            Ok(ToolOutputPayload::Function {
                content: "Hello!".to_string(),
                success: true,
                structured_content: None,
            })
        })
    });
    registry.register_dynamic_tool(spec, func);

    // The tool is registered in the catalog even if the default policy doesn't list it.
    let defs = registry.get_tool_definitions();
    assert!(defs.iter().any(|d| d.name == "skill_greet"));
    assert!(registry
        .list_tool_names()
        .contains(&"skill_greet".to_string()));
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
