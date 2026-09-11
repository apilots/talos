mod image;
pub use image::ToolImageStore;
pub mod command_risk;
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
pub mod state;
pub mod subagent;
pub mod r#trait;
pub mod trait_impl;

#[cfg(test)]
mod test_support;

#[cfg(test)]
mod sandbox_e2e_tests;

pub use command_risk::{
    BashCommandFacts, CommandRiskAssessmentError, CommandRiskAssessmentInput, CommandRiskAssessor,
    EffectivePermissionSummary, LlmCommandRiskAssessor,
};
pub use context::{
    InvocationCancellation, ToolContext, ToolProgressEmitter, ToolProgressEvent, ToolProgressKind,
};
pub use handlers::{DynamicToolHandler, LocalToolHandler, McpToolHandler, ToolHandler};
pub use invocation::{ToolCallPayload, ToolDispatchContext, ToolInvocation};
pub use mcp::{mcp_tool_name, split_mcp_tool_name, McpConnectionManager};
pub use output::{
    NextTurnFailurePayload, TalosToolErrorKind, ToolDispatchFailure, ToolDispatchOutcome,
    ToolDispatchResult, ToolOutputPayload, ToolPending, ToolResultEnvelope, ToolResultError,
    ToolResultPendingMeta, ToolResultPolicyMeta, ToolResultStatus,
};
pub use r#trait::{decode_tool_input, Tool};
pub use registry::{ToolDefinitionProvider, ToolExecutorPort, ToolRegistry, ToolRuntime};
pub use router::ToolRouter;
pub use spec::{
    default_governance_for_source, RegisteredToolSpec, ToolCatalog, ToolParallelism,
    ToolPayloadKind, ToolSelection, ToolSelectionPhase, ToolSelectionRequest, ToolSource,
};
pub use state::{
    get_session_tool_state, CatalogToolEntry, ContentReplacementState, DeferredToolState,
    ReadFileStateEntry, ReadFileStateSource, ReadFileStateStore, SessionToolState, TaskStateEntry,
    TaskStateStore, ToolCatalogState, ToolResultBudgetState,
};
pub use subagent::{AgentControl, AgentControlFuture};
pub use talos_tools_sandbox::{AbsolutePathBuf, ToolSandboxEnvironment};

// Built-in tools
pub mod tools {
    pub mod ask_user_question;
    pub mod bash;
    pub mod edit;
    pub mod glob;
    pub mod grep;
    pub mod memory_get;
    pub mod memory_search;
    pub mod read;
    pub(super) mod subagent;
    pub mod todo_write;
    pub mod tool_search;
    pub mod user_workspace_memory_update;
    pub mod web_fetch;
    pub mod web_search;
    pub mod write;
}

#[cfg(test)]
#[path = "tools/memory_tools_tests.rs"]
mod memory_tools_tests;

// Re-export the built-in tools
pub use tools::{
    ask_user_question::AskUserQuestionTool, bash::BashTool, edit::EditTool, glob::GlobTool,
    grep::GrepTool, memory_get::MemoryGetTool, memory_search::MemorySearchTool, read::ReadTool,
    todo_write::TodoWriteTool, tool_search::ToolSearchTool,
    user_workspace_memory_update::UserWorkspaceMemoryUpdateTool, web_fetch::WebFetchTool,
    web_search::WebSearchTool, write::WriteTool,
};

use std::sync::Arc;

pub fn register_core_local_tools(registry: &ToolRegistry) {
    // File tools
    registry.register(Arc::new(ReadTool));
    registry.register(Arc::new(WriteTool));
    registry.register(Arc::new(EditTool));
    registry.register(Arc::new(GlobTool));
    registry.register(Arc::new(GrepTool));
    // Exec tools (runtime-based)
    registry.register(Arc::new(BashTool));
    // Memory tools
    registry.register(Arc::new(MemorySearchTool));
    registry.register(Arc::new(MemoryGetTool));
    registry.register(Arc::new(UserWorkspaceMemoryUpdateTool));
    // Catalog and workflow tools
    registry.register(Arc::new(ToolSearchTool));
    registry.register(Arc::new(TodoWriteTool));
    registry.register(Arc::new(AskUserQuestionTool));
}

pub fn register_network_tools(registry: &ToolRegistry) {
    registry.register(Arc::new(WebSearchTool));
    registry.register(Arc::new(WebFetchTool));
}

/// Registers the complete formal subagent Tool set as one release surface.
pub fn register_subagent_tools(registry: &ToolRegistry) {
    for tool in tools::subagent::subagent_tools() {
        registry.register(tool);
    }
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
