use super::AgentImpl;
use crate::{
    service::{OrchestratorExecution, OrchestratorRequest},
    Agent, AgentOrchestrator, AgentResponse,
};
use async_trait::async_trait;
use std::collections::HashMap;
use std::time::Duration;
use talos_agent_kernel::{
    Agent as KernelAgent, AgentRunResult, InvocationContext, StopReason, TaskKind, TaskResult,
    TaskStatus, TimelineBuilder,
};
use talos_error::TalosResult;
use talos_types::talos::{
    TalosOutputItem, TalosTimelineDelta, TalosTimelineEvent, TalosTimelineItemStatus,
    TalosToolProgressKind, TalosUsage,
};
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
    ) -> TalosResult<mpsc::Receiver<TalosTimelineEvent>> {
        let turn_id = request
            .turn_id
            .clone()
            .unwrap_or_else(|| format!("turn_{}", Uuid::new_v4().simple()));
        let message_id = request.session.context.message_id.clone();
        let session_id = request.session.context.session_id.clone();
        let execution = AgentOrchestrator::run(self, request).await?;
        let request_id = format!("req_{}", Uuid::new_v4().simple());
        let (tx, rx) = mpsc::channel(8);
        tokio::spawn(async move {
            let mut timeline = TimelineBuilder::new(request_id, turn_id, message_id, session_id);
            let _ = tx.send(timeline.start_turn()).await;
            let answer_started = timeline.start_final_answer();
            let item_id = match &answer_started.payload {
                talos_types::talos::TalosTimelineEventPayload::ItemStarted(payload) => {
                    payload.item.item_id.clone()
                }
                _ => unreachable!("start_final_answer returns item_started"),
            };
            let _ = tx.send(answer_started).await;
            if let Ok(delta) = timeline.append_delta(
                &item_id,
                TalosTimelineDelta::Text {
                    text: execution.output_text,
                },
            ) {
                let _ = tx.send(delta).await;
            }
            if let Ok(completed) =
                timeline.complete_item(&item_id, TalosTimelineItemStatus::Completed, None)
            {
                let _ = tx.send(completed).await;
            }
            let tool_started =
                timeline.start_tool_call("call-1", "Bash", serde_json::json!({}).to_string());
            let tool_item_id = match &tool_started.payload {
                talos_types::talos::TalosTimelineEventPayload::ItemStarted(payload) => {
                    payload.item.item_id.clone()
                }
                _ => unreachable!("start_tool_call returns item_started"),
            };
            let _ = tx.send(tool_started).await;
            if let Ok(progress) = timeline.append_delta(
                &tool_item_id,
                TalosTimelineDelta::ToolProgress {
                    progress_kind: TalosToolProgressKind::Status,
                    text_delta: "execution started".to_string(),
                },
            ) {
                let _ = tx.send(progress).await;
            }
            if let Ok(completed) =
                timeline.complete_item(&tool_item_id, TalosTimelineItemStatus::Completed, None)
            {
                let _ = tx.send(completed).await;
            }
            let _ = tx
                .send(timeline.complete_turn(execution.stop_reason, execution.usage))
                .await;
        });
        Ok(rx)
    }

    async fn close_session(&self, _session: talos_types::TalosSession) -> TalosResult<()> {
        Ok(())
    }

    async fn compact_session(
        &self,
        _session: talos_types::TalosSession,
    ) -> TalosResult<TalosUsage> {
        Ok(TalosUsage::default())
    }

    async fn clear_session(&self, _session: talos_types::TalosSession) -> TalosResult<()> {
        Ok(())
    }

    async fn drain_memory_flushes(&self, _timeout: Duration) -> TalosResult<()> {
        Ok(())
    }

    fn pending_memory_flushes(&self) -> usize {
        0
    }

    async fn has_pending_memory_flush(&self, _session_id: &str) -> bool {
        false
    }

    fn has_active_turn(&self, _session_id: &str) -> bool {
        false
    }

    async fn cancel_active_turn(&self, _session_id: &str) -> TalosResult<()> {
        Ok(())
    }

    async fn drain_active_turns(&self, _timeout: Duration) -> TalosResult<()> {
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
