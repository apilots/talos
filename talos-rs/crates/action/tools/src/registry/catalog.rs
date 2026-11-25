use super::ToolRegistry;
use crate::{
    mcp::McpConnectionManager,
    spec::{ToolCatalog, ToolSource},
};
use std::sync::Arc;
use talos_llm_provider::ToolDefinition;

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

    /// Attach the MCP capability owner; its current directory is read on demand.
    pub fn register_mcp_server(&self, manager: Arc<McpConnectionManager>) {
        self.mcp_manager.insert("default", manager);
    }

    pub fn tool_catalog(&self) -> ToolCatalog {
        let mut specs = self
            .specs
            .iter()
            .map(|entry| entry.value().clone())
            .collect::<Vec<_>>();
        if let Some(manager) = self.mcp_manager.get("default") {
            specs.extend(manager.all_tool_specs());
        }
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
        let catalog = self.tool_catalog();
        if self.worker_environment.ripgrep_recovery().is_some()
            && catalog.find(name).is_some_and(|spec| {
                spec.source == ToolSource::Local && spec.internal_name == "grep"
            })
        {
            // A missing dependency may suspend the batch for one user action.
            return false;
        }
        catalog.supports_parallel(name)
    }

    pub fn tool_is_mutating(&self, name: &str) -> bool {
        self.tool_catalog()
            .find(name)
            .map(|spec| !spec.behavior_flags.read_only)
            .unwrap_or(true)
    }
}
