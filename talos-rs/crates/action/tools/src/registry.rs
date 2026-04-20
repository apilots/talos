use async_trait::async_trait;
use dashmap::DashMap;
use serde_json::Value;
use std::collections::BTreeSet;
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
    default_governance_for_source, RegisteredToolSpec, ToolCatalog, ToolSelection,
    ToolSelectionRequest, ToolSource, ToolVisibility,
};
use crate::state::{get_session_tool_state, PermissionMode};
use crate::ToolContext;

const DEFAULT_TOOL_OUTPUT_MAX_BYTES: usize = 64 * 1024;
const TOOL_OUTPUT_TRUNCATED_SUFFIX: &str = "\n...[truncated]";
const GATE_SOURCE_ALLOW_LIST: &str = "allow_list";
const GATE_SOURCE_TOOL_SEMANTIC: &str = "tool_semantic";
const ERROR_KIND_PERMISSION_REJECTION: &str = "permission_rejection";
const ERROR_KIND_VALIDATION_FAILURE: &str = "validation_failure";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolPromptEntry {
    pub name: String,
    pub description: String,
}

pub trait ToolDefinitionProvider: Send + Sync {
    fn get_tool_definitions(&self) -> Vec<ToolDefinition>;

    fn get_tool_prompt_entries(&self) -> Vec<ToolPromptEntry> {
        self.get_tool_definitions()
            .into_iter()
            .map(|definition| ToolPromptEntry {
                name: definition.name,
                description: definition.description,
            })
            .collect()
    }

    fn get_tool_selection(&self, _request: &ToolSelectionRequest) -> ToolSelection {
        let definitions = self.get_tool_definitions();
        ToolSelection::new(
            definitions
                .into_iter()
                .map(|definition| {
                    RegisteredToolSpec::local_function(
                        definition,
                        crate::spec::ToolParallelism::SerialOnly,
                        false,
                    )
                })
                .collect(),
            Vec::new(),
            Vec::new(),
        )
    }

    fn get_tool_definitions_for_request(
        &self,
        request: &ToolSelectionRequest,
    ) -> Vec<ToolDefinition> {
        self.get_tool_selection(request).model_definitions()
    }

    fn get_tool_prompt_entries_for_request(
        &self,
        request: &ToolSelectionRequest,
    ) -> Vec<ToolPromptEntry> {
        self.get_tool_selection(request)
            .visible_specs()
            .into_iter()
            .map(|spec| ToolPromptEntry {
                name: spec.definition.name.clone(),
                description: spec.definition.description.clone(),
            })
            .collect()
    }
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
    fn tool_priority(name: &str) -> usize {
        match name {
            "Grep" => 0,
            "Read" => 1,
            "Glob" => 2,
            "Bash" => 3,
            "TaskOutput" => 4,
            "TaskStop" => 5,
            _ => 100,
        }
    }

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

    fn tagged_gate_message(error_kind: &str, gate_source: &str, message: &str) -> String {
        format!("[error_kind={error_kind}][gate_source={gate_source}] {message}")
    }

    fn annotate_policy_error(error: TalosError) -> TalosError {
        match error {
            TalosError::AuthorizationDenied => TalosError::ToolError(Self::tagged_gate_message(
                ERROR_KIND_PERMISSION_REJECTION,
                GATE_SOURCE_ALLOW_LIST,
                "authorization denied",
            )),
            other => other,
        }
    }

    fn annotate_pre_execute_error(error: TalosError) -> TalosError {
        match error {
            TalosError::ValidationError(message) => {
                TalosError::ValidationError(Self::tagged_gate_message(
                    ERROR_KIND_VALIDATION_FAILURE,
                    GATE_SOURCE_TOOL_SEMANTIC,
                    message.as_str(),
                ))
            }
            TalosError::AuthorizationDenied => TalosError::ToolError(Self::tagged_gate_message(
                ERROR_KIND_PERMISSION_REJECTION,
                GATE_SOURCE_TOOL_SEMANTIC,
                "authorization denied",
            )),
            other => other,
        }
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
        let tool_name = tool.internal_name().to_string();
        let mutating = tool.mutating();
        let spec = RegisteredToolSpec::new(
            ToolDefinition::new(tool.name(), tool.description(), tool.parameters_schema()),
            tool.source(),
            tool.payload_kind(),
            tool.parallelism(),
            mutating,
            Self::default_governance(tool.internal_name()),
        )
        .with_internal_name(tool.internal_name())
        .with_aliases(tool.aliases().iter().copied())
        .with_visibility(tool.visibility())
        .with_behavior_flags(tool.behavior_flags())
        .with_result_policy(tool.result_policy())
        .with_user_facing_name(tool.user_facing_name());
        self.register_handler(
            tool_name,
            spec,
            Arc::new(LocalToolHandler::new(tool, mutating)),
        );
    }

    fn default_governance(name: &str) -> ToolGovernanceSpec {
        match name {
            "bash" => ToolGovernanceSpec {
                default_approval: ToolApprovalRequirement::Skip,
                preferred_sandbox: ToolSandboxMode::UseDefault,
                argument_override: ToolGovernanceArgumentOverride::BashCommandPermissions,
                retry_without_sandbox: true,
            },
            _ => default_governance_for_source(ToolSource::Local),
        }
    }

    fn activated_deferred_names(request: &ToolSelectionRequest) -> BTreeSet<String> {
        request
            .session_id
            .as_deref()
            .map(get_session_tool_state)
            .map(|state| state.deferred_tools.list().into_iter().collect())
            .unwrap_or_default()
    }

    fn input_contains_any(request: &ToolSelectionRequest, patterns: &[&str]) -> bool {
        let Some(user_input) = request.user_input.as_ref() else {
            return false;
        };
        let normalized = user_input.to_ascii_lowercase();
        patterns.iter().any(|pattern| normalized.contains(pattern))
    }

    fn should_activate_default_visible_tool(
        spec: &RegisteredToolSpec,
        request: &ToolSelectionRequest,
    ) -> bool {
        let name = spec.definition.name.as_str();
        if matches!(spec.visibility, ToolVisibility::AlwaysLoad) {
            return true;
        }
        if Self::activated_deferred_names(request).contains(name) {
            return true;
        }

        match name {
            "Bash" | "Read" | "Grep" | "Glob" | "Edit" | "Write" | "TaskOutput" | "TaskStop" => {
                true
            }
            "ToolSearch" => false,
            "WebFetch" | "WebSearch" => Self::input_contains_any(
                request,
                &[
                    "http", "www.", "url", "website", "web ", "search ", "latest", "news",
                ],
            ),
            "MemorySearch" | "MemoryGet" => Self::input_contains_any(
                request,
                &[
                    "memory",
                    "remember",
                    "recall",
                    "history",
                    "preference",
                    "decision",
                    "todo",
                ],
            ),
            "Agent" => Self::input_contains_any(
                request,
                &[
                    "agent",
                    "subagent",
                    "delegate",
                    "parallel",
                    "background research",
                ],
            ),
            "TodoWrite" => {
                matches!(request.permission_mode, PermissionMode::Plan)
                    || Self::input_contains_any(
                        request,
                        &["todo", "plan", "checklist", "step", "tasks", "task list"],
                    )
            }
            "EnterPlanMode" | "ExitPlanMode" | "AskUserQuestion" => {
                matches!(request.permission_mode, PermissionMode::Plan)
                    || Self::input_contains_any(
                        request,
                        &["plan", "clarify", "question", "ask", "need info"],
                    )
            }
            _ => matches!(spec.visibility, ToolVisibility::DefaultVisible),
        }
    }

    pub fn select_tools(&self, request: &ToolSelectionRequest) -> ToolSelection {
        let specs = self.tool_catalog().specs().to_vec();
        let activated_deferred = Self::activated_deferred_names(request);
        let mut active = Vec::new();
        let mut deferred = Vec::new();
        let mut always_loaded = Vec::new();
        let mut tool_search_spec = None;

        for spec in specs {
            if matches!(spec.visibility, ToolVisibility::Disabled) {
                continue;
            }
            if spec.definition.name == "ToolSearch" {
                tool_search_spec = Some(spec);
                continue;
            }
            match spec.visibility {
                ToolVisibility::AlwaysLoad => always_loaded.push(spec),
                ToolVisibility::Deferred => {
                    if activated_deferred.contains(&spec.definition.name) {
                        active.push(spec);
                    } else {
                        deferred.push(spec);
                    }
                }
                ToolVisibility::DefaultVisible => {
                    if Self::should_activate_default_visible_tool(&spec, request) {
                        active.push(spec);
                    } else {
                        deferred.push(spec);
                    }
                }
                ToolVisibility::Disabled => {}
            }
        }

        if let Some(tool_search) = tool_search_spec {
            if !deferred.is_empty() {
                active.push(tool_search);
            }
        }

        ToolSelection::new(active, deferred, always_loaded)
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
            let tool_name = spec.internal_name.clone();
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
        let tool_name = spec.internal_name.clone();
        let handler = Arc::new(DynamicToolHandler::new(spec.source.clone(), func));
        self.register_handler(tool_name, spec, handler);
    }

    pub fn register_external_dynamic_tool(&self, spec: RegisteredToolSpec) {
        let tool_name = spec.internal_name.clone();
        let handler = Arc::new(DynamicToolHandler::external(spec.source));
        self.register_handler(tool_name, spec, handler);
    }

    pub fn tool_catalog(&self) -> ToolCatalog {
        let mut specs = self
            .specs
            .iter()
            .map(|entry| entry.value().clone())
            .collect::<Vec<_>>();
        specs.sort_by(|left, right| {
            let left_key = (
                Self::tool_priority(left.definition.name.as_str()),
                left.definition.name.as_str(),
            );
            let right_key = (
                Self::tool_priority(right.definition.name.as_str()),
                right.definition.name.as_str(),
            );
            left_key.cmp(&right_key)
        });
        specs.dedup_by(|left, right| left.definition.name == right.definition.name);
        ToolCatalog::new(specs)
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
        if let Some(spec) = self.tool_catalog().find(invocation.tool_name.as_str()) {
            self.policy_checker
                .check_tool_access(spec.definition.name.as_str())
                .map_err(Self::annotate_policy_error)?;
        } else {
            self.policy_checker
                .check_tool_access(invocation.tool_name.as_str())
                .map_err(Self::annotate_policy_error)?;
        }
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
        let Some(handler) = spec.as_ref().and_then(|item| {
            self.handlers
                .get(item.internal_name.as_str())
                .map(|entry| entry.value().clone())
        }) else {
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
        handler
            .pre_execute(&invocation, &context)
            .await
            .map_err(Self::annotate_pre_execute_error)?;
        handler.handle(invocation, context).await
    }

    /// Get a list of available tool names.
    pub fn list_tool_names(&self) -> Vec<String> {
        self.tool_catalog()
            .specs()
            .iter()
            .map(|spec| spec.definition.name.clone())
            .collect()
    }

    /// Get a list of available tool names that are allowed by policy.
    pub fn list_allowed_tool_names(&self) -> Vec<String> {
        self.tool_catalog()
            .specs()
            .iter()
            .map(|spec| spec.definition.name.clone())
            .filter(|name| self.policy_checker.policy.is_allowed(name))
            .collect()
    }

    /// Check if a tool is available and allowed by policy.
    pub fn is_tool_available(&self, name: &str) -> bool {
        self.tool_catalog()
            .find(name)
            .map(|spec| self.policy_checker.policy.is_allowed(&spec.definition.name))
            .unwrap_or(false)
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
            .map(|spec| !spec.behavior_flags.read_only)
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
        let ctx = ctx.with_catalog_snapshot(
            self.tool_catalog()
                .specs()
                .iter()
                .map(|spec| crate::CatalogToolEntry {
                    public_name: spec.definition.name.clone(),
                    description: spec.definition.description.clone(),
                    deferred: matches!(spec.visibility, crate::spec::ToolVisibility::Deferred),
                    available: !matches!(spec.visibility, crate::spec::ToolVisibility::Disabled),
                    unavailable_reason: matches!(
                        spec.visibility,
                        crate::spec::ToolVisibility::Disabled
                    )
                    .then(|| "tool is currently unavailable in this environment".to_string()),
                })
                .collect(),
        );
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

    fn get_tool_prompt_entries(&self) -> Vec<ToolPromptEntry> {
        self.tool_catalog()
            .specs()
            .iter()
            .filter(|spec| spec.is_visible_to_model())
            .map(|spec| {
                let name = if spec.user_facing_name == spec.definition.name {
                    spec.definition.name.clone()
                } else {
                    format!("{} ({})", spec.user_facing_name, spec.definition.name)
                };
                ToolPromptEntry {
                    name,
                    description: spec.definition.description.clone(),
                }
            })
            .collect()
    }

    fn get_tool_selection(&self, request: &ToolSelectionRequest) -> ToolSelection {
        ToolRegistry::select_tools(self, request)
    }

    fn get_tool_definitions_for_request(
        &self,
        request: &ToolSelectionRequest,
    ) -> Vec<ToolDefinition> {
        self.select_tools(request).model_definitions()
    }

    fn get_tool_prompt_entries_for_request(
        &self,
        request: &ToolSelectionRequest,
    ) -> Vec<ToolPromptEntry> {
        self.select_tools(request)
            .visible_specs()
            .into_iter()
            .map(|spec| ToolPromptEntry {
                name: spec.definition.name.clone(),
                description: spec.definition.description.clone(),
            })
            .collect()
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
        let allow_list_registry =
            ToolRegistry::new_with_policy(ToolPolicy::allow_list(vec!["Read"]));
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
}
