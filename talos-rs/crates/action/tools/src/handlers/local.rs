use std::sync::Arc;

use async_trait::async_trait;
use talos_error::{TalosError, TalosResult};

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

        let output = self
            .tool
            .execute_payload(arguments, context.into_tool_context())
            .await?;
        Ok(ToolDispatchOutcome::executed(output))
    }
}

#[cfg(test)]
#[path = "local_tests.rs"]
mod tests;
