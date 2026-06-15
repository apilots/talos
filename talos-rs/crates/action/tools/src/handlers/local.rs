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
mod tests {
    use super::*;
    use crate::context::ToolContext;
    use crate::output::ToolOutputPayload;
    use serde_json::json;

    struct PayloadFirstTool;

    #[async_trait]
    impl Tool for PayloadFirstTool {
        fn name(&self) -> &str {
            "payload_first_tool"
        }

        fn description(&self) -> &str {
            "prefer execute_payload in local handler"
        }

        async fn execute_payload(
            &self,
            _input: serde_json::Value,
            _context: ToolContext,
        ) -> TalosResult<ToolOutputPayload> {
            Ok(ToolOutputPayload::success_structured(
                json!({"result": "payload"}),
            ))
        }
    }

    #[tokio::test]
    async fn local_handler_prefers_structured_execute_payload() {
        let handler = LocalToolHandler::new(Arc::new(PayloadFirstTool), false);
        let invocation = ToolInvocation::function("call-1", "payload_first_tool", json!({}));
        let context = ToolDispatchContext::new(ToolContext::default());

        let outcome = handler
            .handle(invocation, context)
            .await
            .expect("local handler should execute");
        let payload = outcome.payload().expect("payload should exist");

        match payload {
            ToolOutputPayload::Function {
                structured_content, ..
            } => {
                assert_eq!(structured_content, &Some(json!({"result": "payload"})));
            }
            other => panic!("expected function payload, got {other:?}"),
        }
    }
}
