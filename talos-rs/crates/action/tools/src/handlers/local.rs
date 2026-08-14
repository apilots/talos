use std::sync::Arc;

use async_trait::async_trait;
use talos_error::{TalosError, TalosResult};
use talos_tools_sandbox::RequestedFileSystemAccess;

use crate::handlers::ToolHandler;
use crate::invocation::{ToolCallPayload, ToolDispatchContext, ToolInvocation};
use crate::output::ToolDispatchOutcome;
use crate::r#trait::Tool;
use crate::spec::ToolSource;

pub struct LocalToolHandler {
    tool: Arc<dyn Tool>,
    mutating: bool,
}

impl LocalToolHandler {
    pub fn new(tool: Arc<dyn Tool>, mutating: bool) -> Self {
        Self { tool, mutating }
    }
}

#[async_trait]
impl ToolHandler for LocalToolHandler {
    fn kind(&self) -> ToolSource {
        ToolSource::Local
    }

    fn matches(&self, invocation: &ToolInvocation) -> bool {
        matches!(invocation.payload, ToolCallPayload::Function { .. })
    }

    async fn pre_execute(
        &self,
        invocation: &ToolInvocation,
        context: &ToolDispatchContext,
    ) -> TalosResult<()> {
        let ToolCallPayload::Function { arguments } = &invocation.payload else {
            return Err(TalosError::ToolError(format!(
                "Local tool '{}' requires function payload",
                invocation.tool_name
            )));
        };

        self.tool.validate_input(arguments)?;
        self.tool
            .check_permissions(arguments, context.tool_context())?;
        Ok(())
    }

    async fn is_mutating(&self, _invocation: &ToolInvocation) -> bool {
        self.mutating
    }

    fn requested_file_system_access(
        &self,
        invocation: &ToolInvocation,
        context: &ToolDispatchContext,
    ) -> TalosResult<Vec<RequestedFileSystemAccess>> {
        let ToolCallPayload::Function { arguments } = &invocation.payload else {
            return Err(TalosError::ToolError(format!(
                "Local tool '{}' requires function payload",
                invocation.tool_name
            )));
        };
        self.tool
            .requested_file_system_access(arguments, context.tool_context())
    }

    async fn handle(
        &self,
        invocation: ToolInvocation,
        context: ToolDispatchContext,
    ) -> TalosResult<ToolDispatchOutcome> {
        let ToolCallPayload::Function { arguments } = invocation.payload else {
            return Err(TalosError::ToolError(format!(
                "Local tool '{}' requires function payload",
                invocation.tool_name
            )));
        };

        self.tool
            .execute_outcome(arguments, context.into_tool_context())
            .await
    }
}

#[cfg(test)]
#[path = "local_tests.rs"]
mod tests;
