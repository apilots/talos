use super::*;
use talos_error::TalosError;

struct DefaultGuardTool;

#[async_trait]
impl Tool for DefaultGuardTool {
    fn name(&self) -> &str {
        "default_guard_tool"
    }

    fn description(&self) -> &str {
        "default guard tool"
    }

    async fn execute_payload(
        &self,
        _input: Value,
        _context: ToolContext,
    ) -> TalosResult<ToolOutputPayload> {
        Ok(ToolOutputPayload::success_text("ok"))
    }
}

struct CustomGuardTool;

#[async_trait]
impl Tool for CustomGuardTool {
    fn name(&self) -> &str {
        "custom_guard_tool"
    }

    fn description(&self) -> &str {
        "custom guard tool"
    }

    fn validate_input(&self, _input: &Value) -> TalosResult<()> {
        Err(TalosError::ValidationError("input rejected".to_string()))
    }

    fn check_permissions(&self, _input: &Value, _context: &ToolContext) -> TalosResult<()> {
        Err(TalosError::AuthorizationDenied)
    }

    async fn execute_payload(
        &self,
        _input: Value,
        _context: ToolContext,
    ) -> TalosResult<ToolOutputPayload> {
        Ok(ToolOutputPayload::success_text("ok"))
    }
}

#[test]
fn default_guards_allow_input_and_permissions() {
    let tool = DefaultGuardTool;
    let input = serde_json::json!({"path": "demo.txt"});
    let context = ToolContext::anonymous();

    assert!(tool.validate_input(&input).is_ok());
    assert!(tool.check_permissions(&input, &context).is_ok());
}

#[test]
fn custom_guards_can_override_validation_and_permissions() {
    let tool = CustomGuardTool;
    let input = serde_json::json!({"path": "demo.txt"});
    let context = ToolContext::anonymous();

    let validation_error = tool.validate_input(&input).unwrap_err();
    assert!(matches!(validation_error, TalosError::ValidationError(_)));

    let permission_error = tool.check_permissions(&input, &context).unwrap_err();
    assert!(matches!(permission_error, TalosError::AuthorizationDenied));
}

#[tokio::test]
async fn execute_payload_is_the_required_runtime_contract() {
    let tool = DefaultGuardTool;
    let payload = tool
        .execute_payload(serde_json::json!({}), ToolContext::default())
        .await
        .expect("payload execution should succeed");

    match payload {
        ToolOutputPayload::Function {
            success,
            structured_content,
            ..
        } => {
            assert!(success);
            assert_eq!(
                structured_content,
                Some(serde_json::json!({"result": "ok"}))
            );
        }
        other => panic!("expected function payload, got {other:?}"),
    }
}
