use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use talos_error::{TalosError, TalosResult};

use crate::handlers::ToolHandler;
use crate::invocation::{ToolCallPayload, ToolDispatchContext, ToolInvocation};
use crate::output::{ToolDispatchOutcome, ToolOutputPayload};
use crate::spec::ToolSource;

pub type DynamicToolFn = Arc<
    dyn Fn(
            Value,
            ToolDispatchContext,
        ) -> Pin<Box<dyn Future<Output = TalosResult<ToolOutputPayload>> + Send>>
        + Send
        + Sync,
>;

/// Wraps an async callback as a dynamically-registered tool handler.
///
/// Use this for tools whose implementation lives outside the static type system,
/// such as skill bridges or A2A gateway callbacks.
pub struct DynamicToolHandler {
    source: ToolSource,
    backend: DynamicToolBackend,
}

enum DynamicToolBackend {
    Inline(DynamicToolFn),
    External,
}

impl DynamicToolHandler {
    pub fn new(source: ToolSource, func: DynamicToolFn) -> Self {
        Self {
            source,
            backend: DynamicToolBackend::Inline(func),
        }
    }

    pub fn external(source: ToolSource) -> Self {
        Self {
            source,
            backend: DynamicToolBackend::External,
        }
    }
}

#[async_trait]
impl ToolHandler for DynamicToolHandler {
    fn kind(&self) -> ToolSource {
        self.source
    }

    fn matches(&self, invocation: &ToolInvocation) -> bool {
        matches!(invocation.payload, ToolCallPayload::Function { .. })
    }

    async fn handle(
        &self,
        invocation: ToolInvocation,
        context: ToolDispatchContext,
    ) -> TalosResult<ToolDispatchOutcome> {
        let ToolCallPayload::Function { arguments } = invocation.payload else {
            return Err(TalosError::public(
                talos_error::ErrorCode::ToolExecutionFailed,
                format!(
                    "Dynamic tool '{}' requires function payload",
                    invocation.tool_name
                ),
            ));
        };
        match &self.backend {
            DynamicToolBackend::Inline(func) => {
                let payload = (func)(arguments, context).await?;
                Ok(ToolDispatchOutcome::executed(payload))
            }
            DynamicToolBackend::External => {
                if let Some(result) =
                    load_dynamic_tool_result(context.tool_context(), invocation.call_id.as_str())?
                {
                    let payload = if result.success {
                        ToolOutputPayload::success_text(result.output)
                    } else {
                        ToolOutputPayload::failure_structured(result.output, None)
                    };
                    Ok(ToolDispatchOutcome::executed(payload))
                } else {
                    Ok(ToolDispatchOutcome::pending_dynamic(
                        invocation.tool_name,
                        arguments,
                    ))
                }
            }
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct StoredDynamicToolResult {
    pub output: String,
    pub success: bool,
}

fn load_dynamic_tool_result(
    context: &crate::ToolContext,
    call_id: &str,
) -> TalosResult<Option<StoredDynamicToolResult>> {
    Ok(context.get_dynamic_tool_result(call_id))
}
