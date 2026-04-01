use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use talos_error::{TalosError, TalosResult};

use crate::handlers::ToolHandler;
use crate::invocation::{ToolCallPayload, ToolDispatchContext, ToolInvocation};
use crate::output::ToolOutputPayload;
use crate::spec::ToolSource;

pub type DynamicToolFn = Arc<
    dyn Fn(Value, ToolDispatchContext) -> Pin<Box<dyn Future<Output = TalosResult<ToolOutputPayload>> + Send>>
        + Send
        + Sync,
>;

/// Wraps an async callback as a dynamically-registered tool handler.
///
/// Use this for tools whose implementation lives outside the static type system,
/// such as skill bridges or A2A gateway callbacks.
pub struct DynamicToolHandler {
    source: ToolSource,
    func: DynamicToolFn,
}

impl DynamicToolHandler {
    pub fn new(source: ToolSource, func: DynamicToolFn) -> Self {
        Self { source, func }
    }
}

#[async_trait]
impl ToolHandler for DynamicToolHandler {
    fn kind(&self) -> ToolSource {
        self.source.clone()
    }

    fn matches(&self, invocation: &ToolInvocation) -> bool {
        matches!(invocation.payload, ToolCallPayload::Function { .. })
    }

    async fn handle(
        &self,
        invocation: ToolInvocation,
        context: ToolDispatchContext,
    ) -> TalosResult<ToolOutputPayload> {
        let ToolCallPayload::Function { arguments } = invocation.payload else {
            return Err(TalosError::ToolError(format!(
                "Dynamic tool '{}' requires function payload",
                invocation.tool_name
            )));
        };
        (self.func)(arguments, context).await
    }
}
