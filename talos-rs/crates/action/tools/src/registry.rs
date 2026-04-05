use async_trait::async_trait;
use dashmap::DashMap;
use serde_json::Value;
use std::sync::Arc;
use talos_config::AgentKernelConfig;
use talos_error::{TalosError, TalosResult};
use talos_llm_provider::ToolDefinition;
use talos_tools_policy::{ToolPolicy, ToolPolicyChecker};
use talos_tools_sandbox::{
    ToolApprovalRequirement, ToolGovernanceArgumentOverride, ToolGovernanceSpec, ToolSandboxMode,
};
use tracing::warn;

use crate::collaboration::CollaborationRuntime;
use crate::handlers::{DynamicToolHandler, LocalToolHandler, McpToolHandler, ToolHandler};
use crate::invocation::{ToolDispatchContext, ToolInvocation};
use crate::mcp::McpConnectionManager;
use crate::orchestrator::RegistryOrchestrator;
use crate::output::{ToolDispatchOutcome, ToolOutputPayload};
use crate::r#trait::Tool;
use crate::router::ToolRouter;
use crate::spec::{
    default_governance_for_source, RegisteredToolSpec, ToolCatalog, ToolParallelism, ToolSource,
};
use crate::ToolContext;

const DEFAULT_TOOL_OUTPUT_MAX_BYTES: usize = 64 * 1024;
const TOOL_OUTPUT_TRUNCATED_SUFFIX: &str = "\n...[truncated]";

pub trait ToolDefinitionProvider: Send + Sync {
    fn get_tool_definitions(&self) -> Vec<ToolDefinition>;
}

#[async_trait]
pub trait ToolExecutorPort: Send + Sync {
    fn build_function_invocation(
        &self,
        call_id: String,
        tool_name: String,
        arguments: Value,
    ) -> TalosResult<ToolInvocation>;

    async fn execute_invocation_with_context(
        &self,
        invocation: ToolInvocation,
        context: ToolContext,
    ) -> TalosResult<ToolDispatchOutcome>;

    fn tool_supports_parallel(&self, name: &str) -> bool;

    fn tool_is_mutating(&self, name: &str) -> bool;

    fn build_context(
        &self,
        session_id: String,
        user_id: String,
        kernel_config: Option<Arc<AgentKernelConfig>>,
    ) -> ToolContext;
}

pub trait ToolRuntime: ToolDefinitionProvider + ToolExecutorPort + Send + Sync {}

/// Registry for managing tools with security policies.
pub struct ToolRegistry {
    specs: DashMap<String, RegisteredToolSpec>,
    handlers: DashMap<String, Arc<dyn ToolHandler>>,
    router: ToolRouter,
    policy_checker: ToolPolicyChecker,
    collaboration_runtime: DashMap<&'static str, Arc<CollaborationRuntime>>,
}

impl ToolRegistry {
    fn resolve_max_output_bytes(context: &ToolContext) -> usize {
        context
            .kernel_config()
            .map(|config| config.tool_result_max_bytes)
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_TOOL_OUTPUT_MAX_BYTES)
    }

    fn truncate_text(input: &str, max_bytes: usize) -> (String, bool, usize, usize) {
        let raw_size = input.len();
        if raw_size <= max_bytes {
            return (input.to_string(), false, raw_size, raw_size);
        }

        if max_bytes == 0 {
            return (String::new(), true, raw_size, 0);
        }

        if max_bytes <= TOOL_OUTPUT_TRUNCATED_SUFFIX.len() {
            let suffix = &TOOL_OUTPUT_TRUNCATED_SUFFIX[..max_bytes];
            return (suffix.to_string(), true, raw_size, suffix.len());
        }

        let mut cutoff = max_bytes - TOOL_OUTPUT_TRUNCATED_SUFFIX.len();
        while cutoff > 0 && !input.is_char_boundary(cutoff) {
            cutoff -= 1;
        }

        let mut output = String::with_capacity(max_bytes);
        output.push_str(&input[..cutoff]);
        output.push_str(TOOL_OUTPUT_TRUNCATED_SUFFIX);
        let returned_size = output.len();

        (output, true, raw_size, returned_size)
    }

    fn truncate_structured_error(
        value: &mut Option<Value>,
        max_output_bytes: usize,
    ) -> Option<(usize, usize)> {
        let Some(payload) = value.as_mut() else {
            return None;
        };
        let Some(object) = payload.as_object_mut() else {
            return None;
        };
        let Some(error_value) = object.get_mut("error") else {
            return None;
        };
        let Some(error_text) = error_value.as_str() else {
            return None;
        };

        let (limited, truncated, raw_size, returned_size) =
            Self::truncate_text(error_text, max_output_bytes);
        if truncated {
            *error_value = Value::String(limited);
            Some((raw_size, returned_size))
        } else {
            None
        }
    }

    fn enforce_output_limits(
        tool_name: &str,
        mut result: ToolDispatchOutcome,
        max_output_bytes: usize,
    ) -> ToolDispatchOutcome {
        let Some(payload) = (match &mut result {
            ToolDispatchOutcome::Executed { payload } => Some(payload),
            ToolDispatchOutcome::Pending { .. } => None,
        }) else {
            return result;
        };
        let (content_truncated, content_raw_size, content_returned_size) = match payload {
            ToolOutputPayload::Function { content, .. }
            | ToolOutputPayload::Resource { content, .. }
            | ToolOutputPayload::Mcp { content, .. } => {
                let (limited, truncated, raw_size, returned_size) =
                    Self::truncate_text(content, max_output_bytes);
                *content = limited;
                (truncated, raw_size, returned_size)
            }
        };
        let mut error_raw_size = 0;
        let mut error_returned_size = 0;
        let mut error_truncated = false;

        if let ToolOutputPayload::Function {
            structured_content, ..
        } = payload
        {
            if let Some((raw_size, returned_size)) =
                Self::truncate_structured_error(structured_content, max_output_bytes)
            {
                error_truncated = true;
                error_raw_size = raw_size;
                error_returned_size = returned_size;
            }
        }

        if content_truncated || error_truncated {
            warn!(
                tool_name = tool_name,
                max_output_bytes = max_output_bytes,
                output_raw_size = content_raw_size,
                output_returned_size = content_returned_size,
                error_raw_size = error_raw_size,
                error_returned_size = error_returned_size,
                "tool result exceeded size limit and was truncated"
            );
        }

        result
    }

    fn register_local_tool(&self, tool: Arc<dyn Tool>) {
        let tool_name = tool.name().to_string();
        let mutating = Self::default_mutation(tool_name.as_str());
        let spec = RegisteredToolSpec::local_function(
            ToolDefinition::new(tool.name(), tool.description(), tool.parameters_schema()),
            Self::default_parallelism(tool.name()),
            mutating,
        )
        .with_governance(Self::default_governance(tool.name()));
        self.register_handler(
            tool_name,
            spec,
            Arc::new(LocalToolHandler::new(tool, mutating)),
        );
    }

    fn default_parallelism(name: &str) -> ToolParallelism {
        match name {
            "read" | "read_file" | "list_dir" | "grep_files" | "memory_search" | "memory_get"
            | "web_search" | "web_fetch" | "http_request" => ToolParallelism::ParallelSafe,
            _ => ToolParallelism::SerialOnly,
        }
    }

    fn default_mutation(name: &str) -> bool {
        matches!(
            name,
            "write" | "edit" | "exec" | "exec_command" | "write_stdin" | "apply_patch"
        )
    }

    fn default_governance(name: &str) -> ToolGovernanceSpec {
        match name {
            "exec_command" => ToolGovernanceSpec {
                default_approval: ToolApprovalRequirement::Skip,
                preferred_sandbox: ToolSandboxMode::UseDefault,
                argument_override: ToolGovernanceArgumentOverride::ExecCommandPermissions,
                retry_without_sandbox: true,
            },
            "apply_patch" => ToolGovernanceSpec {
                default_approval: ToolApprovalRequirement::Skip,
                preferred_sandbox: ToolSandboxMode::WorkspaceWrite,
                argument_override: ToolGovernanceArgumentOverride::None,
                retry_without_sandbox: false,
            },
            _ => default_governance_for_source(ToolSource::Local),
        }
    }

    /// Create a new registry with default policy.
    pub fn new() -> Self {
        Self::new_with_policy(ToolPolicy::default())
    }

    /// Create a new registry with a custom policy.
    pub fn new_with_policy(policy: ToolPolicy) -> Self {
        Self {
            specs: DashMap::new(),
            handlers: DashMap::new(),
            router: ToolRouter,
            policy_checker: ToolPolicyChecker::new(policy),
            collaboration_runtime: DashMap::new(),
        }
    }

    /// Register a legacy local tool by wrapping it into the new handler path.
    pub fn register(&self, tool: Arc<dyn Tool>) {
        self.register_local_tool(tool);
    }

    pub fn register_handler(
        &self,
        tool_name: impl Into<String>,
        spec: RegisteredToolSpec,
        handler: Arc<dyn ToolHandler>,
    ) {
        let tool_name = tool_name.into();
        if self.specs.insert(tool_name.clone(), spec).is_some() {
            warn!(tool_name = %tool_name, "overwriting tool spec");
        }
        if self.handlers.insert(tool_name.clone(), handler).is_some() {
            warn!(tool_name = %tool_name, "overwriting tool handler");
        }
    }

    /// Register all tools from a connected [`McpConnectionManager`].
    ///
    /// Each tool from the manager is registered with a [`McpToolHandler`] that
    /// delegates execution back through the manager.
    pub fn register_mcp_server(&self, manager: Arc<McpConnectionManager>) {
        let handler = Arc::new(McpToolHandler::new(Arc::clone(&manager)));
        for spec in manager.all_tool_specs() {
            let tool_name = spec.definition.name.clone();
            self.register_handler(
                tool_name,
                spec,
                Arc::clone(&handler) as Arc<dyn ToolHandler>,
            );
        }
    }

    /// Register a dynamic tool backed by an async callback.
    pub fn register_dynamic_tool(
        &self,
        spec: RegisteredToolSpec,
        func: crate::handlers::dynamic::DynamicToolFn,
    ) {
        let tool_name = spec.definition.name.clone();
        let handler = Arc::new(DynamicToolHandler::new(spec.source.clone(), func));
        self.register_handler(tool_name, spec, handler);
    }

    pub fn register_external_dynamic_tool(&self, spec: RegisteredToolSpec) {
        let tool_name = spec.definition.name.clone();
        let handler = Arc::new(DynamicToolHandler::external(spec.source));
        self.register_handler(tool_name, spec, handler);
    }

    pub fn tool_catalog(&self) -> ToolCatalog {
        ToolCatalog::new(
            self.specs
                .iter()
                .map(|entry| entry.value().clone())
                .collect(),
        )
    }

    pub fn build_function_invocation(
        &self,
        call_id: impl Into<String>,
        tool_name: impl Into<String>,
        arguments: Value,
    ) -> TalosResult<ToolInvocation> {
        self.router
            .build_function_invocation(call_id, tool_name, arguments, &self.tool_catalog())
    }

    pub async fn execute_invocation_with_context(
        &self,
        invocation: ToolInvocation,
        context: ToolContext,
    ) -> TalosResult<ToolDispatchOutcome> {
        self.policy_checker
            .check_tool_access(invocation.tool_name.as_str())?;
        self.execute_invocation_unchecked_with_context(invocation, context)
            .await
    }

    pub async fn execute_invocation_unchecked_with_context(
        &self,
        invocation: ToolInvocation,
        context: ToolContext,
    ) -> TalosResult<ToolDispatchOutcome> {
        let max_output_bytes = Self::resolve_max_output_bytes(&context);
        let tool_name = invocation.tool_name.clone();
        let result = self
            .dispatch(invocation, ToolDispatchContext::new(context))
            .await?;
        Ok(Self::enforce_output_limits(
            tool_name.as_str(),
            result,
            max_output_bytes,
        ))
    }

    pub async fn dispatch(
        &self,
        invocation: ToolInvocation,
        context: ToolDispatchContext,
    ) -> TalosResult<ToolDispatchOutcome> {
        let catalog = self.tool_catalog();
        let invocation = self.router.normalize(invocation, &catalog)?;
        let spec = catalog.find(invocation.tool_name.as_str()).cloned();
        let orchestrator = RegistryOrchestrator::from_context(context.tool_context());
        if let Some(pending) = orchestrator.resolve_pending(&invocation, &context, spec.as_ref())? {
            return Ok(pending);
        }
        let Some(handler) = self.handlers.get(invocation.tool_name.as_str()) else {
            return Err(TalosError::ToolError(format!(
                "Tool '{}' not found",
                invocation.tool_name
            )));
        };
        if !handler.matches(&invocation) {
            return Err(TalosError::ToolError(format!(
                "Tool '{}' invoked with incompatible payload",
                invocation.tool_name
            )));
        }
        handler.handle(invocation, context).await
    }

    /// Get a list of available tool names.
    pub fn list_tool_names(&self) -> Vec<String> {
        self.specs.iter().map(|entry| entry.key().clone()).collect()
    }

    /// Get a list of available tool names that are allowed by policy.
    pub fn list_allowed_tool_names(&self) -> Vec<String> {
        self.specs
            .iter()
            .map(|entry| entry.key().clone())
            .filter(|name| self.policy_checker.policy.is_allowed(name))
            .collect()
    }

    /// Check if a tool is available and allowed by policy.
    pub fn is_tool_available(&self, name: &str) -> bool {
        self.specs.contains_key(name) && self.policy_checker.policy.is_allowed(name)
    }

    /// Return all registered tools as model-friendly definitions.
    pub fn get_tool_definitions(&self) -> Vec<ToolDefinition> {
        self.tool_catalog().model_definitions()
    }

    pub fn tool_supports_parallel(&self, name: &str) -> bool {
        self.tool_catalog().supports_parallel(name)
    }

    pub fn tool_is_mutating(&self, name: &str) -> bool {
        self.tool_catalog()
            .find(name)
            .map(|spec| spec.mutating)
            .unwrap_or(true)
    }

    /// Build a context from execution state.
    pub fn build_context(
        &self,
        session_id: impl Into<String>,
        user_id: impl Into<String>,
        kernel_config: Option<Arc<AgentKernelConfig>>,
    ) -> ToolContext {
        let ctx = ToolContext::new_with_kernel_config(session_id, user_id, kernel_config);
        if let Some(runtime) = self
            .collaboration_runtime
            .get("default")
            .map(|entry| entry.value().clone())
        {
            ctx.with_collaboration_runtime(runtime)
        } else {
            ctx
        }
    }

    pub fn set_collaboration_runtime(&self, runtime: Arc<CollaborationRuntime>) {
        self.collaboration_runtime.insert("default", runtime);
    }

    /// Update the policy for the registry.
    pub fn update_policy(&mut self, policy: ToolPolicy) {
        self.policy_checker = ToolPolicyChecker::new(policy);
    }

    /// Get current policy.
    pub fn get_policy(&self) -> &ToolPolicy {
        &self.policy_checker.policy
    }
}

impl ToolDefinitionProvider for ToolRegistry {
    fn get_tool_definitions(&self) -> Vec<ToolDefinition> {
        ToolRegistry::get_tool_definitions(self)
    }
}

#[async_trait]
impl ToolExecutorPort for ToolRegistry {
    fn build_function_invocation(
        &self,
        call_id: String,
        tool_name: String,
        arguments: Value,
    ) -> TalosResult<ToolInvocation> {
        ToolRegistry::build_function_invocation(self, call_id, tool_name, arguments)
    }

    async fn execute_invocation_with_context(
        &self,
        invocation: ToolInvocation,
        context: ToolContext,
    ) -> TalosResult<ToolDispatchOutcome> {
        ToolRegistry::execute_invocation_with_context(self, invocation, context).await
    }

    fn tool_supports_parallel(&self, name: &str) -> bool {
        ToolRegistry::tool_supports_parallel(self, name)
    }

    fn tool_is_mutating(&self, name: &str) -> bool {
        ToolRegistry::tool_is_mutating(self, name)
    }

    fn build_context(
        &self,
        session_id: String,
        user_id: String,
        kernel_config: Option<Arc<AgentKernelConfig>>,
    ) -> ToolContext {
        ToolRegistry::build_context(self, session_id, user_id, kernel_config)
    }
}

impl ToolRuntime for ToolRegistry {}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trait_impl::EchoTestTool;
    use crate::MemorySearchTool;
    use crate::ToolOutput;
    use async_trait::async_trait;
    use talos_config::AgentKernelConfig;
    use talos_tools_policy::ToolPolicy;

    struct LargeErrorTool;

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

        async fn execute(&self, _input: Value, _context: ToolContext) -> TalosResult<ToolOutput> {
            Ok(ToolOutput {
                success: false,
                output: String::new(),
                error: Some("e".repeat(256)),
            })
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

        assert!(result.is_err());
        match result.err().unwrap() {
            talos_error::TalosError::AuthorizationDenied => {}
            _ => panic!("Expected AuthorizationDenied error"),
        }
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
                "memory_search",
                serde_json::json!({ "query": "target" }),
            )
            .unwrap();

        let result = registry
            .execute_invocation_with_context(invocation, context)
            .await
            .expect("default policy should allow memory_search");

        assert!(!result.is_error());
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
    fn test_tool_catalog_tracks_parallel_metadata() {
        let registry = ToolRegistry::new();
        registry.register(Arc::new(MemorySearchTool));

        assert!(registry.tool_supports_parallel("memory_search"));
        assert!(!registry.tool_supports_parallel("exec"));
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
}
