use crate::{
    Agent, AgentRunResult, ExecutionBranch, ExecutionEventHandler, FlowFactory, FlowProfile,
    InvocationContext, MemoryBackedSessionService, Planner, StopReason, TaskResult, TaskStatus,
};
use async_trait::async_trait;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use talos_config::AgentKernelConfig;
use talos_error::TalosResult;
use talos_llm_provider::LlmProvider;
use talos_memory::MemoryStore;
use talos_tools::{ToolDefinitionProvider, ToolExecutorPort};
use talos_types::talos::{TalosOutputItem, TalosUsage};
use uuid::Uuid;

pub struct LlmKernelAgent {
    id: String,
    name: String,
    description: String,
    llm_provider: Arc<dyn LlmProvider>,
    memory_store: Arc<MemoryStore>,
    kernel_config: AgentKernelConfig,
    tool_definition_provider: Arc<dyn ToolDefinitionProvider>,
    tool_executor_port: Arc<dyn ToolExecutorPort>,
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
        tool_definition_provider: Arc<dyn ToolDefinitionProvider>,
        tool_executor_port: Arc<dyn ToolExecutorPort>,
        planner: Arc<dyn Planner>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            description: description.into(),
            llm_provider,
            memory_store,
            kernel_config,
            tool_definition_provider,
            tool_executor_port,
            planner,
        }
    }

    fn branch_for_profile(profile: FlowProfile) -> ExecutionBranch {
        match profile {
            FlowProfile::DirectAnswer => ExecutionBranch::DirectAnswer,
            FlowProfile::ModelThinking => ExecutionBranch::ModelThinking,
            FlowProfile::ReactiveLoop => ExecutionBranch::ReactiveLoop,
        }
    }

    async fn persist_staged_session_events(
        ctx: &mut InvocationContext,
        persisted_event_count: usize,
    ) -> TalosResult<()> {
        if ctx.session.events.len() <= persisted_event_count {
            return Ok(());
        }

        let staged_events = ctx.session.events.split_off(persisted_event_count);
        let session_service = ctx.session_service()?;
        for event in staged_events {
            let _ = session_service
                .append_event(&mut ctx.session, event)
                .await?;
        }
        Ok(())
    }

    async fn execute_general_task(
        &self,
        ctx: &mut InvocationContext,
        mut event_handler: Option<&mut dyn ExecutionEventHandler>,
    ) -> TalosResult<TaskResult> {
        let task_id = Uuid::new_v4().to_string();
        let goal = ctx.input.clone();
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
            self.tool_definition_provider.clone(),
            self.tool_executor_port.clone(),
            self.memory_store.clone(),
            self.kernel_config.clone(),
            self.planner.clone(),
        );
        if ctx.session_service.is_none() {
            ctx.session_service = Some(Arc::new(MemoryBackedSessionService::new(
                self.memory_store.clone(),
            )));
        }
        let session_service = ctx.session_service()?;
        let plugin_manager = ctx.plugin_manager.clone();
        plugin_manager.before_flow(ctx, flow.settings()).await?;
        let persisted_event_count = session_service
            .load_session(
                &ctx.session_id,
                None,
                Some(Self::branch_for_profile(flow.profile())),
            )
            .await?
            .events
            .len();

        let output = match flow.run_events(ctx, &goal).await {
            Ok(flow_execution) => {
                if let Some(handler) = event_handler.as_deref_mut() {
                    for event in flow_execution.events.clone() {
                        let _ = handler.handle(ctx, event).await?;
                    }
                }
                output_items = flow_execution.summary.output_items;
                usage = flow_execution.summary.usage;
                warnings = flow_execution.summary.warnings;
                flow_execution.summary.output_text
            }
            Err(e) => {
                Self::persist_staged_session_events(ctx, persisted_event_count).await?;
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

        state_delta.insert(
            "last_task_kind".to_string(),
            json!(crate::TaskKind::General),
        );
        state_delta.insert("last_task_id".to_string(), json!(task_id));

        Ok(TaskResult {
            task_id,
            kind: crate::TaskKind::General,
            goal,
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

    async fn run(
        &self,
        ctx: &mut InvocationContext,
        event_handler: Option<&mut dyn ExecutionEventHandler>,
    ) -> TalosResult<AgentRunResult> {
        let result = self.execute_general_task(ctx, event_handler).await?;
        let mut merged_state_delta = HashMap::new();
        for (k, v) in &result.state_delta {
            merged_state_delta.insert(k.clone(), v.clone());
        }
        merged_state_delta.insert(
            "last_task_kind".to_string(),
            json!(crate::TaskKind::General),
        );
        let usage = result.usage.clone();
        let warnings = result.warnings.clone();
        let output_text = result.output.clone();
        let output_items = result.output_items.clone();
        let task_results = vec![result];

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
    use crate::{
        ContextStateBags, EventAuthor, ExecutionBranch, InvocationLimits, SessionEvent,
        SessionEventFilter, SessionEventKind, SessionEventPayload, SessionService, TurnPhase,
    };
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
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

    impl Planner for PhaseAssertingPlanner {
        fn build_planning_instruction(
            &self,
            ctx: &InvocationContext,
            _request: &crate::LlmRequestEnvelope,
        ) -> Option<String> {
            assert_eq!(ctx.phase, TurnPhase::Execution);
            Some("build a short plan first".to_string())
        }
    }

    struct EmptyPlanner;
    struct RecordingSessionService {
        recorded: Arc<Mutex<Vec<SessionEvent>>>,
    }
    struct ToolCallingFailureProvider {
        call_count: AtomicUsize,
    }

    impl Planner for EmptyPlanner {}

    impl RecordingSessionService {
        fn new() -> Self {
            Self {
                recorded: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    #[async_trait]
    impl SessionService for RecordingSessionService {
        async fn append_event(
            &self,
            session: &mut crate::InvocationSession,
            event: SessionEvent,
        ) -> TalosResult<SessionEvent> {
            session.events.push(event.clone());
            self.recorded.lock().unwrap().push(event.clone());
            Ok(event)
        }

        async fn load_session(
            &self,
            session_id: &str,
            _invocation_id: Option<&str>,
            _branch: Option<ExecutionBranch>,
        ) -> TalosResult<crate::InvocationSession> {
            Ok(crate::InvocationSession {
                session_id: session_id.to_string(),
                ..crate::InvocationSession::default()
            })
        }

        async fn list_events(
            &self,
            _session_id: &str,
            _filter: &SessionEventFilter,
        ) -> TalosResult<Vec<SessionEvent>> {
            Ok(self.recorded.lock().unwrap().clone())
        }
    }

    #[async_trait]
    impl LlmProvider for ToolCallingFailureProvider {
        fn name(&self) -> &str {
            "tool-calling-failure"
        }

        async fn chat(&self, _request: ChatRequest) -> TalosResult<ChatResponse> {
            let call_index = self.call_count.fetch_add(1, Ordering::SeqCst);
            if call_index == 0 {
                Ok(ChatResponse {
                    content: String::new(),
                    model: "tool-calling-failure".to_string(),
                    usage: Usage::default(),
                    finish_reason: Some("tool_calls".to_string()),
                    tool_calls: Some(vec![talos_llm_provider::FunctionCall::new(
                        "call-1",
                        "missing_tool",
                        serde_json::json!({"path":"/tmp/a"}),
                    )]),
                })
            } else {
                Err(talos_error::TalosError::InternalError(
                    "provider failed on follow-up round".to_string(),
                ))
            }
        }

        async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
            let (_tx, rx) = mpsc::channel::<TalosResult<ChatChunk>>(1);
            Ok(ChatStream::new(rx))
        }

        fn default_model(&self) -> &str {
            "tool-calling-failure"
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
            Arc::new(ToolRegistry::new()),
            planner,
        )
    }

    fn build_agent_with_provider_and_registry(
        planner: Arc<dyn Planner>,
        provider: Arc<dyn LlmProvider>,
        tool_registry: Arc<ToolRegistry>,
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
            tool_registry.clone(),
            tool_registry,
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
            plugin_manager: crate::default_plugin_manager(),
            session_service: None,
            stream_tx: None,
        }
    }

    #[tokio::test]
    async fn test_run_keeps_planner_instruction_outside_agent_phase() {
        let agent = build_agent(Arc::new(PhaseAssertingPlanner));
        let mut ctx = build_ctx();

        let result = agent.run(&mut ctx, None).await.expect("run should succeed");

        assert_eq!(ctx.phase, TurnPhase::Execution);
        assert_eq!(
            ctx.planning_instruction.as_deref(),
            Some("build a short plan first")
        );
        assert_eq!(result.task_results.len(), 1);
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

        let result = agent.run(&mut ctx, None).await.expect("run should succeed");

        assert_eq!(result.requested_transfer.as_deref(), Some("agent-b"));
    }

    #[tokio::test]
    async fn test_run_accumulates_usage_from_general_turn() {
        let agent =
            build_agent_with_provider(Arc::new(EmptyPlanner), Arc::new(UsageStreamProvider));
        let mut ctx = build_ctx();

        let result = agent.run(&mut ctx, None).await.expect("run should succeed");

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
            build_agent_with_provider(Arc::new(EmptyPlanner), Arc::new(UsageStreamProvider));
        let mut ctx = build_ctx();

        let result = agent.run(&mut ctx, None).await.expect("run should succeed");

        assert_eq!(result.task_results.len(), 1);
        assert_eq!(result.task_results[0].warnings, result.warnings);
        assert!(!ctx
            .state_bags
            .request_state
            .contains_key("last_prompt_diagnostics"));
    }

    #[tokio::test]
    async fn test_run_persists_staged_session_events_on_flow_error() {
        let agent = build_agent_with_provider_and_registry(
            Arc::new(EmptyPlanner),
            Arc::new(ToolCallingFailureProvider {
                call_count: AtomicUsize::new(0),
            }),
            Arc::new(ToolRegistry::new()),
        );
        let session_service = Arc::new(RecordingSessionService::new());
        let mut ctx = build_ctx();
        ctx.session_service = Some(session_service.clone());

        let result = agent
            .run(&mut ctx, None)
            .await
            .expect("run should return task result");

        assert_eq!(result.task_results.len(), 1);
        assert_eq!(result.task_results[0].status, TaskStatus::Failed);
        let recorded = session_service.recorded.lock().unwrap().clone();
        assert_eq!(recorded.len(), 3);
        assert!(recorded.iter().any(|event| {
            event.kind == SessionEventKind::UserMessage
                && matches!(
                    &event.payload,
                    SessionEventPayload::Message { message } if message.content == "hello"
                )
        }));
        assert!(recorded.iter().any(|event| {
            event.kind == SessionEventKind::ToolCall && event.author == EventAuthor::Agent
        }));
        assert!(recorded.iter().any(|event| {
            event.kind == SessionEventKind::ToolResult && event.author == EventAuthor::Tool
        }));
    }
}
