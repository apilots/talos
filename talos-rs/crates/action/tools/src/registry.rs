use async_trait::async_trait;
use dashmap::DashMap;
use serde_json::Value;
use std::collections::BTreeSet;
use std::sync::Arc;
use talos_config::AgentKernelConfig;
use talos_error::{TalosError, TalosResult, TalosToolErrorKind};
use talos_llm_provider::ToolDefinition;
use talos_memory::MemoryReader;
use talos_tools_policy::{ToolPolicy, ToolPolicyChecker};
use talos_tools_sandbox::{ToolGovernanceArgumentOverride, ToolGovernanceSpec, ToolSandboxMode};
use tracing::warn;

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
use crate::state::get_session_tool_state;
use crate::AgentControl;
use crate::ToolContext;

const DEFAULT_TOOL_OUTPUT_MAX_BYTES: usize = 64 * 1024;
const TOOL_OUTPUT_TRUNCATED_SUFFIX: &str = "\n...[truncated]";
const GATE_SOURCE_ALLOW_LIST: &str = "allow_list";
const GATE_SOURCE_TOOL_SEMANTIC: &str = "tool_semantic";

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
pub trait ToolExecutorPort: ToolDefinitionProvider + Send + Sync {
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
    agent_control: DashMap<&'static str, Arc<dyn AgentControl>>,
    memory_reader: DashMap<&'static str, Arc<dyn MemoryReader>>,
}

impl ToolRegistry {
    fn tool_priority(name: &str) -> usize {
        match name {
            "Grep" => 0,
            "Read" => 1,
            "Glob" => 2,
            "Bash" => 3,
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

    fn annotate_policy_error(error: TalosError) -> TalosError {
        match error {
            TalosError::AuthorizationDenied => TalosError::ToolFailure {
                kind: TalosToolErrorKind::PermissionRejection,
                message: "authorization denied".to_string(),
                gate_source: Some(GATE_SOURCE_ALLOW_LIST.to_string()),
            },
            other => other,
        }
    }

    fn annotate_pre_execute_error(error: TalosError) -> TalosError {
        match error {
            TalosError::ValidationError(message) => TalosError::ToolFailure {
                kind: TalosToolErrorKind::ValidationFailure,
                message,
                gate_source: Some(GATE_SOURCE_TOOL_SEMANTIC.to_string()),
            },
            TalosError::AuthorizationDenied => TalosError::ToolFailure {
                kind: TalosToolErrorKind::PermissionRejection,
                message: "authorization denied".to_string(),
                gate_source: Some(GATE_SOURCE_TOOL_SEMANTIC.to_string()),
            },
            other => other,
        }
    }

    fn is_static_policy_exempt(spec: &RegisteredToolSpec) -> bool {
        matches!(spec.source, ToolSource::Mcp)
    }

    fn requires_tool_search_activation(spec: &RegisteredToolSpec) -> bool {
        matches!(spec.source, ToolSource::Mcp | ToolSource::Dynamic)
    }

    fn normalize_registered_spec(mut spec: RegisteredToolSpec) -> RegisteredToolSpec {
        if Self::requires_tool_search_activation(&spec)
            && !matches!(spec.visibility, ToolVisibility::Disabled)
        {
            spec.visibility = ToolVisibility::Deferred;
        }
        spec
    }

    fn truncate_structured_error(
        value: &mut Option<Value>,
        max_output_bytes: usize,
    ) -> Option<(usize, usize)> {
        let payload = value.as_mut()?;
        let object = payload.as_object_mut()?;
        let error_value = object.get_mut("error")?;
        let error_text = error_value.as_str()?;

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
        let source = tool.source();
        let spec = RegisteredToolSpec::new(
            ToolDefinition::new(tool.name(), tool.description(), tool.parameters_schema()),
            source,
            tool.payload_kind(),
            tool.parallelism(),
            mutating,
            Self::default_governance(tool.internal_name(), source),
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

    fn default_governance(name: &str, source: ToolSource) -> ToolGovernanceSpec {
        match name {
            "bash" => ToolGovernanceSpec {
                preferred_sandbox: ToolSandboxMode::UseDefault,
                argument_override: ToolGovernanceArgumentOverride::BashCommandPermissions,
                retry_without_sandbox: true,
            },
            _ => default_governance_for_source(source),
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
        if matches!(spec.source, ToolSource::Collaboration) {
            return true;
        }

        match name {
            "Bash" | "Read" | "Grep" | "Glob" | "Edit" | "Write" => true,
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
            "TodoWrite" => Self::input_contains_any(
                request,
                &["todo", "plan", "checklist", "step", "tasks", "task list"],
            ),
            "AskUserQuestion" => {
                Self::input_contains_any(request, &["clarify", "question", "ask", "need info"])
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
            agent_control: DashMap::new(),
            memory_reader: DashMap::new(),
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
        let spec = Self::normalize_registered_spec(spec);
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
        let handler = Arc::new(DynamicToolHandler::new(spec.source, func));
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
            if !Self::is_static_policy_exempt(spec) {
                self.policy_checker
                    .check_tool_access(spec.definition.name.as_str())
                    .map_err(Self::annotate_policy_error)?;
            }
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
            .filter(|spec| {
                Self::is_static_policy_exempt(spec)
                    || self.policy_checker.policy.is_allowed(&spec.definition.name)
            })
            .map(|spec| spec.definition.name.clone())
            .collect()
    }

    /// Check if a tool is available and allowed by policy.
    pub fn is_tool_available(&self, name: &str) -> bool {
        self.tool_catalog()
            .find(name)
            .map(|spec| {
                Self::is_static_policy_exempt(spec)
                    || self.policy_checker.policy.is_allowed(&spec.definition.name)
            })
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
        let ctx = if let Some(control) = self
            .agent_control
            .get("default")
            .map(|entry| entry.value().clone())
        {
            ctx.with_agent_control(control)
        } else {
            ctx
        };
        if let Some(reader) = self
            .memory_reader
            .get("default")
            .map(|entry| entry.value().clone())
        {
            ctx.with_memory_reader(reader)
        } else {
            ctx
        }
    }

    /// Configures the application-owned child Session control port.
    pub fn set_agent_control(&self, control: Arc<dyn AgentControl>) {
        self.agent_control.insert("default", control);
    }

    pub fn set_memory_reader(&self, reader: Arc<dyn MemoryReader>) {
        self.memory_reader.insert("default", reader);
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
#[path = "registry/registry_tests.rs"]
mod tests;
