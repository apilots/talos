use crate::ToolExecutionMode;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use talos_config::AgentKernelConfig;
use talos_error::TalosResult;
use talos_tools::ToolRegistry;
use tokio::task::JoinSet;

#[derive(Debug, Clone)]
pub struct ToolExecutionContext {
    pub session_id: String,
    pub user_id: String,
    pub kernel_config: Arc<AgentKernelConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolExecutionInput {
    pub call_id: String,
    pub tool_name: String,
    #[serde(default)]
    pub arguments: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolExecutionOutput {
    pub call_id: String,
    pub tool_name: String,
    #[serde(default)]
    pub payload: Value,
    #[serde(default)]
    pub is_error: bool,
}

#[async_trait]
pub trait ToolExecutor: Send + Sync {
    fn mode(&self) -> ToolExecutionMode;

    async fn execute(
        &self,
        _execution_context: &ToolExecutionContext,
        _registry: Arc<ToolRegistry>,
        _calls: &[ToolExecutionInput],
    ) -> TalosResult<Vec<ToolExecutionOutput>> {
        Ok(Vec::new())
    }
}

pub struct SequentialToolExecutor;

#[async_trait]
impl ToolExecutor for SequentialToolExecutor {
    fn mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Sequential
    }

    async fn execute(
        &self,
        execution_context: &ToolExecutionContext,
        registry: Arc<ToolRegistry>,
        calls: &[ToolExecutionInput],
    ) -> TalosResult<Vec<ToolExecutionOutput>> {
        let mut outputs = Vec::with_capacity(calls.len());
        let tool_context = registry.build_context(
            execution_context.session_id.clone(),
            execution_context.user_id.clone(),
            Some(execution_context.kernel_config.clone()),
        );
        for call in calls {
            let result = registry
                .execute_with_context(
                    &call.tool_name,
                    call.arguments.clone(),
                    tool_context.clone(),
                )
                .await;
            match result {
                Ok(payload) => outputs.push(ToolExecutionOutput {
                    call_id: call.call_id.clone(),
                    tool_name: call.tool_name.clone(),
                    payload: serde_json::json!({
                        "success": payload.success,
                        "output": payload.output,
                        "error": payload.error,
                    }),
                    is_error: !payload.success,
                }),
                Err(error) => outputs.push(ToolExecutionOutput {
                    call_id: call.call_id.clone(),
                    tool_name: call.tool_name.clone(),
                    payload: serde_json::json!({ "error": error.to_string() }),
                    is_error: true,
                }),
            }
        }
        Ok(outputs)
    }
}

pub struct ParallelToolExecutor;

#[async_trait]
impl ToolExecutor for ParallelToolExecutor {
    fn mode(&self) -> ToolExecutionMode {
        ToolExecutionMode::Parallel
    }

    async fn execute(
        &self,
        execution_context: &ToolExecutionContext,
        registry: Arc<ToolRegistry>,
        calls: &[ToolExecutionInput],
    ) -> TalosResult<Vec<ToolExecutionOutput>> {
        let mut join_set = JoinSet::new();
        for (index, call) in calls.iter().cloned().enumerate() {
            let registry = registry.clone();
            let tool_context = registry.build_context(
                execution_context.session_id.clone(),
                execution_context.user_id.clone(),
                Some(execution_context.kernel_config.clone()),
            );
            join_set.spawn(async move {
                let result = registry
                    .execute_with_context(&call.tool_name, call.arguments.clone(), tool_context)
                    .await;
                let output = match result {
                    Ok(payload) => ToolExecutionOutput {
                        call_id: call.call_id,
                        tool_name: call.tool_name,
                        payload: serde_json::json!({
                            "success": payload.success,
                            "output": payload.output,
                            "error": payload.error,
                        }),
                        is_error: !payload.success,
                    },
                    Err(error) => ToolExecutionOutput {
                        call_id: call.call_id,
                        tool_name: call.tool_name,
                        payload: serde_json::json!({ "error": error.to_string() }),
                        is_error: true,
                    },
                };
                (index, output)
            });
        }

        let mut outputs = vec![None; calls.len()];
        while let Some(result) = join_set.join_next().await {
            let (index, output) = result.expect("parallel tool execution task should not panic");
            outputs[index] = Some(output);
        }
        Ok(outputs
            .into_iter()
            .map(|output| output.expect("parallel tool output should exist"))
            .collect())
    }
}

pub struct ToolExecutorFactory;

impl ToolExecutorFactory {
    pub fn build(mode: ToolExecutionMode) -> Arc<dyn ToolExecutor> {
        match mode {
            ToolExecutionMode::Sequential => Arc::new(SequentialToolExecutor),
            ToolExecutionMode::Parallel => Arc::new(ParallelToolExecutor),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use serde_json::json;
    use talos_tools::{Tool, ToolContext, ToolOutput};
    use tokio::time::{sleep, Duration};

    struct DelayEchoTool;

    #[async_trait]
    impl Tool for DelayEchoTool {
        fn name(&self) -> &str {
            "delay_echo"
        }

        fn description(&self) -> &str {
            "Return the provided label after a configurable delay"
        }

        fn parameters_schema(&self) -> Option<Value> {
            Some(json!({
                "type": "object",
                "required": ["label", "delay_ms"],
                "properties": {
                    "label": { "type": "string" },
                    "delay_ms": { "type": "integer" }
                }
            }))
        }

        async fn execute(&self, input: Value, _context: ToolContext) -> TalosResult<ToolOutput> {
            let label = input
                .get("label")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let delay_ms = input
                .get("delay_ms")
                .and_then(Value::as_u64)
                .unwrap_or_default();
            sleep(Duration::from_millis(delay_ms)).await;
            Ok(ToolOutput {
                success: true,
                output: label,
                error: None,
            })
        }
    }

    #[tokio::test]
    async fn test_parallel_tool_executor_preserves_input_order() {
        let registry = Arc::new(ToolRegistry::new());
        registry.register(Arc::new(DelayEchoTool));
        let executor = ParallelToolExecutor;
        let execution_context = ToolExecutionContext {
            session_id: "s1".to_string(),
            user_id: "u1".to_string(),
            kernel_config: Arc::new(AgentKernelConfig::default()),
        };
        let calls = vec![
            ToolExecutionInput {
                call_id: "call_1".to_string(),
                tool_name: "delay_echo".to_string(),
                arguments: json!({
                    "label": "first",
                    "delay_ms": 50
                }),
            },
            ToolExecutionInput {
                call_id: "call_2".to_string(),
                tool_name: "delay_echo".to_string(),
                arguments: json!({
                    "label": "second",
                    "delay_ms": 1
                }),
            },
        ];

        let outputs = executor
            .execute(&execution_context, registry, &calls)
            .await
            .expect("parallel execution should succeed");

        assert_eq!(outputs.len(), 2);
        assert_eq!(outputs[0].call_id, "call_1");
        assert_eq!(outputs[1].call_id, "call_2");
        assert_eq!(outputs[0].tool_name, "delay_echo");
        assert_eq!(outputs[1].tool_name, "delay_echo");
    }
}
