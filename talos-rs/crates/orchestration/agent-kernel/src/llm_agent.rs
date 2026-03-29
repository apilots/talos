use crate::{
    Agent, AgentRunResult, FlowFactory, InvocationContext, Planner, TaskResult, TaskSpec,
    TaskStatus, StopReason,
};
use async_trait::async_trait;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use talos_config::AgentKernelConfig;
use talos_error::TalosResult;
use talos_llm_provider::LlmProvider;
use talos_memory::MemoryStore;
use talos_tools::ToolRegistry;
use talos_types::talos::{TalosOutputItem, TalosUsage};

pub struct LlmKernelAgent {
    id: String,
    name: String,
    description: String,
    llm_provider: Arc<dyn LlmProvider>,
    memory_store: Arc<MemoryStore>,
    kernel_config: AgentKernelConfig,
    tool_registry: Arc<ToolRegistry>,
    planner: Arc<dyn Planner>,
}

impl LlmKernelAgent {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        description: impl Into<String>,
        llm_provider: Arc<dyn LlmProvider>,
        memory_store: Arc<MemoryStore>,
        kernel_config: AgentKernelConfig,
        tool_registry: Arc<ToolRegistry>,
        planner: Arc<dyn Planner>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            description: description.into(),
            llm_provider,
            memory_store,
            kernel_config,
            tool_registry,
            planner,
        }
    }

    async fn execute_planned_task(
        &self,
        ctx: &mut InvocationContext,
        task: &TaskSpec,
    ) -> TalosResult<TaskResult> {
        let tool_traces = Vec::new();
        let mut state_delta = HashMap::new();
        let mut error = None;
        let mut status = TaskStatus::Success;
        let mut output_items: Vec<TalosOutputItem> = Vec::new();
        let mut usage = TalosUsage::default();
        let mut warnings = Vec::new();

        let flow = FlowFactory::build(
            &self.name,
            &self.description,
            self.llm_provider.clone(),
            self.tool_registry.clone(),
            self.memory_store.clone(),
            self.kernel_config.clone(),
            self.planner.clone(),
        );

        let output = match flow.run(
            ctx,
            &task.goal,
        ).await {
            Ok(flow_result) => {
                output_items = flow_result.output_items;
                usage = flow_result.usage;
                warnings = flow_result.warnings;
                flow_result.output_text
            }
            Err(e) => {
                status = TaskStatus::Failed;
                error = Some(e.to_string());
                output_items.push(TalosOutputItem::Reasoning {
                    text: "Kernel summary: general task failed during kernel orchestration."
                        .to_string(),
                });
                format!("Task failed: {}", e)
            }
        };

        if !output_items
            .iter()
            .any(|item| matches!(item, TalosOutputItem::OutputText { .. }))
        {
            output_items.push(TalosOutputItem::OutputText {
                text: output.clone(),
            });
        }

        state_delta.insert("last_task_kind".to_string(), json!(task.kind));
        state_delta.insert("last_task_id".to_string(), json!(task.id));

        Ok(TaskResult {
            task_id: task.id.clone(),
            kind: task.kind.clone(),
            goal: task.goal.clone(),
            status,
            output,
            output_items,
            usage,
            warnings,
            tool_traces,
            state_delta,
            error,
        })
    }

    fn maybe_request_transfer(ctx: &mut InvocationContext) -> Option<String> {
        if ctx.transfer_consumed() {
            return None;
        }
        let transfer_to = ctx.transfer_to.clone()?;
        if transfer_to.trim().is_empty() {
            return None;
        }
        ctx.mark_transfer_consumed();
        Some(transfer_to)
    }
}

#[async_trait]
impl Agent for LlmKernelAgent {
    fn id(&self) -> &str {
        &self.id
    }

    async fn run(&self, ctx: &mut InvocationContext) -> TalosResult<AgentRunResult> {
        let plan = self.planner.plan(ctx).await?;
        let mut task_results = Vec::new();
        let mut merged_state_delta = HashMap::new();
        let mut usage = TalosUsage::default();

        for task in plan
            .tasks
            .iter()
            .take(self.kernel_config.max_task_per_turn.max(1))
        {
            let result = self.execute_planned_task(ctx, task).await?;
            for (k, v) in &result.state_delta {
                merged_state_delta.insert(k.clone(), v.clone());
            }
            usage.input_tokens = usage.input_tokens.saturating_add(result.usage.input_tokens);
            usage.output_tokens = usage
                .output_tokens
                .saturating_add(result.usage.output_tokens);
            usage.total_tokens = usage.total_tokens.saturating_add(result.usage.total_tokens);
            task_results.push(result);
            merged_state_delta.insert("last_task_kind".to_string(), json!(task.kind));
        }

        let output_text = task_results
            .last()
            .map(|result| result.output.clone())
            .unwrap_or_else(|| "No task produced output".to_string());
        let output_items = task_results
            .iter()
            .flat_map(|result| result.output_items.clone())
            .collect::<Vec<_>>();
        let warnings = task_results
            .iter()
            .flat_map(|result| result.warnings.clone())
            .collect::<Vec<_>>();

        Ok(AgentRunResult {
            output_text,
            output_items,
            stop_reason: StopReason::EndTurn,
            usage,
            warnings,
            task_results,
            state_delta: merged_state_delta,
            continue_loop: false,
            requested_transfer: Self::maybe_request_transfer(ctx),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ContextStateBags, InvocationLimits, TaskPlan, TurnPhase};
    use async_trait::async_trait;
    use std::collections::HashMap;
    use talos_error::TalosResult;
    use talos_llm_provider::{
        ChatChunk, ChatRequest, ChatResponse, ChatStream, LlmProvider, ToolStream, ToolStreamEvent,
        Usage,
    };
    use talos_tools::ToolRegistry;
    use tokio::sync::mpsc;

    struct NoopLlmProvider;

    #[async_trait]
    impl LlmProvider for NoopLlmProvider {
        fn name(&self) -> &str {
            "noop"
        }

        async fn chat(&self, _request: ChatRequest) -> TalosResult<ChatResponse> {
            Ok(ChatResponse {
                content: String::new(),
                model: "noop".to_string(),
                usage: Usage::default(),
                finish_reason: Some("stop".to_string()),
                tool_calls: None,
            })
        }

        async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
            let (_tx, rx) = mpsc::channel::<TalosResult<ChatChunk>>(1);
            Ok(ChatStream::new(rx))
        }

        fn default_model(&self) -> &str {
            "noop"
        }
    }

    struct UsageStreamProvider;

    #[async_trait]
    impl LlmProvider for UsageStreamProvider {
        fn name(&self) -> &str {
            "usage-stream"
        }

        async fn chat(&self, _request: ChatRequest) -> TalosResult<ChatResponse> {
            Ok(ChatResponse {
                content: "ok".to_string(),
                model: "usage-stream".to_string(),
                usage: Usage {
                    prompt_tokens: 11,
                    completion_tokens: 7,
                    total_tokens: 18,
                },
                finish_reason: Some("stop".to_string()),
                tool_calls: None,
            })
        }

        async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
            let (_tx, rx) = mpsc::channel::<TalosResult<ChatChunk>>(1);
            Ok(ChatStream::new(rx))
        }

        async fn stream_with_tools(&self, _request: ChatRequest) -> TalosResult<ToolStream> {
            let (tx, rx) = mpsc::channel::<TalosResult<ToolStreamEvent>>(4);
            tokio::spawn(async move {
                let _ = tx
                    .send(Ok(ToolStreamEvent::TextDelta {
                        delta: "ok".to_string(),
                    }))
                    .await;
                let _ = tx
                    .send(Ok(ToolStreamEvent::Done {
                        finish_reason: Some("stop".to_string()),
                        usage: Some(Usage {
                            prompt_tokens: 11,
                            completion_tokens: 7,
                            total_tokens: 18,
                        }),
                    }))
                    .await;
            });
            Ok(ToolStream::new(rx))
        }

        fn default_model(&self) -> &str {
            "usage-stream"
        }
    }

    struct PhaseAssertingPlanner;

    #[async_trait]
    impl Planner for PhaseAssertingPlanner {
        fn build_planning_instruction(&self, _ctx: &InvocationContext) -> Option<String> {
            Some("build a short plan first".to_string())
        }

        async fn plan(&self, ctx: &InvocationContext) -> TalosResult<TaskPlan> {
            assert_eq!(ctx.phase, TurnPhase::Execution);
            assert!(ctx.planning_instruction.is_none());
            Ok(TaskPlan::default())
        }
    }

    struct EmptyPlanner;

    #[async_trait]
    impl Planner for EmptyPlanner {
        async fn plan(&self, _ctx: &InvocationContext) -> TalosResult<TaskPlan> {
            Ok(TaskPlan::default())
        }
    }

    struct SingleTaskPlanner;

    #[async_trait]
    impl Planner for SingleTaskPlanner {
        async fn plan(&self, _ctx: &InvocationContext) -> TalosResult<TaskPlan> {
            Ok(TaskPlan {
                tasks: vec![TaskSpec {
                    id: "task-1".to_string(),
                    kind: crate::TaskKind::General,
                    goal: "reply once".to_string(),
                    input: json!({}),
                    constraints: crate::TaskConstraints::default(),
                    tool_candidates: Vec::new(),
                    expected_output: None,
                }],
            })
        }
    }

    fn build_agent(planner: Arc<dyn Planner>) -> LlmKernelAgent {
        LlmKernelAgent::new(
            "agent-1",
            "Test Agent",
            "Test kernel agent",
            Arc::new(NoopLlmProvider),
            Arc::new(MemoryStore::new(
                std::env::temp_dir().join("talos-llm-agent-tests"),
            )),
            AgentKernelConfig::default(),
            Arc::new(ToolRegistry::new()),
            planner,
        )
    }

    fn build_agent_with_provider(
        planner: Arc<dyn Planner>,
        provider: Arc<dyn LlmProvider>,
    ) -> LlmKernelAgent {
        LlmKernelAgent::new(
            "agent-1",
            "Test Agent",
            "Test kernel agent",
            provider,
            Arc::new(MemoryStore::new(
                std::env::temp_dir().join("talos-llm-agent-tests"),
            )),
            AgentKernelConfig::default(),
            Arc::new(ToolRegistry::new()),
            planner,
        )
    }

    fn build_ctx() -> InvocationContext {
        InvocationContext {
            session_id: "s1".to_string(),
            invocation_id: "i1".to_string(),
            agent_id: "a1".to_string(),
            workspace_id: "w1".to_string(),
            input: "hello".to_string(),
            model: "m1".to_string(),
            phase: TurnPhase::Execution,
            planning_instruction: None,
            transfer_to: None,
            execution_state: crate::InvocationExecutionState::default(),
            session: crate::InvocationSession {
                session_id: "s1".to_string(),
                ..crate::InvocationSession::default()
            },
            state: HashMap::new(),
            state_bags: ContextStateBags::default(),
            limits: InvocationLimits::default(),
            events: Vec::new(),
            stream_tx: None,
        }
    }

    #[tokio::test]
    async fn test_run_keeps_planner_instruction_outside_agent_phase() {
        let agent = build_agent(Arc::new(PhaseAssertingPlanner));
        let mut ctx = build_ctx();

        let result = agent.run(&mut ctx).await.expect("run should succeed");

        assert_eq!(ctx.phase, TurnPhase::Execution);
        assert!(ctx.planning_instruction.is_none());
        assert_eq!(result.output_text, "No task produced output");
    }

    #[test]
    fn test_maybe_request_transfer_consumes_typed_transfer_once() {
        let mut ctx = build_ctx();
        ctx.transfer_to = Some("agent-b".to_string());

        assert_eq!(
            LlmKernelAgent::maybe_request_transfer(&mut ctx),
            Some("agent-b".to_string())
        );
        assert!(LlmKernelAgent::maybe_request_transfer(&mut ctx).is_none());
    }

    #[tokio::test]
    async fn test_run_returns_typed_transfer_request() {
        let agent = build_agent(Arc::new(EmptyPlanner));
        let mut ctx = build_ctx();
        ctx.transfer_to = Some("agent-b".to_string());

        let result = agent.run(&mut ctx).await.expect("run should succeed");

        assert_eq!(result.requested_transfer.as_deref(), Some("agent-b"));
    }

    #[tokio::test]
    async fn test_run_accumulates_usage_from_general_turn() {
        let agent =
            build_agent_with_provider(Arc::new(SingleTaskPlanner), Arc::new(UsageStreamProvider));
        let mut ctx = build_ctx();

        let result = agent.run(&mut ctx).await.expect("run should succeed");

        assert!(result.usage.total_tokens > 0);
        assert_eq!(
            result.usage.total_tokens,
            result.usage.input_tokens + result.usage.output_tokens
        );
        assert_eq!(result.task_results.len(), 1);
        assert_eq!(
            result.task_results[0].usage.total_tokens,
            result.usage.total_tokens
        );
    }

    #[tokio::test]
    async fn test_run_collects_prompt_warnings() {
        let agent =
            build_agent_with_provider(Arc::new(SingleTaskPlanner), Arc::new(UsageStreamProvider));
        let mut ctx = build_ctx();

        let result = agent.run(&mut ctx).await.expect("run should succeed");

        assert_eq!(result.task_results.len(), 1);
        assert_eq!(result.task_results[0].warnings, result.warnings);
        assert!(
            !ctx.state_bags
                .request_state
                .contains_key("last_prompt_diagnostics")
        );
    }
}
