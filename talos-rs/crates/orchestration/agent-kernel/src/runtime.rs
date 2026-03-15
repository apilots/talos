use crate::{
    Agent, ContextStateBags, DefaultRunner, KernelStreamEvent, RunRequest, Runner,
    SessionHandle, TaskResult,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use talos_config::AgentKernelConfig;
use talos_error::TalosResult;
use talos_types::canonical::{CanonicalOutputItem, CanonicalUsage};
use tokio::sync::mpsc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KernelInput {
    pub session_id: String,
    pub invocation_id: Option<String>,
    pub agent_id: String,
    pub workspace_id: String,
    pub input: String,
    pub model: String,
    pub metadata: Option<Value>,
    pub session_handle: Option<SessionHandle>,
    pub initial_state_bags: Option<ContextStateBags>,
    pub kernel_config: Option<AgentKernelConfig>,
    #[serde(skip)]
    pub stream_tx: Option<mpsc::Sender<KernelStreamEvent>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KernelOutput {
    pub output_text: String,
    pub output_items: Vec<CanonicalOutputItem>,
    pub stop_reason: String,
    pub usage: CanonicalUsage,
    pub model: String,
    pub task_results: Vec<TaskResult>,
    pub transfer_to: Option<String>,
    pub iterations: u32,
    pub metadata: HashMap<String, Value>,
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
        input: KernelInput,
    ) -> TalosResult<KernelOutput> {
        let request = RunRequest {
            session_id: input.session_id,
            invocation_id: input
                .invocation_id
                .unwrap_or_else(|| format!("inv-{}", chrono::Utc::now().timestamp_millis())),
            agent_id: input.agent_id,
            workspace_id: input.workspace_id,
            input: input.input,
            model: input.model,
            metadata: input.metadata,
            session_handle: input.session_handle,
            initial_state_bags: input.initial_state_bags,
            kernel_config: input
                .kernel_config
                .map(|c| c.clone()),
            stream_tx: input.stream_tx,
        };

        let response = self.runner.run_turn(agent, request).await?;
        Ok(KernelOutput {
            output_text: response.output_text,
            output_items: response.output_items,
            stop_reason: response.stop_reason,
            usage: response.usage,
            model: response.model,
            task_results: response.task_results,
            transfer_to: response.transfer_to,
            iterations: response.iterations,
            metadata: response.metadata,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    struct MockKernelAgent;

    #[async_trait]
    impl Agent for MockKernelAgent {
        fn id(&self) -> &str {
            "mock"
        }

        async fn run(
            &self,
            _ctx: &mut crate::InvocationContext,
        ) -> TalosResult<crate::AgentRunResult> {
            Ok(crate::AgentRunResult {
                output_text: "ok".to_string(),
                output_items: vec![talos_types::canonical::CanonicalOutputItem::OutputText {
                    text: "ok".to_string(),
                }],
                stop_reason: "end_turn".to_string(),
                usage: talos_types::canonical::CanonicalUsage::default(),
                task_results: vec![crate::TaskResult {
                    task_id: "t1".to_string(),
                    kind: crate::TaskKind::General,
                    goal: "hello".to_string(),
                    status: crate::TaskStatus::Success,
                    output: "ok".to_string(),
                    output_items: vec![talos_types::canonical::CanonicalOutputItem::OutputText {
                        text: "ok".to_string(),
                    }],
                    tool_traces: Vec::new(),
                    state_delta: HashMap::new(),
                    error: None,
                }],
                state_delta: HashMap::new(),
                continue_loop: false,
                requested_transfer: None,
                metadata: HashMap::new(),
            })
        }
    }

    #[tokio::test]
    async fn test_kernel_runtime_run_turn() {
        let runtime = KernelBuilder::new().build();
        let output = runtime
            .run_turn(
                Arc::new(MockKernelAgent),
                KernelInput {
                    session_id: "s1".to_string(),
                    invocation_id: Some("i1".to_string()),
                    agent_id: "a1".to_string(),
                    workspace_id: "w1".to_string(),
                    input: "hello".to_string(),
                    model: "m1".to_string(),
                    metadata: None,
                    session_handle: None,
                    initial_state_bags: None,
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
