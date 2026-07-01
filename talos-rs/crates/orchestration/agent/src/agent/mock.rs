use super::request::AgentKernelParams;
use crate::{
    service::{
        AgentInputParams, AgentInputResult, AgentInterruptParams, AgentInterruptResult,
        AgentRunParams, AgentRunResult,
    },
    AgentHarness,
};
use async_trait::async_trait;
use std::time::Duration;
use talos_agent_kernel::TimelineBuilder;
use talos_error::TalosResult;
use talos_types::talos::{
    AgentEvent, RunItem, TalosTimelineDelta, TalosTimelineItemStatus, TalosToolProgressKind,
    TokenUsage,
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
impl AgentHarness for MockAgent {
    fn id(&self) -> &str {
        "mock"
    }

    async fn run(&self, request: AgentRunParams) -> TalosResult<AgentRunResult> {
        let input = AgentKernelParams::extract_text_input(&request.input);
        let output = format!("Mock response to: {}", input);
        Ok(AgentRunResult {
            output_text: output,
            output_items: vec![RunItem::OutputText {
                text: "Mock response".to_string(),
            }],
            reasoning: Vec::new(),
            stop_reason: talos_types::talos::StopReason::EndTurn,
            usage: TokenUsage::default(),
            task_count: 1,
            session_id: request.session.context.session_id,
            session_title_update: None,
        })
    }

    async fn run_streamed(
        &self,
        request: AgentRunParams,
    ) -> TalosResult<mpsc::Receiver<AgentEvent>> {
        let turn_id = request
            .turn_id
            .clone()
            .unwrap_or_else(|| format!("turn_{}", Uuid::new_v4().simple()));
        let message_id = request.session.context.message_id.clone();
        let session_id = request.session.context.session_id.clone();
        let execution = AgentHarness::run(self, request).await?;
        let request_id = format!("req_{}", Uuid::new_v4().simple());
        let (tx, rx) = mpsc::channel(8);
        tokio::spawn(async move {
            let mut timeline = TimelineBuilder::new(request_id, turn_id, message_id, session_id);
            let _ = tx.send(timeline.start_turn()).await;
            let answer_started = timeline.start_final_answer();
            let item_id = match &answer_started.payload {
                talos_types::talos::AgentEventPayload::ItemStarted(payload) => {
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
                talos_types::talos::AgentEventPayload::ItemStarted(payload) => {
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
                .send(timeline.complete_turn(execution.stop_reason, execution.usage, None))
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
    ) -> TalosResult<TokenUsage> {
        Ok(TokenUsage::default())
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

    async fn interrupt(&self, _params: AgentInterruptParams) -> TalosResult<AgentInterruptResult> {
        Ok(AgentInterruptResult { interrupted: false })
    }

    async fn drain_active_turns(&self, _timeout: Duration) -> TalosResult<()> {
        Ok(())
    }

    async fn submit_input(&self, _params: AgentInputParams) -> TalosResult<AgentInputResult> {
        Ok(AgentInputResult { accepted: true })
    }
}
