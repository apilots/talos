use crate::{Agent, AgentRunResult, InvocationContext, TaskResult};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use talos_error::TalosResult;
use talos_types::talos::TalosUsage;
use tokio::task::JoinSet;

fn merge_usage(mut acc: TalosUsage, usage: &TalosUsage) -> TalosUsage {
    acc.input_tokens = acc.input_tokens.saturating_add(usage.input_tokens);
    acc.output_tokens = acc.output_tokens.saturating_add(usage.output_tokens);
    acc.total_tokens = acc.total_tokens.saturating_add(usage.total_tokens);
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

    async fn run(&self, ctx: &mut InvocationContext) -> TalosResult<AgentRunResult> {
        let mut output_text = String::new();
        let mut output_items = Vec::new();
        let mut stop_reason = "end_turn".to_string();
        let mut usage = TalosUsage::default();
        let mut task_results: Vec<TaskResult> = Vec::new();
        let mut state_delta = HashMap::new();
        let mut requested_transfer = None;
        let mut continue_loop = false;

        for child in &self.children {
            let result = child.run(ctx).await?;
            output_text = result.output_text.clone();
            output_items.extend(result.output_items.clone());
            stop_reason = result.stop_reason.clone();
            usage = merge_usage(usage, &result.usage);
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

    async fn run(&self, ctx: &mut InvocationContext) -> TalosResult<AgentRunResult> {
        let mut set = JoinSet::new();
        for (index, child) in self.children.iter().enumerate() {
            let child = child.clone();
            let mut child_ctx = ctx.clone();
            set.spawn(async move {
                let result = child.run(&mut child_ctx).await?;
                Ok::<_, talos_error::TalosError>((index, result))
            });
        }

        let mut ordered: Vec<(usize, AgentRunResult)> = Vec::new();
        while let Some(joined) = set.join_next().await {
            let pair = joined.map_err(|e| {
                talos_error::TalosError::InternalError(format!(
                    "parallel agent task join failed: {}",
                    e
                ))
            })??;
            ordered.push(pair);
        }
        ordered.sort_by_key(|(index, _)| *index);

        let mut output_text = String::new();
        let mut output_items = Vec::new();
        let mut stop_reason = "end_turn".to_string();
        let mut usage = TalosUsage::default();
        let mut task_results: Vec<TaskResult> = Vec::new();
        let mut state_delta = HashMap::new();
        let mut requested_transfer = None;
        let mut continue_loop = false;

        for (_, result) in ordered {
            output_text = result.output_text.clone();
            output_items.extend(result.output_items);
            stop_reason = result.stop_reason.clone();
            usage = merge_usage(usage, &result.usage);
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

    async fn run(&self, ctx: &mut InvocationContext) -> TalosResult<AgentRunResult> {
        let mut merged = AgentRunResult {
            output_text: String::new(),
            output_items: Vec::new(),
            stop_reason: "end_turn".to_string(),
            usage: TalosUsage::default(),
            task_results: Vec::new(),
            state_delta: HashMap::new(),
            continue_loop: false,
            requested_transfer: None,
        };

        for _ in 0..self.max_iterations {
            let result = self.child.run(ctx).await?;
            merged.output_text = result.output_text.clone();
            merged.output_items.extend(result.output_items.clone());
            merged.stop_reason = result.stop_reason.clone();
            merged.usage = merge_usage(merged.usage, &result.usage);
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

    async fn run(&self, ctx: &mut InvocationContext) -> TalosResult<AgentRunResult> {
        let mut current = self.start_agent.clone();
        let mut hops = 0_u32;

        let mut merged = AgentRunResult {
            output_text: String::new(),
            output_items: Vec::new(),
            stop_reason: "end_turn".to_string(),
            usage: TalosUsage::default(),
            task_results: Vec::new(),
            state_delta: HashMap::new(),
            continue_loop: false,
            requested_transfer: None,
        };

        while hops < self.max_hops {
            hops += 1;
            let Some(agent) = self.nodes.get(&current) else {
                merged.requested_transfer = Some(current);
                merged.stop_reason = "transfer_unresolved".to_string();
                break;
            };

            let result = agent.run(ctx).await?;
            merged.output_text = result.output_text.clone();
            merged.output_items.extend(result.output_items.clone());
            merged.stop_reason = result.stop_reason.clone();
            merged.usage = merge_usage(merged.usage, &result.usage);
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
    use crate::{TaskKind, TaskStatus};
    use talos_types::talos::TalosOutputItem;

    #[derive(Clone)]
    struct StaticAgent {
        id: String,
        output: String,
        transfer: Option<String>,
        continue_loop: bool,
    }

    #[async_trait]
    impl Agent for StaticAgent {
        fn id(&self) -> &str {
            &self.id
        }

        async fn run(&self, _ctx: &mut InvocationContext) -> TalosResult<AgentRunResult> {
            Ok(AgentRunResult {
                output_text: self.output.clone(),
                output_items: vec![TalosOutputItem::OutputText {
                    text: self.output.clone(),
                }],
                stop_reason: "end_turn".to_string(),
                usage: TalosUsage::default(),
                task_results: vec![TaskResult {
                    task_id: format!("task-{}", self.id),
                    kind: TaskKind::General,
                    goal: self.output.clone(),
                    status: TaskStatus::Success,
                    output: self.output.clone(),
                    output_items: Vec::new(),
                    usage: TalosUsage::default(),
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

    fn mock_ctx() -> InvocationContext {
        InvocationContext {
            session_id: "s1".to_string(),
            invocation_id: "i1".to_string(),
            agent_id: "a1".to_string(),
            workspace_id: "w1".to_string(),
            input: "hello".to_string(),
            model: "m1".to_string(),
            phase: crate::TurnPhase::Execution,
            planning_instruction: None,
            transfer_to: None,
            state: HashMap::new(),
            state_bags: crate::ContextStateBags::default(),
            limits: crate::InvocationLimits::default(),
            events: Vec::new(),
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
        let result = seq.run(&mut ctx).await.expect("sequential should succeed");
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
        let result = par.run(&mut ctx).await.expect("parallel should succeed");
        assert_eq!(result.task_results.len(), 2);
        assert_eq!(result.output_text, "b-out");
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
            .run(&mut ctx)
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
        let result = router.run(&mut ctx).await.expect("router should succeed");
        assert_eq!(result.output_text, "from-b");
        assert!(result.requested_transfer.is_none());
    }
}
