use super::*;
use std::sync::Mutex;
use talos_types::agent_dispatch::AgentDispatchRequest;
use talos_types::talos::{TalosProvider, TalosSourceType};

pub(crate) struct MockGateway;
pub(crate) struct SilentGateway;
#[derive(Default)]
pub(crate) struct MockSessionService {
    session_cleared: Mutex<bool>,
}
pub(crate) struct MockLlmProvider;

fn test_timeline_event(
    sequence: u64,
    event_type: TalosTimelineEventType,
    payload: TalosTimelineEventPayload,
) -> TalosTimelineEvent {
    TalosTimelineEvent {
        event_seq: None,
        event_id: None,
        created_at_ms: None,
        event_type,
        turn_id: "turn_mock".to_string(),
        message_id: "msg_mock".to_string(),
        session_id: "mock-session".to_string(),
        sequence,
        payload,
    }
}

#[async_trait::async_trait]
impl LlmProvider for MockLlmProvider {
    fn name(&self) -> &str {
        "mock"
    }

    fn default_model(&self) -> String {
        "mock-model".to_string()
    }

    async fn chat(&self, _request: ChatRequest) -> TalosResult<ChatResponse> {
        let content = if _request.messages.iter().any(|message| {
            message
                .content
                .contains("Respond ONLY with a valid JSON array of file paths.")
        }) {
            "[\"memory/2026-04-04.md\"]".to_string()
        } else if _request.messages.iter().any(|message| {
            message
                .content
                .contains("Respond ONLY with a valid JSON array.")
        }) {
            "[{\"path\":\"memory/2026-04-04.md\",\"from\":1,\"to\":5}]".to_string()
        } else {
            String::new()
        };
        Ok(ChatResponse {
            content,
            reasoning: Vec::new(),
            model: "mock-model".to_string(),
            usage: Usage::default(),
            finish_reason: None,
            tool_calls: None,
        })
    }

    async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        drop(tx);
        Ok(ChatStream { rx })
    }
}

#[async_trait::async_trait]
impl TurnExecutor for MockGateway {
    async fn start_turn_stream(
        &self,
        _request: AgentDispatchRequest,
    ) -> TalosResult<tokio::sync::mpsc::Receiver<TalosTimelineEvent>> {
        let (tx, rx) = tokio::sync::mpsc::channel(8);
        tokio::spawn(async move {
            let started = test_timeline_event(
                0,
                TalosTimelineEventType::TurnStarted,
                TalosTimelineEventPayload::TurnStarted(TalosTurnStartedEvent {
                    request_id: "req_mock".to_string(),
                    turn_id: "turn_mock".to_string(),
                    started_at_ms: 0,
                }),
            );
            let _ = tx.send(started).await;

            let assistant_item = TalosTimelineItem {
                item_id: "assistant-1".to_string(),
                parent_item_id: None,
                kind: TalosTimelineItemKind::AssistantText,
                phase: TalosTimelinePhase::FinalAnswer,
                status: TalosTimelineItemStatus::Running,
                title: None,
                started_at_ms: 1,
                completed_at_ms: None,
                content: TalosTimelineContent::Text {
                    text: String::new(),
                },
            };
            let output_started = test_timeline_event(
                1,
                TalosTimelineEventType::ItemStarted,
                TalosTimelineEventPayload::ItemStarted(TalosItemStartedEvent {
                    request_id: "req_mock".to_string(),
                    turn_id: "turn_mock".to_string(),
                    item: assistant_item,
                }),
            );
            let _ = tx.send(output_started).await;

            let output = test_timeline_event(
                2,
                TalosTimelineEventType::ItemDelta,
                TalosTimelineEventPayload::ItemDelta(TalosItemDeltaEvent {
                    request_id: "req_mock".to_string(),
                    turn_id: "turn_mock".to_string(),
                    item_id: "assistant-1".to_string(),
                    delta: TalosTimelineDelta::Text {
                        text: "stream only content".to_string(),
                    },
                }),
            );
            let _ = tx.send(output).await;

            let tool_item = TalosTimelineItem {
                item_id: "tool-1".to_string(),
                parent_item_id: None,
                kind: TalosTimelineItemKind::ToolCall,
                phase: TalosTimelinePhase::Acting,
                status: TalosTimelineItemStatus::Running,
                title: Some("Bash".to_string()),
                started_at_ms: 3,
                completed_at_ms: None,
                content: TalosTimelineContent::ToolCall {
                    call_id: "call-1".to_string(),
                    name: "Bash".to_string(),
                    arguments: String::new(),
                    progress: Vec::new(),
                    result: None,
                },
            };
            let tool_started = test_timeline_event(
                3,
                TalosTimelineEventType::ItemStarted,
                TalosTimelineEventPayload::ItemStarted(TalosItemStartedEvent {
                    request_id: "req_mock".to_string(),
                    turn_id: "turn_mock".to_string(),
                    item: tool_item,
                }),
            );
            let _ = tx.send(tool_started).await;

            let progress = test_timeline_event(
                4,
                TalosTimelineEventType::ItemDelta,
                TalosTimelineEventPayload::ItemDelta(TalosItemDeltaEvent {
                    request_id: "req_mock".to_string(),
                    turn_id: "turn_mock".to_string(),
                    item_id: "tool-1".to_string(),
                    delta: TalosTimelineDelta::ToolProgress {
                        progress_kind: TalosToolProgressKind::Status,
                        text_delta: "execution started".to_string(),
                    },
                }),
            );
            let _ = tx.send(progress).await;

            let completed = test_timeline_event(
                5,
                TalosTimelineEventType::TurnCompleted,
                TalosTimelineEventPayload::TurnCompleted(TalosTurnCompletedEvent {
                    request_id: "req_mock".to_string(),
                    turn_id: "turn_mock".to_string(),
                    completed_at_ms: 5,
                    stop_reason: StopReason::EndTurn,
                    usage: TalosUsage::default(),
                    metrics: None,
                }),
            );
            let _ = tx.send(completed).await;
        });
        Ok(rx)
    }

    async fn cancel_active_turn(&self, _session: TalosSession) -> TalosResult<()> {
        Ok(())
    }

    async fn submit_dynamic_tool_result(
        &self,
        _session: TalosSession,
        _call_id: &str,
        _tool_name: &str,
        _output: String,
        _success: bool,
    ) -> TalosResult<()> {
        Ok(())
    }
}

#[async_trait::async_trait]
impl TurnExecutor for SilentGateway {
    async fn start_turn_stream(
        &self,
        _request: AgentDispatchRequest,
    ) -> TalosResult<tokio::sync::mpsc::Receiver<TalosTimelineEvent>> {
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            drop(tx);
        });
        Ok(rx)
    }

    async fn cancel_active_turn(&self, _session: TalosSession) -> TalosResult<()> {
        Ok(())
    }

    async fn submit_dynamic_tool_result(
        &self,
        _session: TalosSession,
        _call_id: &str,
        _tool_name: &str,
        _output: String,
        _success: bool,
    ) -> TalosResult<()> {
        Ok(())
    }
}

#[async_trait::async_trait]
impl SessionService for MockSessionService {
    async fn list_sessions(
        &self,
        _request: talos_types::SessionListRequest,
    ) -> TalosResult<Vec<talos_types::TalosSession>> {
        Ok(Vec::new())
    }

    async fn count_sessions(
        &self,
        _request: talos_types::SessionListRequest,
    ) -> TalosResult<usize> {
        Ok(0)
    }

    async fn get_session(
        &self,
        session_id: &str,
    ) -> TalosResult<Option<talos_types::TalosSession>> {
        if session_id != "mock-session" {
            return Ok(None);
        }
        let session_cleared = *self.session_cleared.lock().expect("session state lock");
        Ok(Some(talos_types::TalosSession {
            title: "Untitled Session".to_string(),
            user_id: "static-token".to_string(),
            context: talos_types::SessionContext {
                session_id: "mock-session".to_string(),
                workspace_id: None,
                source_type: TalosSourceType::Http,
                provider: TalosProvider::Http,
                user_id: "static-token".to_string(),
                message_id: "message-1".to_string(),
                trace_id: "trace-1".to_string(),
                channel_id: Some("test-channel".to_string()),
                thread_id: None,
                project_id: None,
                current_directory: None,
                project_root: None,
                environment: None,
            },
            conf: talos_types::SessionConf::default(),
            project_id: None,
            current_directory: None,
            config: {
                let mut config = talos_types::TalosConfig::default();
                config.agent.model = "qwen-plus".to_string();
                config.llm.model = "qwen-plus".to_string();
                config
            },
            state: talos_types::SessionState {
                created_at: 0,
                last_activity: 0,
                message_count: if session_cleared { 0 } else { 1 },
                usage: talos_types::talos::TalosUsage::default(),
            },
        }))
    }

    async fn clear_session(&self, session_id: &str) -> TalosResult<talos_types::TalosSession> {
        *self.session_cleared.lock().expect("session state lock") = true;
        let mut session = self
            .get_session(session_id)
            .await?
            .ok_or_else(|| talos_error::TalosError::ResourceNotFound(session_id.to_string()))?;
        session.state.message_count = 0;
        session.state.usage = talos_types::talos::TalosUsage::default();
        Ok(session)
    }

    async fn close_session(&self, _session_id: &str) -> TalosResult<()> {
        Ok(())
    }

    async fn delete_session(&self, _session_id: &str) -> TalosResult<SessionPatchAck> {
        Ok(SessionPatchAck { success: true })
    }

    async fn cancel_active_turn(&self, _session_id: &str) -> TalosResult<()> {
        Ok(())
    }

    async fn submit_dynamic_tool_result(
        &self,
        session_id: &str,
        _call_id: &str,
        _tool_name: &str,
        _output: String,
        _success: bool,
    ) -> TalosResult<()> {
        if session_id != "mock-session" {
            return Err(talos_error::TalosError::ResourceNotFound(format!(
                "session {session_id}"
            )));
        }
        Ok(())
    }
}
