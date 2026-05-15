use crate::{
    Agent, AgentRunResult, ExecutionEvent, ExecutionEventHandler, InvocationContext, StopReason,
    TaskResult,
};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use talos_error::TalosResult;
use talos_types::talos::TalosUsage;
use tokio::task::JoinSet;

fn merge_usage(mut acc: TalosUsage, usage: &TalosUsage) -> TalosUsage {
    acc.accumulate(usage);
    acc
}

fn merge_state_delta(
    target: &mut HashMap<String, serde_json::Value>,
    source: &HashMap<String, serde_json::Value>,
) {
    for (k, v) in source {
        target.insert(k.clone(), v.clone());
    }
}

#[derive(Default)]
struct BufferedExecutionEventHandler {
    events: Vec<ExecutionEvent>,
}

#[async_trait]
impl ExecutionEventHandler for BufferedExecutionEventHandler {
    async fn handle(
        &mut self,
        _ctx: &mut InvocationContext,
        event: ExecutionEvent,
    ) -> TalosResult<Option<ExecutionEvent>> {
        self.events.push(event.clone());
        Ok(Some(event))
    }
}

pub struct SequentialKernelAgent {
    id: String,
    children: Vec<Arc<dyn Agent>>,
}

impl SequentialKernelAgent {
    pub fn new(id: impl Into<String>, children: Vec<Arc<dyn Agent>>) -> Self {
        Self {
            id: id.into(),
            children,
        }
    }
}

#[async_trait]
impl Agent for SequentialKernelAgent {
    fn id(&self) -> &str {
        &self.id
    }

    async fn run(
        &self,
        ctx: &mut InvocationContext,
        mut event_handler: Option<&mut dyn ExecutionEventHandler>,
    ) -> TalosResult<AgentRunResult> {
        let mut output_text = String::new();
        let mut output_items = Vec::new();
        let mut stop_reason = StopReason::EndTurn;
        let mut usage = TalosUsage::default();
        let mut task_results: Vec<TaskResult> = Vec::new();
        let mut state_delta = HashMap::new();
        let mut warnings = Vec::new();
        let mut requested_transfer = None;
        let mut continue_loop = false;

        for child in &self.children {
            let result = if let Some(handler) = event_handler.as_mut() {
                child.run(ctx, Some(*handler)).await?
            } else {
                child.run(ctx, None).await?
            };
            output_text = result.output_text.clone();
            output_items.extend(result.output_items.clone());
            stop_reason = result.stop_reason.clone();
            usage = merge_usage(usage, &result.usage);
            warnings.extend(result.warnings.clone());
            task_results.extend(result.task_results.clone());
            merge_state_delta(&mut state_delta, &result.state_delta);
            continue_loop = continue_loop || result.continue_loop;

            if let Some(transfer) = result.requested_transfer {
                requested_transfer = Some(transfer);
                break;
            }
        }

        Ok(AgentRunResult {
            output_text,
            output_items,
            stop_reason,
            usage,
            warnings,
            task_results,
            state_delta,
            continue_loop,
            requested_transfer,
        })
    }
}

pub struct ParallelKernelAgent {
    id: String,
    children: Vec<Arc<dyn Agent>>,
}

impl ParallelKernelAgent {
    pub fn new(id: impl Into<String>, children: Vec<Arc<dyn Agent>>) -> Self {
        Self {
            id: id.into(),
            children,
        }
    }
}

#[async_trait]
impl Agent for ParallelKernelAgent {
    fn id(&self) -> &str {
        &self.id
    }

    async fn run(
        &self,
        ctx: &mut InvocationContext,
        mut event_handler: Option<&mut dyn ExecutionEventHandler>,
    ) -> TalosResult<AgentRunResult> {
        let mut set = JoinSet::new();
        for (index, child) in self.children.iter().enumerate() {
            let child = child.clone();
            let mut child_ctx = ctx.clone();
            set.spawn(async move {
                let mut buffered_handler = BufferedExecutionEventHandler::default();
                let result = child
                    .run(&mut child_ctx, Some(&mut buffered_handler))
                    .await?;
                Ok::<_, talos_error::TalosError>((index, result, buffered_handler.events))
            });
        }

        let mut ordered: Vec<(usize, AgentRunResult, Vec<ExecutionEvent>)> = Vec::new();
        while let Some(joined) = set.join_next().await {
            let pair = joined.map_err(|e| {
                talos_error::TalosError::InternalError(format!(
                    "parallel agent task join failed: {}",
                    e
                ))
            })??;
            ordered.push(pair);
        }
        ordered.sort_by_key(|(index, _, _)| *index);

        let mut output_text = String::new();
        let mut output_items = Vec::new();
        let mut stop_reason = StopReason::EndTurn;
        let mut usage = TalosUsage::default();
        let mut task_results: Vec<TaskResult> = Vec::new();
        let mut state_delta = HashMap::new();
        let mut warnings = Vec::new();
        let mut requested_transfer = None;
        let mut continue_loop = false;

        for (_, result, buffered_events) in ordered {
            if let Some(handler) = event_handler.as_deref_mut() {
                for event in buffered_events {
                    let _ = handler.handle(ctx, event).await?;
                }
            }
            output_text = result.output_text.clone();
            output_items.extend(result.output_items);
            stop_reason = result.stop_reason.clone();
            usage = merge_usage(usage, &result.usage);
            warnings.extend(result.warnings);
            task_results.extend(result.task_results);
            merge_state_delta(&mut state_delta, &result.state_delta);
            continue_loop = continue_loop || result.continue_loop;
            if requested_transfer.is_none() {
                requested_transfer = result.requested_transfer;
            }
        }

        Ok(AgentRunResult {
            output_text,
            output_items,
            stop_reason,
            usage,
            warnings,
            task_results,
            state_delta,
            continue_loop,
            requested_transfer,
        })
    }
}

pub struct LoopKernelAgent {
    id: String,
    child: Arc<dyn Agent>,
    max_iterations: u32,
}

impl LoopKernelAgent {
    pub fn new(id: impl Into<String>, child: Arc<dyn Agent>, max_iterations: u32) -> Self {
        Self {
            id: id.into(),
            child,
            max_iterations: max_iterations.max(1),
        }
    }
}

#[async_trait]
impl Agent for LoopKernelAgent {
    fn id(&self) -> &str {
        &self.id
    }

    async fn run(
        &self,
        ctx: &mut InvocationContext,
        mut event_handler: Option<&mut dyn ExecutionEventHandler>,
    ) -> TalosResult<AgentRunResult> {
        let mut merged = AgentRunResult {
            output_text: String::new(),
            output_items: Vec::new(),
            stop_reason: StopReason::EndTurn,
            usage: TalosUsage::default(),
            warnings: Vec::new(),
            task_results: Vec::new(),
            state_delta: HashMap::new(),
            continue_loop: false,
            requested_transfer: None,
        };

        for _ in 0..self.max_iterations {
            let result = if let Some(handler) = event_handler.as_mut() {
                self.child.run(ctx, Some(*handler)).await?
            } else {
                self.child.run(ctx, None).await?
            };
            merged.output_text = result.output_text.clone();
            merged.output_items.extend(result.output_items.clone());
            merged.stop_reason = result.stop_reason.clone();
            merged.usage = merge_usage(merged.usage, &result.usage);
            merged.warnings.extend(result.warnings.clone());
            merged.task_results.extend(result.task_results.clone());
            merge_state_delta(&mut merged.state_delta, &result.state_delta);
            merged.continue_loop = result.continue_loop;

            if let Some(transfer) = result.requested_transfer {
                merged.requested_transfer = Some(transfer);
                break;
            }
            if !result.continue_loop {
                break;
            }
        }

        merged.continue_loop = false;
        Ok(merged)
    }
}

pub struct RoutingKernelAgent {
    id: String,
    start_agent: String,
    max_hops: u32,
    nodes: HashMap<String, Arc<dyn Agent>>,
}

impl RoutingKernelAgent {
    pub fn new(id: impl Into<String>, start_agent: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            start_agent: start_agent.into(),
            max_hops: 8,
            nodes: HashMap::new(),
        }
    }

    pub fn with_max_hops(mut self, max_hops: u32) -> Self {
        self.max_hops = max_hops.max(1);
        self
    }

    pub fn register_node(&mut self, agent_id: impl Into<String>, agent: Arc<dyn Agent>) {
        self.nodes.insert(agent_id.into(), agent);
    }
}

#[async_trait]
impl Agent for RoutingKernelAgent {
    fn id(&self) -> &str {
        &self.id
    }

    async fn run(
        &self,
        ctx: &mut InvocationContext,
        mut event_handler: Option<&mut dyn ExecutionEventHandler>,
    ) -> TalosResult<AgentRunResult> {
        let mut current = self.start_agent.clone();
        let mut hops = 0_u32;

        let mut merged = AgentRunResult {
            output_text: String::new(),
            output_items: Vec::new(),
            stop_reason: StopReason::EndTurn,
            usage: TalosUsage::default(),
            warnings: Vec::new(),
            task_results: Vec::new(),
            state_delta: HashMap::new(),
            continue_loop: false,
            requested_transfer: None,
        };

        while hops < self.max_hops {
            hops += 1;
            let Some(agent) = self.nodes.get(&current) else {
                merged.requested_transfer = Some(current);
                merged.stop_reason = StopReason::TransferUnresolved;
                break;
            };

            let result = if let Some(handler) = event_handler.as_mut() {
                agent.run(ctx, Some(*handler)).await?
            } else {
                agent.run(ctx, None).await?
            };
            merged.output_text = result.output_text.clone();
            merged.output_items.extend(result.output_items.clone());
            merged.stop_reason = result.stop_reason.clone();
            merged.usage = merge_usage(merged.usage, &result.usage);
            merged.warnings.extend(result.warnings.clone());
            merged.task_results.extend(result.task_results.clone());
            merge_state_delta(&mut merged.state_delta, &result.state_delta);
            if let Some(next) = result.requested_transfer {
                if self.nodes.contains_key(&next) {
                    current = next;
                    continue;
                }
                merged.requested_transfer = Some(next);
                break;
            }

            merged.requested_transfer = None;
            break;
        }

        Ok(merged)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        EventAuthor, ExecutionBranch, ExecutionEvent, ExecutionEventKind, TaskKind, TaskStatus,
    };
    use talos_types::talos::TalosOutputItem;

    #[derive(Clone)]
    struct StaticAgent {
        id: String,
        output: String,
        transfer: Option<String>,
        continue_loop: bool,
    }

    struct EventEmittingAgent {
        id: String,
        output: String,
    }

    #[derive(Default)]
    struct RecordingEventHandler {
        events: Vec<ExecutionEvent>,
    }

    #[async_trait]
    impl ExecutionEventHandler for RecordingEventHandler {
        async fn handle(
            &mut self,
            _ctx: &mut InvocationContext,
            event: ExecutionEvent,
        ) -> TalosResult<Option<ExecutionEvent>> {
            self.events.push(event.clone());
            Ok(Some(event))
        }
    }

    #[async_trait]
    impl Agent for StaticAgent {
        fn id(&self) -> &str {
            &self.id
        }

        async fn run(
            &self,
            _ctx: &mut InvocationContext,
            _event_handler: Option<&mut dyn ExecutionEventHandler>,
        ) -> TalosResult<AgentRunResult> {
            Ok(AgentRunResult {
                output_text: self.output.clone(),
                output_items: vec![TalosOutputItem::OutputText {
                    text: self.output.clone(),
                }],
                stop_reason: StopReason::EndTurn,
                usage: TalosUsage::default(),
                warnings: Vec::new(),
                task_results: vec![TaskResult {
                    task_id: format!("task-{}", self.id),
                    kind: TaskKind::General,
                    goal: self.output.clone(),
                    status: TaskStatus::Success,
                    output: self.output.clone(),
                    output_items: Vec::new(),
                    usage: TalosUsage::default(),
                    warnings: Vec::new(),
                    tool_traces: Vec::new(),
                    state_delta: HashMap::new(),
                    error: None,
                }],
                state_delta: HashMap::new(),
                continue_loop: self.continue_loop,
                requested_transfer: self.transfer.clone(),
            })
        }
    }

    #[async_trait]
    impl Agent for EventEmittingAgent {
        fn id(&self) -> &str {
            &self.id
        }

        async fn run(
            &self,
            ctx: &mut InvocationContext,
            mut event_handler: Option<&mut dyn ExecutionEventHandler>,
        ) -> TalosResult<AgentRunResult> {
            if let Some(handler) = event_handler.as_deref_mut() {
                let _ = handler
                    .handle(
                        ctx,
                        ExecutionEvent::text(
                            ctx,
                            EventAuthor::Agent,
                            ExecutionBranch::ReactiveLoop,
                            ExecutionEventKind::FinalResponse,
                            self.output.clone(),
                            false,
                        ),
                    )
                    .await?;
            }

            Ok(AgentRunResult {
                output_text: self.output.clone(),
                output_items: vec![TalosOutputItem::OutputText {
                    text: self.output.clone(),
                }],
                stop_reason: StopReason::EndTurn,
                usage: TalosUsage::default(),
                warnings: Vec::new(),
                task_results: vec![TaskResult {
                    task_id: format!("task-{}", self.id),
                    kind: TaskKind::General,
                    goal: self.output.clone(),
                    status: TaskStatus::Success,
                    output: self.output.clone(),
                    output_items: Vec::new(),
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

    fn mock_ctx() -> InvocationContext {
        InvocationContext {
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
            phase: crate::TurnPhase::Execution,
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
            plugin_manager: crate::default_plugin_manager(),
            transcript_service: None,
            stream_tx: None,
        }
    }

    #[tokio::test]
    async fn test_sequential_agent_runs_all_children() {
        let seq = SequentialKernelAgent::new(
            "seq",
            vec![
                Arc::new(StaticAgent {
                    id: "a".to_string(),
                    output: "a-out".to_string(),
                    transfer: None,
                    continue_loop: false,
                }),
                Arc::new(StaticAgent {
                    id: "b".to_string(),
                    output: "b-out".to_string(),
                    transfer: None,
                    continue_loop: false,
                }),
            ],
        );
        let mut ctx = mock_ctx();
        let result = seq
            .run(&mut ctx, None)
            .await
            .expect("sequential should succeed");
        assert_eq!(result.output_text, "b-out");
        assert_eq!(result.task_results.len(), 2);
    }

    #[tokio::test]
    async fn test_parallel_agent_merges_results() {
        let par = ParallelKernelAgent::new(
            "par",
            vec![
                Arc::new(StaticAgent {
                    id: "a".to_string(),
                    output: "a-out".to_string(),
                    transfer: None,
                    continue_loop: false,
                }),
                Arc::new(StaticAgent {
                    id: "b".to_string(),
                    output: "b-out".to_string(),
                    transfer: None,
                    continue_loop: false,
                }),
            ],
        );
        let mut ctx = mock_ctx();
        let result = par
            .run(&mut ctx, None)
            .await
            .expect("parallel should succeed");
        assert_eq!(result.task_results.len(), 2);
        assert_eq!(result.output_text, "b-out");
    }

    #[tokio::test]
    async fn test_sequential_agent_forwards_event_handler_to_children() {
        let seq = SequentialKernelAgent::new(
            "seq",
            vec![Arc::new(EventEmittingAgent {
                id: "a".to_string(),
                output: "a-out".to_string(),
            })],
        );
        let mut ctx = mock_ctx();
        let mut handler = RecordingEventHandler::default();

        let result = seq
            .run(&mut ctx, Some(&mut handler))
            .await
            .expect("sequential should succeed");

        assert_eq!(result.output_text, "a-out");
        assert_eq!(handler.events.len(), 1);
    }

    #[tokio::test]
    async fn test_parallel_agent_replays_child_events_into_parent_handler() {
        let par = ParallelKernelAgent::new(
            "par",
            vec![
                Arc::new(EventEmittingAgent {
                    id: "a".to_string(),
                    output: "a-out".to_string(),
                }),
                Arc::new(EventEmittingAgent {
                    id: "b".to_string(),
                    output: "b-out".to_string(),
                }),
            ],
        );
        let mut ctx = mock_ctx();
        let mut handler = RecordingEventHandler::default();

        let result = par
            .run(&mut ctx, Some(&mut handler))
            .await
            .expect("parallel should succeed");

        assert_eq!(result.task_results.len(), 2);
        assert_eq!(handler.events.len(), 2);
    }

    #[tokio::test]
    async fn test_loop_agent_repeats_until_continue_loop_false() {
        let loop_agent = LoopKernelAgent::new(
            "loop",
            Arc::new(StaticAgent {
                id: "a".to_string(),
                output: "a-out".to_string(),
                transfer: None,
                continue_loop: false,
            }),
            3,
        );
        let mut ctx = mock_ctx();
        let result = loop_agent
            .run(&mut ctx, None)
            .await
            .expect("loop agent should succeed");
        assert_eq!(result.task_results.len(), 1);
    }

    #[tokio::test]
    async fn test_routing_agent_handles_transfer_to_registered_node() {
        let mut router = RoutingKernelAgent::new("router", "a");
        router.register_node(
            "a",
            Arc::new(StaticAgent {
                id: "a".to_string(),
                output: "from-a".to_string(),
                transfer: Some("b".to_string()),
                continue_loop: false,
            }),
        );
        router.register_node(
            "b",
            Arc::new(StaticAgent {
                id: "b".to_string(),
                output: "from-b".to_string(),
                transfer: None,
                continue_loop: false,
            }),
        );

        let mut ctx = mock_ctx();
        let result = router
            .run(&mut ctx, None)
            .await
            .expect("router should succeed");
        assert_eq!(result.output_text, "from-b");
        assert!(result.requested_transfer.is_none());
    }

    #[tokio::test]
    async fn test_routing_agent_forwards_event_handler_across_hops() {
        let mut router = RoutingKernelAgent::new("router", "a");
        router.register_node(
            "a",
            Arc::new(StaticAgent {
                id: "a".to_string(),
                output: "from-a".to_string(),
                transfer: Some("b".to_string()),
                continue_loop: false,
            }),
        );
        router.register_node(
            "b",
            Arc::new(EventEmittingAgent {
                id: "b".to_string(),
                output: "from-b".to_string(),
            }),
        );

        let mut ctx = mock_ctx();
        let mut handler = RecordingEventHandler::default();
        let result = router
            .run(&mut ctx, Some(&mut handler))
            .await
            .expect("router should succeed");

        assert_eq!(result.output_text, "from-b");
        assert_eq!(handler.events.len(), 1);
    }
}
