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
