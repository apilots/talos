use async_trait::async_trait;
use serde_json::{json, Value};
use talos_error::{TalosError, TalosResult};

use crate::{Tool, ToolContext, ToolOutput};

pub struct SpawnAgentTool;
pub struct SendInputTool;
pub struct WaitAgentTool;
pub struct CloseAgentTool;
pub struct ResumeAgentTool;

#[async_trait]
impl Tool for SpawnAgentTool {
    fn name(&self) -> &str {
        "spawn_agent"
    }

    fn description(&self) -> &str {
        "Spawn a child agent session and optionally send an initial prompt."
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(json!({
            "type": "object",
            "properties": {
                "message": { "type": "string" }
            }
        }))
    }

    async fn execute(&self, input: Value, context: ToolContext) -> TalosResult<ToolOutput> {
        let runtime = context.collaboration_runtime().ok_or_else(|| {
            TalosError::ToolError("collaboration runtime is not configured".to_string())
        })?;
        let message = input
            .get("message")
            .and_then(Value::as_str)
            .map(str::to_string);
        let session_id = runtime.spawn_agent(context.user_id(), message).await?;
        Ok(ToolOutput {
            success: true,
            output: json!({ "session_id": session_id }).to_string(),
            error: None,
        })
    }
}

#[async_trait]
impl Tool for SendInputTool {
    fn name(&self) -> &str {
        "send_input"
    }

    fn description(&self) -> &str {
        "Send a prompt to a child agent session."
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(json!({
            "type": "object",
            "required": ["target", "message"],
            "properties": {
                "target": { "type": "string" },
                "message": { "type": "string" },
                "interrupt": { "type": "boolean" }
            }
        }))
    }

    async fn execute(&self, input: Value, context: ToolContext) -> TalosResult<ToolOutput> {
        let runtime = context.collaboration_runtime().ok_or_else(|| {
            TalosError::ToolError("collaboration runtime is not configured".to_string())
        })?;
        let target = required_string(&input, "target")?;
        let message = required_string(&input, "message")?;
        let interrupt = input
            .get("interrupt")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let submission_id = runtime.send_input(&target, &message, interrupt).await?;
        Ok(ToolOutput {
            success: true,
            output: json!({ "submission_id": submission_id }).to_string(),
            error: None,
        })
    }
}

#[async_trait]
impl Tool for WaitAgentTool {
    fn name(&self) -> &str {
        "wait_agent"
    }

    fn description(&self) -> &str {
        "Wait until one of the child agent sessions reaches a final state."
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(json!({
            "type": "object",
            "required": ["targets"],
            "properties": {
                "targets": {
                    "type": "array",
                    "items": { "type": "string" }
                },
                "timeout_ms": { "type": "number" }
            }
        }))
    }

    async fn execute(&self, input: Value, context: ToolContext) -> TalosResult<ToolOutput> {
        let runtime = context.collaboration_runtime().ok_or_else(|| {
            TalosError::ToolError("collaboration runtime is not configured".to_string())
        })?;
        let targets = input
            .get("targets")
            .and_then(Value::as_array)
            .ok_or_else(|| TalosError::ValidationError("targets must be an array".to_string()))?
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect::<Vec<_>>();
        if targets.is_empty() {
            return Err(TalosError::ValidationError(
                "targets must be non-empty".to_string(),
            ));
        }
        let timeout_ms = input
            .get("timeout_ms")
            .and_then(Value::as_u64)
            .unwrap_or(30_000);
        let statuses = runtime.wait(&targets, timeout_ms).await?;
        Ok(ToolOutput {
            success: true,
            output: json!({ "statuses": statuses }).to_string(),
            error: None,
        })
    }
}

#[async_trait]
impl Tool for CloseAgentTool {
    fn name(&self) -> &str {
        "close_agent"
    }

    fn description(&self) -> &str {
        "Close a child agent session and stop any in-flight task."
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(json!({
            "type": "object",
            "required": ["target"],
            "properties": {
                "target": { "type": "string" }
            }
        }))
    }

    async fn execute(&self, input: Value, context: ToolContext) -> TalosResult<ToolOutput> {
        let runtime = context.collaboration_runtime().ok_or_else(|| {
            TalosError::ToolError("collaboration runtime is not configured".to_string())
        })?;
        let target = required_string(&input, "target")?;
        let status = runtime.close_agent(&target).await?;
        Ok(ToolOutput {
            success: true,
            output: json!({ "status": status }).to_string(),
            error: None,
        })
    }
}

#[async_trait]
impl Tool for ResumeAgentTool {
    fn name(&self) -> &str {
        "resume_agent"
    }

    fn description(&self) -> &str {
        "Re-open a known child agent session handle if it still exists."
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(json!({
            "type": "object",
            "required": ["target"],
            "properties": {
                "target": { "type": "string" }
            }
        }))
    }

    async fn execute(&self, input: Value, context: ToolContext) -> TalosResult<ToolOutput> {
        let runtime = context.collaboration_runtime().ok_or_else(|| {
            TalosError::ToolError("collaboration runtime is not configured".to_string())
        })?;
        let target = required_string(&input, "target")?;
        let status = runtime.resume_agent(&target).await?;
        Ok(ToolOutput {
            success: true,
            output: json!({ "status": status }).to_string(),
            error: None,
        })
    }
}

fn required_string(input: &Value, key: &str) -> TalosResult<String> {
    input
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| TalosError::ValidationError(format!("{key} must be a string")))
}
