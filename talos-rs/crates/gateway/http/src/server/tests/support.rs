use super::*;

pub(crate) struct MockGateway;
pub(crate) struct SilentGateway;
pub(crate) struct MockSessionService;
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
impl Gateway for MockGateway {
    async fn handle_request(
        &self,
        _request: GatewayRequest,
    ) -> TalosResult<talos_gateway::GatewayResponse> {
        Ok(talos_gateway::GatewayResponse {
            content: "mock content".to_string(),
            output_items: vec![TalosOutputItem::OutputText {
                text: "mock content".to_string(),
            }],
            reasoning: vec![TalosReasoning {
                text: "mock reasoning".to_string(),
            }],
            stop_reason: StopReason::EndTurn,
            usage: TalosUsage::default(),
            success: true,
            model: "qwen-plus".to_string(),
            session_id: "mock-session".to_string(),
        })
    }

    async fn handle_request_stream(
        &self,
        _request: GatewayRequest,
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

    async fn start(&self) -> TalosResult<()> {
        Ok(())
    }

    async fn cancel_active_turn(&self, _session_id: &str) -> TalosResult<()> {
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

#[async_trait::async_trait]
impl Gateway for SilentGateway {
    async fn handle_request(
        &self,
        _request: GatewayRequest,
    ) -> TalosResult<talos_gateway::GatewayResponse> {
        unreachable!("silent gateway is only used by async stream tests")
    }

    async fn handle_request_stream(
        &self,
        _request: GatewayRequest,
    ) -> TalosResult<tokio::sync::mpsc::Receiver<TalosTimelineEvent>> {
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            drop(tx);
        });
        Ok(rx)
    }

    async fn start(&self) -> TalosResult<()> {
        Ok(())
    }

    async fn cancel_active_turn(&self, _session_id: &str) -> TalosResult<()> {
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

#[async_trait::async_trait]
impl SessionService for MockSessionService {
    async fn create_session(
        &self,
        context: TalosContext,
        model: Option<String>,
        conf: Option<talos_types::SessionConf>,
        project_id: Option<String>,
        current_directory: Option<std::path::PathBuf>,
    ) -> TalosResult<talos_types::TalosSession> {
        Ok(talos_types::TalosSession {
            title: "Untitled Session".to_string(),
            user_id: context.user_id.clone(),
            context: talos_types::SessionContext {
                session_id: "mock-session".to_string(),
                workspace_id: None,
                source_type: context.source_type,
                provider: context.provider,
                user_id: context.user_id,
                message_id: context.message_id,
                trace_id: context.trace_id,
                channel_id: context.channel_id,
                thread_id: context.thread_id,
                project_id: project_id.clone(),
                current_directory: current_directory.clone(),
                project_root: context.project_root,
                environment: context.environment,
            },
            conf: conf.unwrap_or_default(),
            project_id,
            current_directory,
            config: {
                let mut config = talos_types::TalosConfig::default();
                let resolved = model.unwrap_or_else(|| "qwen-plus".to_string());
                config.agent.model = resolved.clone();
                config.llm.model = resolved;
                config
            },
            state: talos_types::SessionState::default(),
        })
    }

    async fn list_sessions(
        &self,
        _query: talos_types::SessionQuery,
    ) -> TalosResult<Vec<talos_types::TalosSession>> {
        Ok(Vec::new())
    }

    async fn count_sessions(&self, _query: talos_types::SessionQuery) -> TalosResult<usize> {
        Ok(0)
    }

    async fn get_session(
        &self,
        session_id: &str,
    ) -> TalosResult<Option<talos_types::TalosSession>> {
        if session_id != "mock-session" {
            return Ok(None);
        }
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
                message_count: 1,
                usage: talos_types::talos::TalosUsage::default(),
            },
        }))
    }

    async fn patch_session(
        &self,
        _session_id: &str,
        _patch: talos_types::SessionPatch,
    ) -> TalosResult<talos_types::SessionPatchAck> {
        Ok(talos_types::SessionPatchAck { success: true })
    }

    async fn compact_session(&self, session_id: &str) -> TalosResult<talos_types::TalosSession> {
        self.get_session(session_id)
            .await?
            .ok_or_else(|| talos_error::TalosError::ResourceNotFound(session_id.to_string()))
    }

    async fn clear_session(&self, session_id: &str) -> TalosResult<talos_types::TalosSession> {
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

    async fn delete_session(&self, _session_id: &str) -> TalosResult<talos_types::SessionPatchAck> {
        Ok(talos_types::SessionPatchAck { success: true })
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

    async fn list_projects(&self) -> TalosResult<Vec<talos_types::ProjectRecord>> {
        Ok(Vec::new())
    }

    async fn get_project(
        &self,
        _project_id: &str,
    ) -> TalosResult<Option<talos_types::ProjectRecord>> {
        Ok(None)
    }

    async fn create_project(
        &self,
        name: String,
        directory: std::path::PathBuf,
        _user_id: String,
    ) -> TalosResult<talos_types::ProjectRecord> {
        Ok(talos_types::ProjectRecord {
            project_id: "project-mock".to_string(),
            name,
            user_id: "static-token".to_string(),
            directory,
            created_at: 0,
            updated_at: 0,
            session_count: 0,
        })
    }

    async fn patch_project(
        &self,
        _project_id: &str,
        _patch: talos_types::ProjectPatch,
    ) -> TalosResult<Option<talos_types::ProjectRecord>> {
        Ok(None)
    }

    async fn delete_project(&self, _project_id: &str) -> TalosResult<()> {
        Ok(())
    }
}
