use super::AgentImpl;
use crate::{
    service::{
        OrchestratorEvent, OrchestratorExecution, OrchestratorMessageCompletedEvent,
        OrchestratorMessageOutputDeltaEvent, OrchestratorMessageStartedEvent,
        OrchestratorMessageToolCallExecutingEvent, OrchestratorMessageToolCallProgressDeltaEvent,
        OrchestratorMessageToolCallQueuedEvent, OrchestratorRequest, OrchestratorToolProgressKind,
        OrchestratorToolStreamKey,
    },
    Agent, AgentOrchestrator, AgentResponse,
};
use async_trait::async_trait;
use std::collections::HashMap;
use std::time::Duration;
use talos_agent_kernel::{
    Agent as KernelAgent, AgentRunResult, InvocationContext, StopReason, TaskKind, TaskResult,
    TaskStatus,
};
use talos_error::TalosResult;
use talos_types::talos::{TalosOutputItem, TalosUsage};
use tokio::sync::mpsc;
use uuid::Uuid;

pub struct MockAgent;

impl MockAgent {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Agent for MockAgent {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    async fn execute_tool(
        &self,
        _tool_name: &str,
        _args: serde_json::Value,
    ) -> TalosResult<AgentResponse> {
        Ok(AgentResponse {
            output_text: "Mock tool execution".to_string(),
            output_items: vec![TalosOutputItem::OutputText {
                text: "Mock tool execution".to_string(),
            }],
            reasoning: Vec::new(),
            tool_calls: vec![],
        })
    }
}

#[async_trait]
impl KernelAgent for MockAgent {
    fn id(&self) -> &str {
        "mock"
    }

    async fn run(
        &self,
        _ctx: &mut InvocationContext,
        _event_handler: Option<&mut dyn talos_agent_kernel::ExecutionEventHandler>,
    ) -> TalosResult<AgentRunResult> {
        Ok(AgentRunResult {
            output_text: "Mock response".to_string(),
            output_items: vec![TalosOutputItem::OutputText {
                text: "Mock response".to_string(),
            }],
            stop_reason: StopReason::EndTurn,
            usage: TalosUsage::default(),
            warnings: Vec::new(),
            task_results: vec![TaskResult {
                task_id: "mock-task".to_string(),
                kind: TaskKind::General,
                goal: "mock".to_string(),
                status: TaskStatus::Success,
                output: "Mock response".to_string(),
                output_items: vec![TalosOutputItem::OutputText {
                    text: "Mock response".to_string(),
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

#[async_trait]
impl AgentOrchestrator for MockAgent {
    fn id(&self) -> &str {
        "mock"
    }

    async fn run(&self, request: OrchestratorRequest) -> TalosResult<OrchestratorExecution> {
        let input = AgentImpl::extract_text_input(&request.input);
        let output = format!("Mock response to: {}", input);
        Ok(OrchestratorExecution {
            output_text: output,
            output_items: vec![TalosOutputItem::OutputText {
                text: "Mock response".to_string(),
            }],
            reasoning: Vec::new(),
            stop_reason: talos_types::talos::StopReason::EndTurn,
            usage: TalosUsage::default(),
            task_count: 1,
            session_id: request.session.context.session_id,
        })
    }

    async fn run_stream(
        &self,
        request: OrchestratorRequest,
    ) -> TalosResult<mpsc::Receiver<OrchestratorEvent>> {
        let execution = AgentOrchestrator::run(self, request).await?;
        let request_id = format!("req_{}", Uuid::new_v4().simple());
        let (tx, rx) = mpsc::channel(8);
        tokio::spawn(async move {
            let _ = tx
                .send(OrchestratorEvent::MessageStarted(
                    OrchestratorMessageStartedEvent {
                        request_id: request_id.clone(),
                        model: "mock-model".to_string(),
                        usage: TalosUsage::default(),
                        iteration: 0,
                        step_id: "start".to_string(),
                    },
                ))
                .await;
            let _ = tx
                .send(OrchestratorEvent::MessageOutputDelta(
                    OrchestratorMessageOutputDeltaEvent {
                        request_id: request_id.clone(),
                        delta: execution.output_text,
                        item_index: 0,
                        iteration: 1,
                        step_id: "step-1".to_string(),
                    },
                ))
                .await;
            let _ = tx
                .send(OrchestratorEvent::MessageToolCallQueued(
                    OrchestratorMessageToolCallQueuedEvent {
                        request_id: request_id.clone(),
                        stream_key: OrchestratorToolStreamKey { round: 0, index: 0 },
                        call_id: "call-1".to_string(),
                        tool_name: "exec_command".to_string(),
                        item_index: 1,
                        iteration: 2,
                        step_id: "step-2".to_string(),
                    },
                ))
                .await;
            let _ = tx
                .send(OrchestratorEvent::MessageToolCallExecuting(
                    OrchestratorMessageToolCallExecutingEvent {
                        request_id: request_id.clone(),
                        stream_key: OrchestratorToolStreamKey { round: 0, index: 0 },
                        call_id: "call-1".to_string(),
                        tool_name: "exec_command".to_string(),
                        item_index: 2,
                        iteration: 3,
                        step_id: "step-3".to_string(),
                    },
                ))
                .await;
            let _ = tx
                .send(OrchestratorEvent::MessageToolCallProgressDelta(
                    OrchestratorMessageToolCallProgressDeltaEvent {
                        request_id: request_id.clone(),
                        stream_key: OrchestratorToolStreamKey { round: 0, index: 0 },
                        call_id: "call-1".to_string(),
                        tool_name: "exec_command".to_string(),
                        message: "execution started".to_string(),
                        progress_kind: OrchestratorToolProgressKind::Status,
                        item_index: 3,
                        iteration: 4,
                        step_id: "step-4".to_string(),
                    },
                ))
                .await;
            let _ = tx
                .send(OrchestratorEvent::MessageCompleted(
                    OrchestratorMessageCompletedEvent {
                        request_id,
                        stop_reason: execution.stop_reason,
                        usage: execution.usage,
                        total_output_items: 4,
                        iteration: 5,
                        step_id: "completed".to_string(),
                        task_count: execution.task_count,
                    },
                ))
                .await;
        });
        Ok(rx)
    }

    async fn close_session(&self, _session: talos_types::TalosSession) -> TalosResult<()> {
        Ok(())
    }

    async fn drain_memory_flushes(&self, _timeout: Duration) -> TalosResult<()> {
        Ok(())
    }

    fn pending_memory_flushes(&self) -> usize {
        0
    }

    async fn cancel_active_turn(&self, _session_id: &str) -> TalosResult<()> {
        Ok(())
    }

    async fn submit_tool_approval(
        &self,
        _session_id: &str,
        _call_id: &str,
        _tool_name: &str,
        _approved: bool,
        _reason: Option<String>,
    ) -> TalosResult<()> {
        Ok(())
    }

    async fn submit_dynamic_tool_result(
        &self,
        _session_id: &str,
        _call_id: &str,
        _tool_name: &str,
        _output: String,
        _success: bool,
    ) -> TalosResult<()> {
        Ok(())
    }
}
