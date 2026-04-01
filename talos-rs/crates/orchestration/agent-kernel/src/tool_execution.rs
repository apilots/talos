use crate::ToolExecutionMode;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::sync::Arc;
use talos_config::AgentKernelConfig;
use talos_error::TalosResult;
use talos_tools::{ToolInvocation, ToolOutputPayload, ToolRegistry};
use tokio::task::JoinSet;

#[derive(Debug, Clone)]
pub struct ToolExecutionContext {
    pub session_id: String,
    pub user_id: String,
    pub kernel_config: Arc<AgentKernelConfig>,
    pub pre_approved_call_ids: HashSet<String>,
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
    pub payload: ToolOutputPayload,
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

fn build_tool_context(
    execution_context: &ToolExecutionContext,
    registry: &ToolRegistry,
    pre_approved: bool,
) -> talos_tools::ToolContext {
    let ctx = registry.build_context(
        execution_context.session_id.clone(),
        execution_context.user_id.clone(),
        Some(execution_context.kernel_config.clone()),
    );
    if pre_approved {
        ctx.with_pre_approved()
    } else {
        ctx
    }
}

async fn execute_single_tool_call(
    registry: Arc<ToolRegistry>,
    execution_context: &ToolExecutionContext,
    call: ToolExecutionInput,
) -> ToolExecutionOutput {
    let pre_approved = execution_context
        .pre_approved_call_ids
        .contains(&call.call_id);
    let tool_context = build_tool_context(execution_context, registry.as_ref(), pre_approved);
    let invocation = call.to_invocation();
    let result = registry
        .execute_invocation_with_context(invocation, tool_context)
        .await;
    match result {
        Ok(payload) => ToolExecutionOutput {
            call_id: call.call_id,
            tool_name: call.tool_name,
            is_error: payload.is_error(),
            payload,
        },
        Err(error) => ToolExecutionOutput {
            call_id: call.call_id,
            tool_name: call.tool_name,
            payload: ToolOutputPayload::Mcp {
                content: error.to_string(),
                is_error: true,
            },
            is_error: true,
        },
    }
}

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
        for call in calls {
            outputs.push(
                execute_single_tool_call(registry.clone(), execution_context, call.clone()).await,
            );
        }
        Ok(outputs)
    }
}

pub struct ParallelToolExecutor;

impl ParallelToolExecutor {
    fn batch_allows_parallel(registry: &ToolRegistry, calls: &[ToolExecutionInput]) -> bool {
        !calls.is_empty()
            && calls.iter().all(|call| {
                registry.tool_supports_parallel(&call.tool_name)
                    && !registry.tool_is_mutating(&call.tool_name)
            })
    }
}

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
        if !Self::batch_allows_parallel(registry.as_ref(), calls) {
            return SequentialToolExecutor
                .execute(execution_context, registry, calls)
                .await;
        }

        let mut join_set = JoinSet::new();
        for (index, call) in calls.iter().cloned().enumerate() {
            let registry = registry.clone();
            let pre_approved = execution_context
                .pre_approved_call_ids
                .contains(&call.call_id);
            let tool_context =
                build_tool_context(execution_context, registry.as_ref(), pre_approved);
            join_set.spawn(async move {
                let invocation = call.to_invocation();
                let result = registry
                    .execute_invocation_with_context(invocation, tool_context)
                    .await;
                let output = match result {
                    Ok(payload) => ToolExecutionOutput {
                        call_id: call.call_id,
                        tool_name: call.tool_name,
                        is_error: payload.is_error(),
                        payload,
                    },
                    Err(error) => ToolExecutionOutput {
                        call_id: call.call_id,
                        tool_name: call.tool_name,
                        payload: ToolOutputPayload::Mcp {
                            content: error.to_string(),
                            is_error: true,
                        },
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

impl ToolExecutionInput {
    pub fn to_invocation(&self) -> ToolInvocation {
        ToolInvocation::function(
            self.call_id.clone(),
            self.tool_name.clone(),
            self.arguments.clone(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use talos_tools::{Tool, ToolContext, ToolOutput};
    use tokio::time::{sleep, Duration};

    struct ObservedDelayTool {
        name: &'static str,
        concurrent: Arc<AtomicUsize>,
        max_concurrent: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl Tool for ObservedDelayTool {
        fn name(&self) -> &str {
            self.name
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
            let active = self.concurrent.fetch_add(1, Ordering::SeqCst) + 1;
            let _ = self.max_concurrent.fetch_max(active, Ordering::SeqCst);
            sleep(Duration::from_millis(delay_ms)).await;
            self.concurrent.fetch_sub(1, Ordering::SeqCst);
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
        let concurrent = Arc::new(AtomicUsize::new(0));
        let max_concurrent = Arc::new(AtomicUsize::new(0));
        registry.register(Arc::new(ObservedDelayTool {
            name: "read_file",
            concurrent: concurrent.clone(),
            max_concurrent: max_concurrent.clone(),
        }));
        let executor = ParallelToolExecutor;
        let execution_context = ToolExecutionContext {
            session_id: "s1".to_string(),
            user_id: "u1".to_string(),
            kernel_config: Arc::new(AgentKernelConfig::default()),
            pre_approved_call_ids: HashSet::new(),
        };
        let calls = vec![
            ToolExecutionInput {
                call_id: "call_1".to_string(),
                tool_name: "read_file".to_string(),
                arguments: json!({
                    "label": "first",
                    "delay_ms": 50
                }),
            },
            ToolExecutionInput {
                call_id: "call_2".to_string(),
                tool_name: "read_file".to_string(),
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
        assert_eq!(outputs[0].tool_name, "read_file");
        assert_eq!(outputs[1].tool_name, "read_file");
        assert!(max_concurrent.load(Ordering::SeqCst) >= 2);
    }

    #[tokio::test]
    async fn test_parallel_tool_executor_falls_back_for_mutating_batch() {
        let registry = Arc::new(ToolRegistry::new());
        let concurrent = Arc::new(AtomicUsize::new(0));
        let max_concurrent = Arc::new(AtomicUsize::new(0));
        registry.register(Arc::new(ObservedDelayTool {
            name: "read_file",
            concurrent: concurrent.clone(),
            max_concurrent: max_concurrent.clone(),
        }));
        registry.register(Arc::new(ObservedDelayTool {
            name: "apply_patch",
            concurrent,
            max_concurrent: max_concurrent.clone(),
        }));

        let executor = ParallelToolExecutor;
        let execution_context = ToolExecutionContext {
            session_id: "s1".to_string(),
            user_id: "u1".to_string(),
            kernel_config: Arc::new(AgentKernelConfig::default()),
            pre_approved_call_ids: HashSet::new(),
        };
        let calls = vec![
            ToolExecutionInput {
                call_id: "call_1".to_string(),
                tool_name: "read_file".to_string(),
                arguments: json!({
                    "label": "read",
                    "delay_ms": 50
                }),
            },
            ToolExecutionInput {
                call_id: "call_2".to_string(),
                tool_name: "apply_patch".to_string(),
                arguments: json!({
                    "label": "write",
                    "delay_ms": 50
                }),
            },
        ];

        let outputs = executor
            .execute(&execution_context, registry, &calls)
            .await
            .expect("mixed batch execution should succeed");

        assert_eq!(outputs.len(), 2);
        assert_eq!(outputs[0].call_id, "call_1");
        assert_eq!(outputs[1].call_id, "call_2");
        assert_eq!(max_concurrent.load(Ordering::SeqCst), 1);
    }
}
