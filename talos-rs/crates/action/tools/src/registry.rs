use dashmap::DashMap;
use serde_json::Value;
use std::sync::Arc;
use talos_config::AgentKernelConfig;
use talos_error::TalosResult;
use talos_llm_provider::ToolDefinition;
use tracing::warn;

use crate::ToolOutput;
use crate::{Tool, ToolContext};
use talos_tools_policy::{ToolPolicy, ToolPolicyChecker};

const DEFAULT_TOOL_OUTPUT_MAX_BYTES: usize = 64 * 1024;
const TOOL_OUTPUT_TRUNCATED_SUFFIX: &str = "\n...[truncated]";

/// Registry for managing tools with security policies
pub struct ToolRegistry {
    tools: DashMap<String, Arc<dyn Tool>>,
    policy_checker: ToolPolicyChecker,
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

    fn enforce_output_limits(
        tool_name: &str,
        mut result: ToolOutput,
        max_output_bytes: usize,
    ) -> ToolOutput {
        let (limited_output, output_truncated, output_raw_size, output_returned_size) =
            Self::truncate_text(&result.output, max_output_bytes);
        result.output = limited_output;

        let (limited_error, error_truncated, error_raw_size, error_returned_size) =
            match result.error.take() {
                Some(error_text) => {
                    let (text, truncated, raw_size, returned_size) =
                        Self::truncate_text(&error_text, max_output_bytes);
                    (Some(text), truncated, raw_size, returned_size)
                }
                None => (None, false, 0, 0),
            };
        result.error = limited_error;

        if output_truncated || error_truncated {
            warn!(
                tool_name = tool_name,
                max_output_bytes = max_output_bytes,
                output_raw_size = output_raw_size,
                output_returned_size = output_returned_size,
                error_raw_size = error_raw_size,
                error_returned_size = error_returned_size,
                "tool result exceeded size limit and was truncated"
            );
        }

        result
    }

    /// Create a new registry with default policy
    pub fn new() -> Self {
        let default_policy = ToolPolicy::default();
        let policy_checker = ToolPolicyChecker::new(default_policy);

        Self {
            tools: DashMap::new(),
            policy_checker,
        }
    }

    /// Create a new registry with a custom policy
    pub fn new_with_policy(policy: ToolPolicy) -> Self {
        let policy_checker = ToolPolicyChecker::new(policy);

        Self {
            tools: DashMap::new(),
            policy_checker,
        }
    }

    /// Register a tool
    pub fn register(&self, tool: Arc<dyn Tool>) {
        let name = tool.name().to_string();
        self.tools.insert(name, tool);
    }

    /// Execute a tool by name, checking security policy first
    pub async fn execute(&self, name: &str, input: Value) -> TalosResult<ToolOutput> {
        self.execute_with_context(name, input, ToolContext::default())
            .await
    }

    /// Execute a tool by name with custom context.
    pub async fn execute_with_context(
        &self,
        name: &str,
        input: Value,
        context: ToolContext,
    ) -> TalosResult<ToolOutput> {
        // Check if the tool is allowed by policy
        self.policy_checker.check_tool_access(name)?;
        let max_output_bytes = Self::resolve_max_output_bytes(&context);

        if let Some(tool) = self.tools.get(name) {
            let result = tool.execute(input, context).await?;
            Ok(Self::enforce_output_limits(name, result, max_output_bytes))
        } else {
            Err(talos_error::TalosError::ToolError(format!(
                "Tool '{}' not found",
                name
            )))
        }
    }

    /// Execute a tool without policy check (internal use only)
    pub async fn execute_unchecked(&self, name: &str, input: Value) -> TalosResult<ToolOutput> {
        self.execute_unchecked_with_context(name, input, ToolContext::default())
            .await
    }

    /// Execute a tool without policy check with custom context.
    pub async fn execute_unchecked_with_context(
        &self,
        name: &str,
        input: Value,
        context: ToolContext,
    ) -> TalosResult<ToolOutput> {
        let max_output_bytes = Self::resolve_max_output_bytes(&context);
        if let Some(tool) = self.tools.get(name) {
            let result = tool.execute(input, context).await?;
            Ok(Self::enforce_output_limits(name, result, max_output_bytes))
        } else {
            Err(talos_error::TalosError::ToolError(format!(
                "Tool '{}' not found",
                name
            )))
        }
    }

    /// Get a list of available tool names
    pub fn list_tool_names(&self) -> Vec<String> {
        self.tools.iter().map(|entry| entry.key().clone()).collect()
    }

    /// Get a list of available tool names that are allowed by policy
    pub fn list_allowed_tool_names(&self) -> Vec<String> {
        self.tools
            .iter()
            .map(|entry| entry.key().clone())
            .filter(|name| self.policy_checker.policy.is_allowed(name))
            .collect()
    }

    /// Check if a tool is available and allowed by policy
    pub fn is_tool_available(&self, name: &str) -> bool {
        self.tools.contains_key(name) && self.policy_checker.policy.is_allowed(name)
    }

    /// Return all registered tools as model-friendly definitions.
    pub fn get_tool_definitions(&self) -> Vec<ToolDefinition> {
        self.tools
            .iter()
            .map(|entry| {
                let tool = entry.value();
                ToolDefinition::new(tool.name(), tool.description(), tool.parameters_schema())
            })
            .collect()
    }

    /// Build a context from execution state.
    pub fn build_context(
        &self,
        session_id: impl Into<String>,
        user_id: impl Into<String>,
        kernel_config: Option<Arc<AgentKernelConfig>>,
    ) -> ToolContext {
        ToolContext::new_with_kernel_config(session_id, user_id, kernel_config)
    }

    /// Update the policy for the registry
    pub fn update_policy(&mut self, policy: ToolPolicy) {
        self.policy_checker = ToolPolicyChecker::new(policy);
    }

    /// Get current policy
    pub fn get_policy(&self) -> &ToolPolicy {
        &self.policy_checker.policy
    }
}

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
        let result = registry.execute("echo_test", input).await.unwrap();

        assert!(result.success);
        assert_eq!(result.output, "hello");
    }

    #[tokio::test]
    async fn test_registry_tool_not_found() {
        let registry = ToolRegistry::new();
        let input = serde_json::json!("test");
        let result = registry.execute("nonexistent_tool", input).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_registry_policy_enforcement() {
        // Create a restrictive policy that only allows "read" tool
        let policy = ToolPolicy::allow_list(vec!["read"]);
        let registry = ToolRegistry::new_with_policy(policy);

        let echo_tool = Arc::new(EchoTestTool);
        registry.register(echo_tool);

        // Attempt to execute "echo_test" tool, which should be denied
        let input = serde_json::json!("hello");
        let result = registry.execute("echo_test", input).await;

        // This should fail because "echo_test" is not in the allow list
        assert!(result.is_err());
        match result.err().unwrap() {
            talos_error::TalosError::AuthorizationDenied => {} // Expected error
            _ => panic!("Expected AuthorizationDenied error"),
        }
    }

    #[tokio::test]
    async fn test_registry_policy_allowed_tool() {
        // Create a policy that allows "echo_test" tool
        let policy = ToolPolicy::allow_list(vec!["echo_test"]);
        let registry = ToolRegistry::new_with_policy(policy);

        let echo_tool = Arc::new(EchoTestTool);
        registry.register(echo_tool);

        let input = serde_json::json!("hello");
        let result = registry.execute("echo_test", input).await.unwrap();

        // This should succeed because "echo_test" is in the allow list
        assert!(result.success);
        assert_eq!(result.output, "hello");
    }

    #[tokio::test]
    async fn test_list_allowed_tools() {
        // Create a policy that only allows "read" and "write" tools
        let policy = ToolPolicy::allow_list(vec!["read", "write"]);
        let registry = ToolRegistry::new_with_policy(policy);

        // Register multiple tools
        registry.register(Arc::new(EchoTestTool));

        let allowed_tools = registry.list_allowed_tool_names();
        // The "echo_test" tool is not in the allow list, so it shouldn't appear
        assert!(!allowed_tools.contains(&"echo_test".to_string()));
    }

    #[tokio::test]
    async fn test_is_tool_available() {
        let policy = ToolPolicy::allow_list(vec!["read"]);
        let registry = ToolRegistry::new_with_policy(policy);

        registry.register(Arc::new(EchoTestTool));

        // The tool is registered but not allowed by policy
        assert!(!registry.is_tool_available("echo_test"));

        // Register a tool that is allowed by policy
        let policy_allow_echo = ToolPolicy::allow_list(vec!["echo_test"]);
        let registry2 = ToolRegistry::new_with_policy(policy_allow_echo);
        registry2.register(Arc::new(EchoTestTool));

        // Now the tool should be available
        assert!(registry2.is_tool_available("echo_test"));
    }

    #[tokio::test]
    async fn test_registry_execute_with_context() {
        let policy = ToolPolicy::allow_list(vec!["echo_test"]);
        let registry = ToolRegistry::new_with_policy(policy);
        registry.register(Arc::new(EchoTestTool));

        let context = ToolContext::new("session-ctx", "user-ctx");

        let result = registry
            .execute_with_context("echo_test", serde_json::json!("hello"), context)
            .await
            .unwrap();
        assert!(result.success);
        assert_eq!(result.output, "hello");
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
        config.memory.search_min_score = 0.0;
        let context =
            ToolContext::new_with_kernel_config("session-ctx", "user-ctx", Some(Arc::new(config)));

        let result = registry
            .execute_with_context(
                "memory_search",
                serde_json::json!({ "query": "target" }),
                context,
            )
            .await
            .expect("default policy should allow memory_search");

        assert!(result.success);
    }

    #[test]
    fn test_get_tool_definitions() {
        let registry = ToolRegistry::new();
        registry.register(Arc::new(EchoTestTool));
        let defs = registry.get_tool_definitions();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].name, "echo_test");
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
        let result = registry
            .execute_with_context("echo_test", serde_json::json!(large_text), context)
            .await
            .unwrap();

        assert!(result.success);
        assert!(result.output.len() <= 64);
        assert!(result.output.contains("[truncated]"));
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

        let result = registry
            .execute_with_context("large_error", serde_json::json!(null), context)
            .await
            .unwrap();

        assert!(!result.success);
        let err = result.error.unwrap_or_default();
        assert!(err.len() <= 80);
        assert!(err.contains("[truncated]"));
    }
}
