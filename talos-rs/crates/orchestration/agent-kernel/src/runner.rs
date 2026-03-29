use crate::{
    Agent, FinalResponseEventData, IterationEventData, KernelEvent, KernelEventData,
    KernelEventLabel, KernelEventType, PluginRegistry, RunRequest, RunResponse, Runner,
    TaskCompletedEventData, TransferRequestedEventData, UserInputEventData,
};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use talos_error::TalosResult;

pub struct DefaultRunner {
    plugins: PluginRegistry,
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
            input,
            model,
            phase,
            planning_instruction,
            transfer_to,
            initial_state_bags,
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
            input,
            model: model.clone(),
            phase,
            planning_instruction,
            transfer_to,
            execution_state: crate::InvocationExecutionState::default(),
            session: crate::InvocationSession {
                session_id: ctx_session_id,
                ..crate::InvocationSession::default()
            },
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
            self.plugins.before_plan(&mut ctx).await?;
            ctx.events.push(KernelEvent::new(
                KernelEventType::PlanCreated,
                KernelEventLabel::Iteration,
                KernelEventData::Iteration(IterationEventData {
                    iteration: iterations,
                }),
            ));

            let mut run_result = agent.run(&mut ctx).await?;
            for task_result in &run_result.task_results {
                self.plugins
                    .before_task(
                        &mut ctx,
                        &crate::TaskSpec {
                            id: task_result.task_id.clone(),
                            kind: task_result.kind.clone(),
                            goal: task_result.goal.clone(),
                            input: serde_json::Value::Null,
                            constraints: crate::TaskConstraints::default(),
                            tool_candidates: Vec::new(),
                            expected_output: None,
                        },
                    )
                    .await?;
                self.plugins.after_task(&mut ctx, task_result).await?;
                ctx.events.push(KernelEvent::new(
                    KernelEventType::TaskCompleted,
                    KernelEventLabel::TaskCompleted,
                    KernelEventData::TaskCompleted(TaskCompletedEventData {
                        task_id: task_result.task_id.clone(),
                        status: task_result.status.clone(),
                    }),
                ));
            }
            self.plugins
                .after_finalize(&mut ctx, &mut run_result)
                .await?;

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
            model,
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
                    input: "hello".to_string(),
                    model: "m1".to_string(),
                    phase: TurnPhase::Execution,
                    planning_instruction: None,
                    transfer_to: None,
                    initial_state_bags: None,
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
}
