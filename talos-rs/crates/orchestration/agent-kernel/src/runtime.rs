use crate::{Agent, DefaultRunner, KernelWarning, RunRequest, Runner, StopReason, TaskResult};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use talos_config::AgentKernelConfig;
use talos_error::TalosResult;
use talos_types::talos::{TalosOutputItem, TalosUsage};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KernelOutput {
    pub output_text: String,
    pub output_items: Vec<TalosOutputItem>,
    pub stop_reason: StopReason,
    pub usage: TalosUsage,
    pub warnings: Vec<KernelWarning>,
    pub model: String,
    pub task_results: Vec<TaskResult>,
    pub transfer_to: Option<String>,
    pub iterations: u32,
}

pub struct KernelBuilder {
    runner: Option<Arc<dyn Runner>>,
    default_kernel_config: AgentKernelConfig,
}

impl KernelBuilder {
    pub fn new() -> Self {
        Self {
            runner: None,
            default_kernel_config: AgentKernelConfig::default(),
        }
    }

    pub fn with_runner(mut self, runner: Arc<dyn Runner>) -> Self {
        self.runner = Some(runner);
        self
    }

    pub fn with_kernel_config(mut self, config: AgentKernelConfig) -> Self {
        self.default_kernel_config = config;
        self
    }

    pub fn build(self) -> KernelRuntime {
        KernelRuntime {
            runner: self
                .runner
                .unwrap_or_else(|| Arc::new(DefaultRunner::new())),
            default_kernel_config: self.default_kernel_config,
        }
    }
}

impl Default for KernelBuilder {
    fn default() -> Self {
        Self::new()
    }
}

pub struct KernelRuntime {
    runner: Arc<dyn Runner>,
    default_kernel_config: AgentKernelConfig,
}

impl KernelRuntime {
    pub fn new_default() -> Self {
        KernelBuilder::new().build()
    }

    pub fn default_kernel_config(&self) -> &AgentKernelConfig {
        &self.default_kernel_config
    }

    pub async fn run_turn(
        &self,
        agent: Arc<dyn Agent>,
        request: RunRequest,
    ) -> TalosResult<KernelOutput> {
        let response = self.runner.run_turn(agent, request).await?;
        Ok(KernelOutput {
            output_text: response.output_text,
            output_items: response.output_items,
            stop_reason: response.stop_reason,
            usage: response.usage,
            warnings: response.warnings,
            model: response.model,
            task_results: response.task_results,
            transfer_to: response.transfer_to,
            iterations: response.iterations,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TurnPhase;
    use async_trait::async_trait;
    use std::collections::HashMap;
    use talos_types::talos::{TalosOutputItem, TalosUsage};

    struct MockKernelAgent;

    #[async_trait]
    impl Agent for MockKernelAgent {
        fn id(&self) -> &str {
            "mock"
        }

        async fn run(
            &self,
            _ctx: &mut crate::InvocationContext,
            _event_handler: Option<&mut dyn crate::ExecutionEventHandler>,
        ) -> TalosResult<crate::AgentRunResult> {
            Ok(crate::AgentRunResult {
                output_text: "ok".to_string(),
                output_items: vec![TalosOutputItem::OutputText {
                    text: "ok".to_string(),
                }],
                stop_reason: StopReason::EndTurn,
                usage: TalosUsage::default(),
                warnings: Vec::new(),
                task_results: vec![crate::TaskResult {
                    task_id: "t1".to_string(),
                    kind: crate::TaskKind::General,
                    goal: "hello".to_string(),
                    status: crate::TaskStatus::Success,
                    output: "ok".to_string(),
                    output_items: vec![TalosOutputItem::OutputText {
                        text: "ok".to_string(),
                    }],
                    usage: TalosUsage::default(),
                    warnings: Vec::new(),
                    tool_traces: Vec::new(),
                    state_delta: HashMap::new(),
                    error: None,
                }],
                state_delta: HashMap::new(),
                continue_loop: false,
                requested_transfer: None,
            })
        }
    }

    #[tokio::test]
    async fn test_kernel_runtime_run_turn() {
        let runtime = KernelBuilder::new().build();
        let output = runtime
            .run_turn(
                Arc::new(MockKernelAgent),
                RunRequest {
                    session_id: "s1".to_string(),
                    message_id: "msg-1".to_string(),
                    invocation_id: "i1".to_string(),
                    agent_id: "a1".to_string(),
                    workspace_id: "w1".to_string(),
                    project_root: None,
                    input: "hello".to_string(),
                    selected_model_info: talos_types::ModelInfo {
                        id: "m1".to_string(),
                        name: "m1".to_string(),
                        provider: "p1".to_string(),
                        context_window: 0,
                        reasoning: false,
                        input: Vec::new(),
                        cost: Default::default(),
                        compat: Default::default(),
                    },
                    phase: TurnPhase::Execution,
                    planning_instruction: None,
                    transfer_to: None,
                    initial_state_bags: None,
                    session_environment: None,
                    kernel_config: None,
                    stream_tx: None,
                },
            )
            .await
            .expect("kernel runtime should run");
        assert_eq!(output.output_text, "ok");
        assert_eq!(output.task_results.len(), 1);
    }
}
