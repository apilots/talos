pub mod collaboration;
pub mod context;
pub mod handlers;
pub mod invocation;
pub mod mcp;
pub mod orchestrator;
pub mod output;
pub mod registry;
pub mod router;
pub mod runtimes;
pub mod spec;
pub mod r#trait;
pub mod trait_impl;

pub use collaboration::{CollaborationAgentStatus, CollaborationRuntime};
pub use context::ToolContext;
pub use handlers::{DynamicToolHandler, LocalToolHandler, McpToolHandler, ToolHandler};
pub use invocation::{ToolCallPayload, ToolDispatchContext, ToolInvocation};
pub use mcp::{mcp_tool_name, split_mcp_tool_name, McpConnectionManager};
pub use output::{ToolDispatchOutcome, ToolOutputPayload, ToolPending};
pub use r#trait::{decode_tool_input, Tool, ToolOutput};
pub use registry::{ToolDefinitionProvider, ToolExecutorPort, ToolRegistry, ToolRuntime};
pub use router::ToolRouter;
pub use spec::{
    default_governance_for_source, RegisteredToolSpec, ToolCatalog, ToolParallelism,
    ToolPayloadKind, ToolSource,
};

// Built-in tools
pub mod tools {
    pub mod apply_patch;
    pub mod collaboration;
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
    collaboration::{
        CloseAgentTool, ResumeAgentTool, SendInputTool, SpawnAgentTool, WaitAgentTool,
    },
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

pub fn register_collaboration_tools(registry: &ToolRegistry) {
    for tool in [
        Arc::new(SpawnAgentTool) as Arc<dyn Tool>,
        Arc::new(SendInputTool) as Arc<dyn Tool>,
        Arc::new(WaitAgentTool) as Arc<dyn Tool>,
        Arc::new(CloseAgentTool) as Arc<dyn Tool>,
        Arc::new(ResumeAgentTool) as Arc<dyn Tool>,
    ] {
        let definition = talos_llm_provider::ToolDefinition::new(
            tool.name(),
            tool.description(),
            tool.parameters_schema(),
        );
        let spec = RegisteredToolSpec::new(
            definition,
            ToolSource::Collaboration,
            ToolPayloadKind::Function,
            ToolParallelism::SerialOnly,
            true,
            default_governance_for_source(ToolSource::Collaboration),
        );
        registry.register_handler(
            tool.name().to_string(),
            spec,
            Arc::new(LocalToolHandler::new(tool, true)),
        );
    }
}
