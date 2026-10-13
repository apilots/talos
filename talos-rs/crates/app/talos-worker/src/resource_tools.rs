//! Invocation-local product capabilities over the existing node tool registry.
use std::sync::Arc;
use talos_tools::{
    AgentControl, ToolContext, ToolDefinitionProvider, ToolDispatchOutcome, ToolDispatchResult,
    ToolExecutorPort, ToolImageStore, ToolInvocation, ToolRegistry, ToolSelection,
    ToolSelectionRequest,
};

pub(crate) struct ResourceTools {
    pub registry: Arc<ToolRegistry>,
    pub images: Arc<dyn ToolImageStore>,
    pub control: Option<Arc<dyn AgentControl>>,
}
impl ToolDefinitionProvider for ResourceTools {
    fn get_tool_definitions(&self) -> Vec<talos_llm_provider::ToolDefinition> {
        self.registry.get_tool_definitions()
    }
    fn get_tool_selection(&self, request: &ToolSelectionRequest) -> ToolSelection {
        self.registry.get_tool_selection(request)
    }
}
#[async_trait::async_trait]
impl ToolExecutorPort for ResourceTools {
    fn build_function_invocation(
        &self,
        call_id: String,
        tool_name: String,
        arguments: serde_json::Value,
    ) -> talos_error::TalosResult<ToolInvocation> {
        self.registry
            .build_function_invocation(call_id, tool_name, arguments)
    }
    async fn execute_invocation_with_context(
        &self,
        invocation: ToolInvocation,
        context: ToolContext,
    ) -> ToolDispatchResult<ToolDispatchOutcome> {
        self.registry
            .execute_invocation_with_context(invocation, context)
            .await
    }
    fn tool_supports_parallel(&self, name: &str) -> bool {
        self.registry.tool_supports_parallel(name)
    }
    fn tool_is_mutating(&self, name: &str) -> bool {
        self.registry.tool_is_mutating(name)
    }
    fn image_store(&self) -> Option<Arc<dyn ToolImageStore>> {
        Some(self.images.clone())
    }
    fn build_context(
        &self,
        session_id: String,
        user_id: String,
        config: Option<Arc<talos_config::AgentKernelConfig>>,
    ) -> ToolContext {
        let context = self
            .registry
            .build_context(session_id, user_id, config)
            .with_image_store(self.images.clone());
        if let Some(control) = &self.control {
            context.with_agent_control(control.clone())
        } else {
            context
        }
    }
}
