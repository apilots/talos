use crate::{
    Agent, ExecutionEvent, ExecutionEventHandler, FinalResponseEventData, IterationEventData,
    KernelEvent, KernelEventData, KernelEventLabel, KernelEventType, PluginRegistry, RunRequest,
    RunResponse, Runner, SessionEvent, TaskCompletedEventData, TransferRequestedEventData,
    UserInputEventData,
};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use talos_error::TalosResult;

pub struct DefaultRunner {
    plugins: PluginRegistry,
}

struct RunnerEventHandler;

#[async_trait]
impl ExecutionEventHandler for RunnerEventHandler {
    async fn handle(
        &mut self,
        ctx: &mut crate::InvocationContext,
        event: ExecutionEvent,
    ) -> TalosResult<Option<ExecutionEvent>> {
        let mut event = event;
        let plugin_manager = ctx.plugin_manager.clone();
        if !plugin_manager.on_event(ctx, &mut event).await? {
            return Ok(None);
        }
        if !event.partial {
            let session_service = ctx.session_service()?;
            let session_event =
                SessionEvent::from_execution_event(ctx.session_id.clone(), event.clone());
            let _ = session_service
                .append_event(&mut ctx.session, session_event)
                .await?;
        }
        Ok(Some(event))
    }
}

impl DefaultRunner {
    pub fn new() -> Self {
        Self {
            plugins: PluginRegistry::new(),
        }
    }

    pub fn with_plugins(mut self, plugins: PluginRegistry) -> Self {
        self.plugins = plugins;
        self
    }
}

#[async_trait]
impl Runner for DefaultRunner {
    async fn run_turn(
        &self,
        agent: Arc<dyn Agent>,
        request: RunRequest,
    ) -> TalosResult<RunResponse> {
        let crate::RunRequest {
            session_id,
            invocation_id,
            agent_id,
            workspace_id,
            project_root,
            input,
            selected_model_info,
            phase,
            planning_instruction,
            transfer_to,
            initial_state_bags,
            session_environment,
            kernel_config,
            stream_tx,
        } = request;

        let config = kernel_config
            .as_ref()
            .map(|c| c.clone())
            .unwrap_or_default();
        let ctx_session_id = session_id.clone();
        let user_input_session_id = session_id.clone();
        let user_input_agent_id = agent_id.clone();

        let mut ctx = crate::InvocationContext {
            session_id,
            invocation_id,
            agent_id,
            workspace_id,
            project_root,
            input,
            selected_model_info,
            phase,
            planning_instruction,
            transfer_to,
            execution_state: crate::InvocationExecutionState::default(),
            session: crate::InvocationSession {
                session_id: ctx_session_id,
                ..crate::InvocationSession::default()
            },
            session_environment,
            state: HashMap::new(),
            state_bags: initial_state_bags.unwrap_or_default(),
            limits: crate::InvocationLimits::from(&config),
            events: vec![KernelEvent::new(
                KernelEventType::UserInput,
                KernelEventLabel::UserInputReceived,
                KernelEventData::UserInput(UserInputEventData {
                    session_id: user_input_session_id,
                    agent_id: user_input_agent_id,
                }),
            )],
            plugin_manager: Arc::new(self.plugins.clone()),
            session_service: None,
            stream_tx,
        };

        let mut iterations = 0_u32;
        let (
            last_output,
            last_output_items,
            last_stop_reason,
            last_usage,
            last_warnings,
            last_task_results,
            transfer_to,
        ) = loop {
            iterations += 1;
            let plugin_manager = ctx.plugin_manager.clone();
            plugin_manager.before_agent_run(&mut ctx).await?;
            plugin_manager.before_run(&mut ctx).await?;
            ctx.events.push(KernelEvent::new(
                KernelEventType::PlanCreated,
                KernelEventLabel::Iteration,
                KernelEventData::Iteration(IterationEventData {
                    iteration: iterations,
                }),
            ));

            let mut event_handler = RunnerEventHandler;
            let mut run_result = agent.run(&mut ctx, Some(&mut event_handler)).await?;
            for task_result in &run_result.task_results {
                let plugin_manager = ctx.plugin_manager.clone();
                plugin_manager.after_flow(&mut ctx, task_result).await?;
                ctx.events.push(KernelEvent::new(
                    KernelEventType::TaskCompleted,
                    KernelEventLabel::TaskCompleted,
                    KernelEventData::TaskCompleted(TaskCompletedEventData {
                        task_id: task_result.task_id.clone(),
                        status: task_result.status.clone(),
                    }),
                ));
            }
            let plugin_manager = ctx.plugin_manager.clone();
            plugin_manager
                .after_agent_run(&mut ctx, &mut run_result)
                .await?;
            let plugin_manager = ctx.plugin_manager.clone();
            plugin_manager.after_run(&mut ctx).await?;

            for (k, v) in &run_result.state_delta {
                ctx.state.insert(k.clone(), v.clone());
                ctx.set_session_state(k.clone(), v.clone());
            }

            let output = run_result.output_text;
            let output_items = run_result.output_items;
            let stop_reason = run_result.stop_reason;
            let usage = run_result.usage;
            let warnings = run_result.warnings;
            let task_results = run_result.task_results;
            let requested_transfer = run_result.requested_transfer;
            let should_continue = run_result.continue_loop && iterations < ctx.limits.max_iters;

            if requested_transfer.is_some() {
                ctx.events.push(KernelEvent::new(
                    KernelEventType::Transfer,
                    KernelEventLabel::TransferRequested,
                    KernelEventData::TransferRequested(TransferRequestedEventData {
                        target_agent: requested_transfer.clone(),
                    }),
                ));
                break (
                    output,
                    output_items,
                    stop_reason,
                    usage,
                    warnings,
                    task_results,
                    requested_transfer,
                );
            }

            if !should_continue {
                break (
                    output,
                    output_items,
                    stop_reason,
                    usage,
                    warnings,
                    task_results,
                    requested_transfer,
                );
            }
        };

        ctx.events.push(KernelEvent::new(
            KernelEventType::FinalResponse,
            KernelEventLabel::FinalResponse,
            KernelEventData::FinalResponse(FinalResponseEventData {
                iterations,
                stop_reason: last_stop_reason.clone(),
            }),
        ));

        Ok(RunResponse {
            output_text: last_output,
            output_items: last_output_items,
            stop_reason: last_stop_reason,
            usage: last_usage,
            warnings: last_warnings,
            model: ctx.selected_model_id().to_string(),
            task_results: last_task_results,
            transfer_to,
            iterations,
            events: ctx.events,
        })
    }
}

impl Default for DefaultRunner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TurnPhase;
    use async_trait::async_trait;
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use talos_types::talos::{TalosOutputItem, TalosUsage};

    struct MockKernelAgent;
    struct RecordingSessionService {
        recorded: Arc<Mutex<Vec<SessionEvent>>>,
    }

    struct RedactingPlugin;
    struct DroppingPlugin;

    impl RecordingSessionService {
        fn new() -> Self {
            Self {
                recorded: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    #[async_trait]
    impl crate::SessionService for RecordingSessionService {
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
            _branch: Option<crate::ExecutionBranch>,
        ) -> TalosResult<crate::InvocationSession> {
            Ok(crate::InvocationSession {
                session_id: session_id.to_string(),
                ..crate::InvocationSession::default()
            })
        }

        async fn list_events(
            &self,
            _session_id: &str,
            _filter: &crate::SessionEventFilter,
        ) -> TalosResult<Vec<SessionEvent>> {
            Ok(self.recorded.lock().unwrap().clone())
        }
    }

    #[async_trait]
    impl crate::Plugin for RedactingPlugin {
        fn name(&self) -> &str {
            "redacting"
        }

        async fn on_event(
            &self,
            _ctx: &mut crate::InvocationContext,
            event: &mut ExecutionEvent,
        ) -> TalosResult<bool> {
            if let crate::EventPayload::Text { text } = &mut event.payload {
                *text = "redacted".to_string();
            }
            Ok(true)
        }
    }

    #[async_trait]
    impl crate::Plugin for DroppingPlugin {
        fn name(&self) -> &str {
            "dropping"
        }

        async fn on_event(
            &self,
            _ctx: &mut crate::InvocationContext,
            _event: &mut ExecutionEvent,
        ) -> TalosResult<bool> {
            Ok(false)
        }
    }

    fn mock_ctx(
        session_service: Arc<dyn crate::SessionService>,
        plugin_manager: Arc<crate::InvocationPluginManager>,
    ) -> crate::InvocationContext {
        crate::InvocationContext {
            session_id: "s1".to_string(),
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
            execution_state: crate::InvocationExecutionState::default(),
            session: crate::InvocationSession {
                session_id: "s1".to_string(),
                ..crate::InvocationSession::default()
            },
            session_environment: None,
            state: HashMap::new(),
            state_bags: crate::ContextStateBags::default(),
            limits: crate::InvocationLimits::default(),
            events: Vec::new(),
            plugin_manager,
            session_service: Some(session_service),
            stream_tx: None,
        }
    }

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
                stop_reason: crate::StopReason::EndTurn,
                usage: TalosUsage::default(),
                warnings: Vec::new(),
                task_results: vec![crate::TaskResult {
                    task_id: "t1".to_string(),
                    kind: crate::TaskKind::General,
                    goal: "g".to_string(),
                    status: crate::TaskStatus::Success,
                    output: "done".to_string(),
                    output_items: vec![],
                    usage: TalosUsage::default(),
                    warnings: Vec::new(),
                    tool_traces: vec![],
                    state_delta: HashMap::new(),
                    error: None,
                }],
                state_delta: HashMap::from([(String::from("k"), json!("v"))]),
                continue_loop: false,
                requested_transfer: None,
            })
        }
    }

    #[tokio::test]
    async fn test_default_runner_run_turn() {
        let runner = DefaultRunner::new();
        let response = runner
            .run_turn(
                Arc::new(MockKernelAgent),
                RunRequest {
                    session_id: "s1".to_string(),
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
                    kernel_config: Some(talos_config::AgentKernelConfig::default()),
                    stream_tx: None,
                },
            )
            .await
            .expect("runner should succeed");
        assert_eq!(response.output_text, "ok");
        assert_eq!(response.iterations, 1);
        assert!(!response.events.is_empty());
    }

    #[tokio::test]
    async fn test_runner_event_handler_applies_plugin_redaction_before_persist() {
        let session_service = Arc::new(RecordingSessionService::new());
        let mut plugin_manager = crate::InvocationPluginManager::new();
        plugin_manager.register(Arc::new(RedactingPlugin));
        let plugin_manager = Arc::new(plugin_manager);
        let mut ctx = mock_ctx(session_service.clone(), plugin_manager);
        let mut handler = RunnerEventHandler;

        let event =
            ExecutionEvent::final_response(&ctx, crate::ExecutionBranch::ReactiveLoop, "secret");
        let handled = handler.handle(&mut ctx, event).await.unwrap().unwrap();

        match handled.payload {
            crate::EventPayload::Text { text } => assert_eq!(text, "redacted"),
            other => panic!("expected text payload, got {:?}", other),
        }
        assert_eq!(ctx.session.events.len(), 1);
        let persisted = &ctx.session.events[0];
        assert_eq!(
            persisted
                .payload
                .message()
                .map(|message| message.content.as_str()),
            Some("redacted")
        );
        assert_eq!(session_service.recorded.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_runner_event_handler_does_not_persist_dropped_event() {
        let session_service = Arc::new(RecordingSessionService::new());
        let mut plugin_manager = crate::InvocationPluginManager::new();
        plugin_manager.register(Arc::new(DroppingPlugin));
        let plugin_manager = Arc::new(plugin_manager);
        let mut ctx = mock_ctx(session_service.clone(), plugin_manager);
        let mut handler = RunnerEventHandler;

        let event =
            ExecutionEvent::final_response(&ctx, crate::ExecutionBranch::ReactiveLoop, "secret");
        let handled = handler.handle(&mut ctx, event).await.unwrap();

        assert!(handled.is_none());
        assert!(ctx.session.events.is_empty());
        assert!(session_service.recorded.lock().unwrap().is_empty());
    }
}
