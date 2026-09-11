use super::ToolRegistry;
use crate::{mcp::McpConnectionManager, spec::ToolCatalog};
use std::sync::Arc;

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
}
