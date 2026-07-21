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
pub mod state;
pub mod r#trait;
pub mod trait_impl;

pub use collaboration::{CollaborationAgentHost, CollaborationAgentStatus, CollaborationRuntime};
pub use context::{ToolContext, ToolProgressEmitter, ToolProgressEvent, ToolProgressKind};
pub use handlers::{DynamicToolHandler, LocalToolHandler, McpToolHandler, ToolHandler};
pub use invocation::{ToolCallPayload, ToolDispatchContext, ToolInvocation};
pub use mcp::{mcp_tool_name, split_mcp_tool_name, McpConnectionManager};
pub use output::{
    NextTurnFailurePayload, ToolDispatchOutcome, ToolOutputPayload, ToolPending,
    ToolResultEnvelope, ToolResultError, ToolResultPendingMeta, ToolResultPolicyMeta,
    ToolResultStatus,
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
    PermissionMode, ReadFileStateEntry, ReadFileStateSource, ReadFileStateStore, SessionToolState,
    TaskStateEntry, TaskStateStore, ToolCatalogState, ToolPermissionContext, ToolResultBudgetState,
};

// Built-in tools
pub mod tools {
    pub mod agent;
    pub mod apply_patch;
    pub mod ask_user_question;
    pub mod bash;
    pub mod edit;
    pub mod enter_plan_mode;
    pub mod exit_plan_mode;
    pub mod glob;
    pub mod grep;
    pub mod memory_get;
    pub mod memory_search;
    pub mod read;
    pub mod todo_write;
    pub mod tool_search;
    pub mod web_fetch;
    pub mod web_search;
    pub mod write;
}

#[cfg(test)]
#[path = "tools/memory_tools_tests.rs"]
mod memory_tools_tests;

// Re-export the built-in tools
pub use tools::{
    agent::AgentTool, apply_patch::ApplyPatchTool, ask_user_question::AskUserQuestionTool,
    bash::BashTool, edit::EditTool, enter_plan_mode::EnterPlanModeTool,
    exit_plan_mode::ExitPlanModeTool, glob::GlobTool, grep::GrepTool, memory_get::MemoryGetTool,
    memory_search::MemorySearchTool, read::ReadTool, todo_write::TodoWriteTool,
    tool_search::ToolSearchTool, web_fetch::WebFetchTool, web_search::WebSearchTool,
    write::WriteTool,
};

use std::sync::Arc;

pub fn register_core_local_tools(registry: &ToolRegistry) {
    // File tools
    registry.register(Arc::new(ReadTool));
    registry.register(Arc::new(WriteTool));
    registry.register(Arc::new(EditTool));
    registry.register(Arc::new(ApplyPatchTool));
    registry.register(Arc::new(GlobTool));
    registry.register(Arc::new(GrepTool));
    // Exec tools (runtime-based)
    registry.register(Arc::new(BashTool));
    // Memory tools
    registry.register(Arc::new(MemorySearchTool));
    registry.register(Arc::new(MemoryGetTool));
    // Catalog and workflow tools
    registry.register(Arc::new(ToolSearchTool));
    registry.register(Arc::new(TodoWriteTool));
    registry.register(Arc::new(EnterPlanModeTool));
    registry.register(Arc::new(ExitPlanModeTool));
    registry.register(Arc::new(AskUserQuestionTool));
}

pub fn register_network_tools(registry: &ToolRegistry) {
    registry.register(Arc::new(WebSearchTool));
    registry.register(Arc::new(WebFetchTool));
}

pub fn register_collaboration_tools(registry: &ToolRegistry) {
    let tool = Arc::new(AgentTool) as Arc<dyn Tool>;
    let definition = talos_llm_provider::ToolDefinition::new(
        tool.name(),
        tool.description(),
        tool.parameters_schema(),
    );
    let spec = RegisteredToolSpec::new(
        definition,
        ToolSource::Collaboration,
        ToolPayloadKind::Function,
        tool.parallelism(),
        tool.mutating(),
        default_governance_for_source(ToolSource::Collaboration),
    )
    .with_internal_name(tool.internal_name())
    .with_aliases(tool.aliases().iter().copied())
    .with_visibility(tool.visibility())
    .with_behavior_flags(tool.behavior_flags())
    .with_result_policy(tool.result_policy())
    .with_user_facing_name(tool.user_facing_name());
    registry.register_handler(
        tool.internal_name().to_string(),
        spec,
        Arc::new(LocalToolHandler::new(tool, true)),
    );
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
