pub mod context;
pub mod handlers;
pub mod invocation;
pub mod mcp;
pub mod output;
pub mod registry;
pub mod router;
pub mod runtimes;
pub mod spec;
pub mod r#trait;
pub mod trait_impl;

pub use context::ToolContext;
pub use handlers::{DynamicToolHandler, LocalToolHandler, McpToolHandler, ToolHandler};
pub use invocation::{ToolCallPayload, ToolDispatchContext, ToolInvocation};
pub use mcp::{McpConnectionManager, mcp_tool_name, split_mcp_tool_name};
pub use output::ToolOutputPayload;
pub use r#trait::{Tool, ToolOutput};
pub use registry::ToolRegistry;
pub use router::ToolRouter;
pub use spec::{RegisteredToolSpec, ToolCatalog, ToolParallelism, ToolPayloadKind, ToolSource};

// Built-in tools
pub mod tools {
    pub mod apply_patch;
    pub mod edit;
    pub mod grep_files;
    pub mod list_dir;
    pub mod memory;
    pub mod network;
    pub mod read_file;
    pub mod unified_exec;
    pub mod write;
}

// Re-export the built-in tools
pub use tools::{
    apply_patch::ApplyPatchTool,
    edit::EditTool,
    grep_files::GrepFilesTool,
    list_dir::ListDirTool,
    memory::{MemoryGetTool, MemorySearchTool},
    network::{HttpRequestTool, WebFetchTool, WebSearchTool},
    read_file::ReadFileTool,
    unified_exec::{ExecCommandTool, WriteStdinTool},
    write::WriteTool,
};

use std::sync::Arc;

pub fn register_core_local_tools(registry: &ToolRegistry) {
    // File tools
    registry.register(Arc::new(ReadFileTool));
    registry.register(Arc::new(WriteTool));
    registry.register(Arc::new(EditTool));
    registry.register(Arc::new(ListDirTool));
    registry.register(Arc::new(GrepFilesTool));
    registry.register(Arc::new(ApplyPatchTool));
    // Exec tools (runtime-based)
    registry.register(Arc::new(ExecCommandTool));
    registry.register(Arc::new(WriteStdinTool));
    // Memory tools
    registry.register(Arc::new(MemorySearchTool));
    registry.register(Arc::new(MemoryGetTool));
}

pub fn register_network_tools(registry: &ToolRegistry) {
    registry.register(Arc::new(WebSearchTool));
    registry.register(Arc::new(WebFetchTool));
    registry.register(Arc::new(HttpRequestTool));
}
