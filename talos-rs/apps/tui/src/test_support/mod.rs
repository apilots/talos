pub(crate) use super::*;
mod mock_gateway;
mod render_runtime;
pub(crate) use mock_gateway::*;

use async_trait::async_trait;
pub(crate) use bytes::Bytes;
pub(crate) use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
pub(crate) use render_runtime::{
    append_live_tool_progress, interactive_transcript_text, start_live_text, start_live_tool,
};
pub(crate) use std::path::PathBuf;
use std::sync::Mutex;
use talos_error::TalosError;
pub(crate) use talos_sdk::{
    ActiveTurnState, AgentAssistantTextPhase, AgentMessageRole, AgentTurnRef, AgentUserContent,
    AgentUserInput, AuthSessionReadResponse, AuthSessionSnapshot, AuthSessionUserSnapshot,
    ContextCompactionOutcome, FileInfo, FileUploadRequest, FileUploadResponse, MessageQueue,
    MessageQueueChangedNotification, QueuedMessage, RecoverTimelineResponse,
    SessionHistoryResponse, SessionHistoryTurn, SessionInfo, SkillSuggestion, StartTurnRequest,
    SubmitTurnInput, SubmitTurnInputRequest, TimelineActiveTurn, TimelineCursorStatus,
    TimelineEvent, TimelineReplay, UserPermissionMode, UserRole,
};

mod events;
pub(crate) use events::*;

impl MockGatewayClient {
    pub(crate) fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(MockGatewayState {
                models: vec![
                    mock_model_info("qwen-plus", 128_000),
                    mock_model_info("gpt-5.3 medium", 128_000),
                ],
                default_model: talos_sdk::ModelRef {
                    provider: "mock".to_string(),
                    id: "qwen-plus".to_string(),
                },
                session_id: "sess-test".to_string(),
                current_model: "qwen-plus".to_string(),
                current_reasoning_effort: None,
                current_permission_mode: UserPermissionMode::FullAccess,
                submitted_turn_inputs: Vec::new(),
                current_project_root: None,
                auth_email: Some("talos@example.com".to_string()),
                updated_model: None,
                stream_counter: 0,
                list_models_failures_remaining: 0,
                connect_delay_ms: 0,
                stream_failures_remaining: 0,
                close_session_delay_ms: 0,
                close_session_failures_remaining: 0,
                shutdown_calls: 0,
                cancelled_sessions: Vec::new(),
                active_turn_id: None,
                session_usage: talos_types::talos::TokenUsage::default(),
                session_context_usage: None,
                compact_calls: 0,
                compact_failures_remaining: 0,
                clear_calls: 0,
                reconnect_calls: 0,
                reconnect_since_event_ids: Vec::new(),
                reconnect_events: Vec::new(),
                session_subscription_calls: 0,
                session_subscription_since_event_ids: Vec::new(),
                session_subscription_events: Vec::new(),
                timeline_high_water_mark_event_id: None,
                sent_requests: Vec::new(),
                upload_requests: Vec::new(),
                upload_failure_calls: Vec::new(),
                message_queue: MessageQueue {
                    revision: 0,
                    max_messages: 10,
                    messages: Vec::new(),
                },
                session_history_turns: Vec::new(),
                next_queued_message_id: 0,
                message_queue_subscribers: Vec::new(),
            })),
        }
    }

    pub(crate) fn with_connect_delay(connect_delay_ms: u64) -> Self {
        let client = Self::new();
        client.state.lock().unwrap().connect_delay_ms = connect_delay_ms;
        client
    }

    pub(crate) fn with_list_models_failures(list_models_failures_remaining: u8) -> Self {
        let client = Self::new();
        client.state.lock().unwrap().list_models_failures_remaining =
            list_models_failures_remaining;
        client
    }

    pub(crate) fn with_stream_failures(stream_failures_remaining: u8) -> Self {
        let client = Self::new();
        client.state.lock().unwrap().stream_failures_remaining = stream_failures_remaining;
        client
    }

    pub(crate) fn with_active_reconnect_events(events: Vec<TimelineEvent>) -> Self {
        let client = Self::new();
        client.state.lock().unwrap().reconnect_events = events;
        client
    }

    pub(crate) fn with_close_delay(close_session_delay_ms: u64) -> Self {
        let client = Self::new();
        client.state.lock().unwrap().close_session_delay_ms = close_session_delay_ms;
        client
    }
}

#[async_trait]
impl GatewayClient for MockGatewayClient {
    async fn list_models(&self) -> TalosResult<talos_sdk::ModelListResponse> {
        let mut state = self.state.lock().unwrap();
        if state.list_models_failures_remaining > 0 {
            state.list_models_failures_remaining -= 1;
            return Err(TalosError::internal("mock list_models failure"));
        }
        for model in &mut state.models {
            model.selection_capabilities = model.model.selection_capabilities();
        }
        let default_model = state
            .models
            .iter()
            .find(|model| {
                model.provider == state.default_model.provider
                    && model.model.id == state.default_model.id
            })
            .expect("mock default model should exist");
        Ok(talos_sdk::ModelListResponse {
            models: state.models.clone(),
            default_model_selection: talos_sdk::ModelSelection::from_model(
                talos_sdk::ModelRef {
                    provider: default_model.provider.clone(),
                    id: default_model.model.id.clone(),
                },
                &default_model.model,
            ),
        })
    }

    async fn read_auth_session(&self) -> TalosResult<AuthSessionReadResponse> {
        let state = self.state.lock().unwrap();
        Ok(AuthSessionReadResponse {
            session: Some(AuthSessionSnapshot {
                schema_version: 1,
                cloud_api_base_url: "https://cloud.example.test".to_string(),
                access_token: "test-token".to_string(),
                token_type: "Bearer".to_string(),
                expires_at_ms: None,
                last_used_at_ms: "1".to_string(),
                user: AuthSessionUserSnapshot {
                    user_id: "test-user".to_string(),
                    username: "talos-user".to_string(),
                    email: state.auth_email.clone(),
                    role: UserRole::User,
                    r#type: Default::default(),
                },
            }),
        })
    }

    async fn suggest_skills(&self, _query: &str) -> TalosResult<Vec<SkillSuggestion>> {
        Ok(Vec::new())
    }

    async fn upload_file(
        &self,
        request: FileUploadRequest,
        data: Bytes,
    ) -> TalosResult<FileUploadResponse> {
        let mut state = self.state.lock().unwrap();
        state.upload_requests.push((request.clone(), data.clone()));
        let call = state.upload_requests.len();
        if state.upload_failure_calls.contains(&call) {
            return Err(TalosError::internal(
                "mock upload failure on call; technical details were withheld.",
            )
            .context(format!("mock upload failure on call {call}")));
        }
        Ok(FileUploadResponse {
            file: FileInfo {
                file_id: format!("uploaded-file-{call}"),
                scope: request.scope,
                original_filename: request.filename,
                mime_type: "image/png".to_string(),
                size_bytes: data.len() as u64,
                tags: request.tags,
                metadata: request.metadata,
                created_at: call as i64,
                updated_at: call as i64,
            },
        })
    }

    async fn list_sessions(&self) -> TalosResult<Vec<SessionInfo>> {
        Ok(vec![])
    }

    async fn get_session(&self, _session_id: &str) -> TalosResult<SessionInfo> {
        let state = self.state.lock().unwrap();
        Ok(state.session_info(3))
    }

    async fn close_session(&self, _session_id: &str) -> TalosResult<()> {
        let (delay, should_fail) = {
            let mut state = self.state.lock().unwrap();
            let should_fail = if state.close_session_failures_remaining > 0 {
                state.close_session_failures_remaining -= 1;
                true
            } else {
                false
            };
            (state.close_session_delay_ms, should_fail)
        };
        if delay > 0 {
            tokio::time::sleep(Duration::from_millis(delay)).await;
        }
        if should_fail {
            return Err(TalosError::internal("mock close session failure"));
        }
        Ok(())
    }

    async fn cancel_active_turn(&self, session_id: &str, _turn_id: &str) -> TalosResult<()> {
        self.state
            .lock()
            .unwrap()
            .cancelled_sessions
            .push(session_id.to_string());
        Ok(())
    }

    async fn add_queued_message(
        &self,
        request: talos_sdk::MessageQueueAddRequest,
    ) -> TalosResult<MessageQueue> {
        let mut state = self.state.lock().unwrap();
        if state.message_queue.messages.len() >= state.message_queue.max_messages as usize {
            return Err(TalosError::new(
                talos_error::ErrorCode::MessageQueueFull,
                "Message queue is full".to_string(),
            ));
        }
        state.next_queued_message_id = state.next_queued_message_id.saturating_add(1);
        let id = format!("q{}", state.next_queued_message_id);
        state.message_queue.messages.push(QueuedMessage {
            id,
            input: request.input,
        });
        state.message_queue.revision = state.message_queue.revision.saturating_add(1);
        state.publish_message_queue_changed();
        Ok(state.message_queue.clone())
    }

    async fn list_message_queue(
        &self,
        _request: talos_sdk::MessageQueueListRequest,
    ) -> TalosResult<MessageQueue> {
        Ok(self.state.lock().unwrap().message_queue.clone())
    }

    async fn update_queued_message(
        &self,
        request: talos_sdk::MessageQueueUpdateRequest,
    ) -> TalosResult<MessageQueue> {
        let mut state = self.state.lock().unwrap();
        let Some(message) = state
            .message_queue
            .messages
            .iter_mut()
            .find(|message| message.id == request.message_id)
        else {
            return Err(TalosError::new(
                talos_error::ErrorCode::ResourceNotFound,
                "Queued message not found".to_string(),
            ));
        };
        message.input = request.input;
        state.message_queue.revision = state.message_queue.revision.saturating_add(1);
        state.publish_message_queue_changed();
        Ok(state.message_queue.clone())
    }

    async fn delete_queued_message(
        &self,
        request: talos_sdk::MessageQueueDeleteRequest,
    ) -> TalosResult<MessageQueue> {
        let mut state = self.state.lock().unwrap();
        let before = state.message_queue.messages.len();
        state
            .message_queue
            .messages
            .retain(|message| message.id != request.message_id);
        if state.message_queue.messages.len() == before {
            return Err(TalosError::new(
                talos_error::ErrorCode::ResourceNotFound,
                "Queued message not found".to_string(),
            ));
        }
        state.message_queue.revision = state.message_queue.revision.saturating_add(1);
        state.publish_message_queue_changed();
        Ok(state.message_queue.clone())
    }

    async fn reorder_message_queue(
        &self,
        request: talos_sdk::MessageQueueReorderRequest,
    ) -> TalosResult<MessageQueue> {
        let mut state = self.state.lock().unwrap();
        let mut reordered = Vec::with_capacity(request.message_ids.len());
        for message_id in request.message_ids {
            let Some(message) = state
                .message_queue
                .messages
                .iter()
                .find(|message| message.id == message_id)
                .cloned()
            else {
                return Err(TalosError::new(
                    talos_error::ErrorCode::Conflict,
                    "Queue order is stale".to_string(),
                ));
            };
            reordered.push(message);
        }
        if reordered.len() != state.message_queue.messages.len() {
            return Err(TalosError::new(
                talos_error::ErrorCode::Conflict,
                "Queue order is incomplete".to_string(),
            ));
        }
        state.message_queue.messages = reordered;
        state.message_queue.revision = state.message_queue.revision.saturating_add(1);
        state.publish_message_queue_changed();
        Ok(state.message_queue.clone())
    }

    async fn start_queued_message(
        &self,
        request: talos_sdk::MessageQueueStartRequest,
    ) -> TalosResult<talos_sdk::StartTurnResponse> {
        let mut state = self.state.lock().unwrap();
        let index = request
            .message_id
            .as_ref()
            .and_then(|message_id| {
                state
                    .message_queue
                    .messages
                    .iter()
                    .position(|message| &message.id == message_id)
            })
            .unwrap_or_default();
        if index >= state.message_queue.messages.len() {
            return Err(TalosError::new(
                talos_error::ErrorCode::ResourceNotFound,
                "Queued message not found".to_string(),
            ));
        }
        state.message_queue.messages.remove(index);
        state.message_queue.revision = state.message_queue.revision.saturating_add(1);
        state.active_turn_id = Some("turn-queued".to_string());
        state.publish_message_queue_changed();
        Ok(talos_sdk::StartTurnResponse {
            request_id: "request-queued".to_string(),
            message_id: Some("message-queued".to_string()),
            session_id: request.session_id,
            turn_id: "turn-queued".to_string(),
            status: talos_sdk::TurnCommandStatus::Accepted,
        })
    }

    async fn submit_turn_input(
        &self,
        request: SubmitTurnInputRequest,
    ) -> TalosResult<talos_sdk::SubmitTurnInputResponse> {
        let message_id = match &request.input {
            SubmitTurnInput::QueuedMessage { message_id } => {
                self.delete_queued_message(talos_sdk::MessageQueueDeleteRequest {
                    session_id: request.identity.session_id.clone(),
                    message_id: message_id.clone(),
                })
                .await?;
                message_id.clone()
            }
            SubmitTurnInput::UserActionResponse { action_id, .. } => {
                format!("msg_action_{action_id}")
            }
            SubmitTurnInput::Message { .. } => "msg-input".to_string(),
        };
        self.state
            .lock()
            .unwrap()
            .submitted_turn_inputs
            .push(request.clone());
        Ok(talos_sdk::SubmitTurnInputResponse {
            request_id: request
                .client_request_id
                .unwrap_or_else(|| "request-input".to_string()),
            identity: request.identity,
            message_id,
        })
    }

    async fn subscribe_session_events(
        &self,
        _session_id: &str,
    ) -> TalosResult<crate::client::SessionSubscription> {
        let (sender, receiver) = mpsc::channel(32);
        self.state
            .lock()
            .unwrap()
            .message_queue_subscribers
            .push(sender);
        Ok(crate::client::SessionSubscription::new(receiver, None))
    }

    async fn update_session_model_selection(
        &self,
        _session_id: &str,
        model_selection: &talos_sdk::ModelSelection,
    ) -> TalosResult<SessionInfo> {
        let mut state = self.state.lock().unwrap();
        state.current_model = model_selection.model_ref.id.clone();
        state.current_reasoning_effort = model_selection.reasoning_effort;
        state.updated_model = Some(model_selection.model_ref.id.clone());
        Ok(state.session_info(3))
    }

    async fn update_session_permission_mode(
        &self,
        _session_id: &str,
        mode: UserPermissionMode,
    ) -> TalosResult<SessionInfo> {
        let mut state = self.state.lock().unwrap();
        state.current_permission_mode = mode;
        Ok(state.session_info(3))
    }

    async fn compact_session(&self, _session_id: &str) -> TalosResult<SessionInfo> {
        let mut state = self.state.lock().unwrap();
        state.compact_calls = state.compact_calls.saturating_add(1);
        if state.compact_failures_remaining > 0 {
            state.compact_failures_remaining -= 1;
            return Err(TalosError::internal("mock session compaction failure"));
        }
        state.session_usage.total_tokens = state.session_usage.total_tokens.saturating_add(42);
        Ok(state.session_info(3))
    }

    async fn subscribe_session_timeline(
        &self,
        _session_id: &str,
        since_event_id: Option<String>,
    ) -> TalosResult<mpsc::Receiver<TimelineEvent>> {
        let events = {
            let mut state = self.state.lock().unwrap();
            state.session_subscription_calls = state.session_subscription_calls.saturating_add(1);
            state
                .session_subscription_since_event_ids
                .push(since_event_id);
            std::mem::take(&mut state.session_subscription_events)
        };
        let (tx, rx) = mpsc::channel(32);
        for event in events {
            tx.send(event).await.unwrap();
        }
        Ok(rx)
    }

    async fn clear_session(&self, _session_id: &str) -> TalosResult<SessionInfo> {
        let mut state = self.state.lock().unwrap();
        state.clear_calls = state.clear_calls.saturating_add(1);
        state.session_usage = talos_types::talos::TokenUsage::default();
        Ok(state.session_info(0))
    }

    async fn start_turn_stream(
        &self,
        request: StartTurnRequest,
    ) -> TalosResult<mpsc::Receiver<TimelineEvent>> {
        let (delay, should_fail) = {
            let mut state = self.state.lock().unwrap();
            state.sent_requests.push(request.clone());
            let should_fail = if state.stream_failures_remaining > 0 {
                state.stream_failures_remaining -= 1;
                true
            } else {
                false
            };
            (state.connect_delay_ms, should_fail)
        };
        if delay > 0 {
            tokio::time::sleep(Duration::from_millis(delay)).await;
        }
        if should_fail {
            return Err(TalosError::internal("mock stream connect failure"));
        }

        let (tx, rx) = mpsc::channel(32);
        let prompt = request
            .input
            .content
            .iter()
            .find_map(|item| match item {
                AgentUserContent::Text { text } => Some(text.clone()),
                _ => None,
            })
            .unwrap_or_else(|| "unknown".to_string());
        let session_id = request.session_id.clone();

        let message_id = {
            let mut state = self.state.lock().unwrap();
            state.stream_counter += 1;
            format!("msg-{}", state.stream_counter)
        };

        tx.send(turn_started_event(session_id.clone(), 0))
            .await
            .unwrap();
        tx.send(assistant_block_started_event(
            message_id.clone(),
            session_id.to_string(),
            1,
        ))
        .await
        .unwrap();

        tx.send(assistant_text_delta_event(
            message_id.clone(),
            session_id.to_string(),
            2,
            "# Title\n",
        ))
        .await
        .unwrap();

        tx.send(assistant_text_delta_event(
            message_id.clone(),
            session_id.to_string(),
            3,
            "- item\n",
        ))
        .await
        .unwrap();

        tx.send(assistant_text_delta_event(
            message_id.clone(),
            session_id.to_string(),
            4,
            format!("reply({})", prompt),
        ))
        .await
        .unwrap();

        let final_text = format!("# Title\n- item\nreply({prompt})");
        tx.send(assistant_block_completed_event(
            message_id.clone(),
            session_id.to_string(),
            5,
            final_text,
        ))
        .await
        .unwrap();

        tx.send(turn_completed_event(session_id.to_string(), 6))
            .await
            .unwrap();

        drop(tx);
        Ok(rx)
    }

    async fn reconnect_active_turn_stream(
        &self,
        _session_id: &str,
        since_event_id: Option<String>,
    ) -> TalosResult<mpsc::Receiver<TimelineEvent>> {
        let events = {
            let mut state = self.state.lock().unwrap();
            state.reconnect_calls = state.reconnect_calls.saturating_add(1);
            state.reconnect_since_event_ids.push(since_event_id.clone());
            state.reconnect_events.clone()
        };
        if events.is_empty() {
            return Err(TalosError::new(
                talos_error::ErrorCode::Conflict,
                "No active turn stream is available for this session.".to_string(),
            ));
        }
        let (tx, rx) = mpsc::channel(32);
        for event in events {
            tx.send(event).await.unwrap();
        }
        drop(tx);
        Ok(rx)
    }

    async fn get_session_history(&self, _session_id: &str) -> TalosResult<SessionHistoryResponse> {
        let data = self.state.lock().unwrap().session_history_turns.clone();
        Ok(SessionHistoryResponse {
            data,
            next_cursor: None,
            last_stable_event_id: None,
        })
    }

    async fn get_active_turn(&self, session_id: &str) -> TalosResult<Option<TimelineActiveTurn>> {
        let active_turn_id = self.state.lock().unwrap().active_turn_id.clone();
        Ok(active_turn_id.map(|turn_id| TimelineActiveTurn {
            identity: AgentTurnRef::new(session_id, turn_id),
            initial_message_id: None,
            state: ActiveTurnState::Running,
            last_event_id: "ev_0".to_string(),
            updated_at: 0,
        }))
    }

    async fn recover_timeline(
        &self,
        _session_id: &str,
        _after_event_id: Option<String>,
    ) -> TalosResult<RecoverTimelineResponse> {
        let high_water_mark_event_id = self
            .state
            .lock()
            .unwrap()
            .timeline_high_water_mark_event_id
            .clone();
        Ok(RecoverTimelineResponse {
            replay: TimelineReplay {
                events: Vec::new(),
                high_water_mark_event_id,
                next_cursor: None,
                last_event_seq: 0,
                cursor_status: TimelineCursorStatus::FromBeginning,
            },
            active_turn: None,
        })
    }

    async fn shutdown(&self) -> TalosResult<()> {
        let mut state = self.state.lock().unwrap();
        state.shutdown_calls = state.shutdown_calls.saturating_add(1);
        Ok(())
    }
}

pub(crate) fn line_plain_text(line: &ratatui::text::Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>()
}
