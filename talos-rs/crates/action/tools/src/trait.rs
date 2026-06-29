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
#[path = "trait_tests.rs"]
mod tests;
