use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde_json::Value;
use talos_error::TalosResult;

use crate::context::ToolContext;
use crate::output::ToolOutputPayload;
use crate::spec::{
    ToolBehaviorFlags, ToolParallelism, ToolPayloadKind, ToolResultPolicy, ToolSource,
    ToolVisibility,
};

/// Base trait for all tools
#[async_trait]
pub trait Tool: Send + Sync {
    /// Canonical public tool name.
    fn name(&self) -> &str;

    /// Internal implementation name. Defaults to canonical public name.
    fn internal_name(&self) -> &str {
        self.name()
    }

    /// Temporary or backward-compatible aliases.
    fn aliases(&self) -> &'static [&'static str] {
        &[]
    }

    /// Human-facing label.
    fn user_facing_name(&self) -> &str {
        self.name()
    }

    /// Tool description
    fn description(&self) -> &str;

    /// Tool input JSON schema.
    fn parameters_schema(&self) -> Option<Value> {
        None
    }

    /// Validate raw tool input before dispatch enters the runtime body.
    fn validate_input(&self, _input: &Value) -> TalosResult<()> {
        Ok(())
    }

    /// Check tool-specific permissions before dispatch enters the runtime body.
    fn check_permissions(&self, _input: &Value, _context: &ToolContext) -> TalosResult<()> {
        Ok(())
    }

    fn source(&self) -> ToolSource {
        ToolSource::Local
    }

    fn payload_kind(&self) -> ToolPayloadKind {
        ToolPayloadKind::Function
    }

    fn visibility(&self) -> ToolVisibility {
        ToolVisibility::DefaultVisible
    }

    fn parallelism(&self) -> ToolParallelism {
        if self.behavior_flags().concurrency_safe {
            ToolParallelism::ParallelSafe
        } else {
            ToolParallelism::SerialOnly
        }
    }

    fn mutating(&self) -> bool {
        !self.behavior_flags().read_only
    }

    fn behavior_flags(&self) -> ToolBehaviorFlags {
        ToolBehaviorFlags {
            read_only: true,
            destructive: false,
            concurrency_safe: false,
            requires_user_interaction: false,
        }
    }

    fn result_policy(&self) -> ToolResultPolicy {
        ToolResultPolicy::default()
    }

    /// Produce the structured tool payload consumed by the runtime.
    async fn execute_payload(
        &self,
        input: Value,
        context: ToolContext,
    ) -> TalosResult<ToolOutputPayload>;
}

pub fn decode_tool_input<T>(input: Value, tool_name: &str) -> TalosResult<T>
where
    T: DeserializeOwned,
{
    serde_json::from_value(input).map_err(|err| {
        talos_error::TalosError::ToolError(format!("invalid {} arguments: {}", tool_name, err))
    })
}

#[cfg(test)]
mod tests {
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
}
