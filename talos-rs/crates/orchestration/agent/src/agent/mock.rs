use super::request::AgentKernelParams;
use crate::{
    service::{
        AgentInputParams, AgentInputResult, AgentInterruptParams, AgentInterruptResult,
        AgentRunParams, AgentRunResult,
    },
    AgentCapabilities, AgentDescriptor, AgentHarness, AgentKind, AgentResult, AgentStatus,
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

/// Test Harness implementation that emits deterministic Agent outputs.
///
/// MockAgent is for tests and local harness wiring only. It should not become a
/// product runtime kind or a Gateway/SDK-visible Agent implementation.
pub struct MockAgent;

impl MockAgent {
    /// Create a deterministic mock Harness instance.
    pub fn new() -> Self {
        Self
    }
}

impl Default for MockAgent {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AgentHarness for MockAgent {
    fn id(&self) -> &str {
        "mock"
    }

    fn descriptor(&self) -> AgentDescriptor {
        AgentDescriptor {
            agent_id: self.id().to_string(),
            name: self.id().to_string(),
            kind: AgentKind::Kernel,
            description: "Deterministic test agent".to_string(),
            capabilities: AgentCapabilities::kernel(),
            status: AgentStatus::Enabled,
        }
    }

    async fn run(&self, request: AgentRunParams) -> AgentResult<AgentRunResult> {
        let input = AgentKernelParams::extract_text_input(&request.input);
        let output = format!("Mock response to: {}", input);
        Ok(AgentRunResult {
            output_text: output,
            output_items: vec![RunItem::OutputText {
                text: "Mock response".to_string(),
            }],
            reasoning: Default::default(),
            stop_reason: talos_types::talos::StopReason::EndTurn,
            usage: TokenUsage::default(),
            task_count: 1,
            session_id: request.context.session_id,
            generated_session_title: None,
        })
    }

    async fn run_streamed(
        &self,
        request: AgentRunParams,
    ) -> AgentResult<mpsc::Receiver<AgentEvent>> {
        let turn_id = request.turn_id().to_string();
        let message_id = request.context.message_id.clone();
        let session_id = request.context.session_id.clone();
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
                .send(timeline.complete_turn(execution.stop_reason, execution.usage, None, None))
                .await;
        });
        Ok(rx)
    }

    async fn close_session(&self, _session: talos_types::TalosSession) -> TalosResult<()> {
        Ok(())
    }

    async fn compact_session(
        &self,
        _params: crate::AgentSessionCompactionParams,
    ) -> TalosResult<TokenUsage> {
        Ok(TokenUsage::default())
    }

    async fn clear_session(&self, _session: talos_types::TalosSession) -> TalosResult<()> {
        Ok(())
    }

    fn has_active_turn(&self, _session_id: &str) -> bool {
        false
    }

    async fn interrupt(&self, _params: AgentInterruptParams) -> AgentResult<AgentInterruptResult> {
        Ok(AgentInterruptResult { interrupted: false })
    }

    async fn drain_active_turns(&self, _timeout: Duration) -> TalosResult<()> {
        Ok(())
    }

    async fn submit_input(&self, _params: AgentInputParams) -> AgentResult<AgentInputResult> {
        Ok(AgentInputResult { accepted: true })
    }
}
