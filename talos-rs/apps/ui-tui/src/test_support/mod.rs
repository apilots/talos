pub(crate) use super::*;
pub(crate) use crate::markdown::render_markdown_lines;
use crate::utils::char_display_width;
use async_trait::async_trait;
pub(crate) use crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
pub(crate) use ratatui::style::{Color, Modifier};
pub(crate) use serde_json::json;
pub(crate) use std::path::PathBuf;
use std::sync::Mutex;
pub(crate) use talos_client::{
    SessionCreateParams, StopReason, TalosContentItem, TalosEventPayload, TalosInputItem,
    TalosMessageCompletedEvent, TalosMessageOutputDeltaEvent, TalosMessageReasoningDeltaEvent,
    TalosMessageRequest, TalosMessageResponse, TalosMessageStartedEvent, TalosOutputItem,
    TalosThinkingSource, TalosThinkingStage,
};
use talos_error::TalosError;
pub(crate) use talos_types::talos::TalosEventType;

#[derive(Default)]
pub(crate) struct MockGatewayState {
    pub(crate) models: Vec<ModelInfo>,
    pub(crate) session_id: String,
    pub(crate) current_model: String,
    pub(crate) default_project_root: Option<PathBuf>,
    pub(crate) current_project_root: Option<PathBuf>,
    pub(crate) updated_model: Option<String>,
    pub(crate) stream_counter: u64,
    pub(crate) list_models_failures_remaining: u8,
    pub(crate) connect_delay_ms: u64,
    pub(crate) create_session_failures_remaining: u8,
    pub(crate) create_session_delay_ms: u64,
    pub(crate) stream_failures_remaining: u8,
    pub(crate) close_session_delay_ms: u64,
    pub(crate) close_session_failures_remaining: u8,
    pub(crate) shutdown_calls: usize,
    pub(crate) cancelled_sessions: Vec<String>,
    pub(crate) session_usage: talos_types::talos::TalosUsage,
    pub(crate) compact_calls: usize,
    pub(crate) clear_calls: usize,
}

pub(crate) struct MockGatewayClient {
    pub(crate) state: Arc<Mutex<MockGatewayState>>,
}

pub(crate) fn mock_model_info(id: &str, context_window: u32) -> ModelInfo {
    ModelInfo {
        id: id.to_string(),
        name: id.to_string(),
        provider: "mock".to_string(),
        context_window,
        reasoning: false,
        input: vec![talos_types::ModelInputKind::Text],
        cost: talos_types::ModelCostConfig::default(),
        compat: talos_types::ModelCompatConfig::default(),
    }
}

pub(crate) fn started_event(message_id: impl Into<String>, sequence: u64) -> TalosEvent {
    TalosEvent {
        event_type: TalosEventType::MessageStarted,
        message_id: message_id.into(),
        session_id: "sess-test".to_string(),
        sequence,
        payload: TalosEventPayload::MessageStarted(TalosMessageStartedEvent {
            request_id: "req-test".to_string(),
            model: "qwen-plus".to_string(),
            usage: TalosUsage::default(),
            iteration: 0,
            step_id: "start".to_string(),
        }),
    }
}

pub(crate) fn output_delta_event(
    message_id: impl Into<String>,
    session_id: impl Into<String>,
    sequence: u64,
    delta: impl Into<String>,
) -> TalosEvent {
    TalosEvent {
        event_type: TalosEventType::MessageOutputDelta,
        message_id: message_id.into(),
        session_id: session_id.into(),
        sequence,
        payload: TalosEventPayload::MessageOutputDelta(TalosMessageOutputDeltaEvent {
            request_id: "req-test".to_string(),
            delta: delta.into(),
            item_index: sequence,
            iteration: sequence,
            step_id: format!("step-{}", sequence),
        }),
    }
}

pub(crate) fn reasoning_delta_event(
    message_id: impl Into<String>,
    session_id: impl Into<String>,
    sequence: u64,
    delta: impl Into<String>,
) -> TalosEvent {
    TalosEvent {
        event_type: TalosEventType::MessageReasoningDelta,
        message_id: message_id.into(),
        session_id: session_id.into(),
        sequence,
        payload: TalosEventPayload::MessageReasoningDelta(TalosMessageReasoningDeltaEvent {
            request_id: "req-test".to_string(),
            delta: delta.into(),
            source: TalosThinkingSource::Provider,
            stage: TalosThinkingStage::ModelReasoning,
            item_index: sequence,
            iteration: sequence,
            step_id: format!("step-{}", sequence),
        }),
    }
}

pub(crate) fn completed_event(
    message_id: impl Into<String>,
    session_id: impl Into<String>,
    sequence: u64,
    usage: TalosUsage,
) -> TalosEvent {
    TalosEvent {
        event_type: TalosEventType::MessageCompleted,
        message_id: message_id.into(),
        session_id: session_id.into(),
        sequence,
        payload: TalosEventPayload::MessageCompleted(TalosMessageCompletedEvent {
            request_id: "req-test".to_string(),
            stop_reason: StopReason::EndTurn,
            usage,
            total_output_items: sequence.max(1),
            iteration: sequence,
            step_id: "completed".to_string(),
            task_count: 1,
        }),
    }
}

impl MockGatewayClient {
    pub(crate) fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(MockGatewayState {
                models: vec![
                    mock_model_info("qwen-plus", 128_000),
                    mock_model_info("gpt-5.3 medium", 128_000),
                ],
                session_id: "sess-test".to_string(),
                current_model: "qwen-plus".to_string(),
                default_project_root: Some(PathBuf::from("/mock/workspace")),
                current_project_root: None,
                updated_model: None,
                stream_counter: 0,
                list_models_failures_remaining: 0,
                connect_delay_ms: 0,
                create_session_failures_remaining: 0,
                create_session_delay_ms: 0,
                stream_failures_remaining: 0,
                close_session_delay_ms: 0,
                close_session_failures_remaining: 0,
                shutdown_calls: 0,
                cancelled_sessions: Vec::new(),
                session_usage: talos_types::talos::TalosUsage::default(),
                compact_calls: 0,
                clear_calls: 0,
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

    pub(crate) fn with_session_delay(create_session_delay_ms: u64) -> Self {
        let client = Self::new();
        client.state.lock().unwrap().create_session_delay_ms = create_session_delay_ms;
        client
    }

    pub(crate) fn with_create_session_failures(create_session_failures_remaining: u8) -> Self {
        let client = Self::new();
        client
            .state
            .lock()
            .unwrap()
            .create_session_failures_remaining = create_session_failures_remaining;
        client
    }

    pub(crate) fn with_stream_failures(stream_failures_remaining: u8) -> Self {
        let client = Self::new();
        client.state.lock().unwrap().stream_failures_remaining = stream_failures_remaining;
        client
    }

    pub(crate) fn with_close_delay(close_session_delay_ms: u64) -> Self {
        let client = Self::new();
        client.state.lock().unwrap().close_session_delay_ms = close_session_delay_ms;
        client
    }

    pub(crate) fn with_close_failures(close_session_failures_remaining: u8) -> Self {
        let client = Self::new();
        client
            .state
            .lock()
            .unwrap()
            .close_session_failures_remaining = close_session_failures_remaining;
        client
    }
}

#[async_trait]
impl GatewayClient for MockGatewayClient {
    async fn list_models(&self) -> TalosResult<Vec<ModelInfo>> {
        let mut state = self.state.lock().unwrap();
        if state.list_models_failures_remaining > 0 {
            state.list_models_failures_remaining -= 1;
            return Err(TalosError::ClientError(
                "mock list_models failure".to_string(),
            ));
        }
        Ok(state.models.clone())
    }

    async fn list_sessions(&self) -> TalosResult<Vec<TalosSession>> {
        Ok(vec![])
    }

    async fn create_session(&self, params: &SessionCreateParams) -> TalosResult<TalosSession> {
        let (delay, should_fail) = {
            let mut state = self.state.lock().unwrap();
            let should_fail = if state.create_session_failures_remaining > 0 {
                state.create_session_failures_remaining -= 1;
                true
            } else {
                false
            };
            if let Some(current_directory) = &params.current_directory {
                state.current_project_root = Some(current_directory.clone());
            } else if params.project_id.is_some() {
                state.current_project_root = state.default_project_root.clone();
            } else if state.current_project_root.is_none() {
                state.current_project_root = state.default_project_root.clone();
            }
            (state.create_session_delay_ms, should_fail)
        };
        if delay > 0 {
            tokio::time::sleep(Duration::from_millis(delay)).await;
        }
        if should_fail {
            return Err(TalosError::ClientError(
                "mock create_session failure".to_string(),
            ));
        }
        let state = self.state.lock().unwrap();
        Ok(TalosSession {
            title: "Untitled Session".to_string(),
            user_id: "test-user".to_string(),
            context: talos_types::SessionContext {
                session_id: state.session_id.clone(),
                workspace_id: None,
                source_type: talos_types::talos::TalosSourceType::Http,
                provider: talos_types::talos::TalosProvider::Sdk,
                user_id: "test-user".to_string(),
                message_id: "message-1".to_string(),
                trace_id: "trace-1".to_string(),
                channel_id: Some("terminal".to_string()),
                thread_id: None,
                project_id: None,
                current_directory: state.current_project_root.clone(),
                project_root: None,
                environment: None,
            },
            conf: talos_client::SessionConf::default(),
            project_id: None,
            current_directory: state.current_project_root.clone(),
            config: {
                let mut config = talos_types::TalosConfig::default();
                config.agent.model = state.current_model.clone();
                config.llm.model = state.current_model.clone();
                config
            },
            state: talos_types::SessionState {
                created_at: 1,
                last_activity: 1,
                message_count: 0,
                usage: talos_types::talos::TalosUsage::default(),
            },
        })
    }

    async fn get_session(&self, _session_id: &str) -> TalosResult<TalosSession> {
        let state = self.state.lock().unwrap();
        Ok(TalosSession {
            title: "Untitled Session".to_string(),
            user_id: "test-user".to_string(),
            context: talos_types::SessionContext {
                session_id: state.session_id.clone(),
                workspace_id: None,
                source_type: talos_types::talos::TalosSourceType::Http,
                provider: talos_types::talos::TalosProvider::Sdk,
                user_id: "test-user".to_string(),
                message_id: "message-1".to_string(),
                trace_id: "trace-1".to_string(),
                channel_id: Some("terminal".to_string()),
                thread_id: None,
                project_id: None,
                current_directory: state.current_project_root.clone(),
                project_root: None,
                environment: None,
            },
            conf: talos_client::SessionConf::default(),
            project_id: None,
            current_directory: state.current_project_root.clone(),
            config: {
                let mut config = talos_types::TalosConfig::default();
                config.agent.model = state.current_model.clone();
                config.llm.model = state.current_model.clone();
                config
            },
            state: talos_types::SessionState {
                created_at: 1,
                last_activity: 2,
                message_count: 3,
                usage: state.session_usage.clone(),
            },
        })
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
            return Err(TalosError::ClientError(
                "mock close session failure".to_string(),
            ));
        }
        Ok(())
    }

    async fn delete_session(&self, _session_id: &str) -> TalosResult<()> {
        let delay = self.state.lock().unwrap().close_session_delay_ms;
        if delay > 0 {
            tokio::time::sleep(Duration::from_millis(delay)).await;
        }
        Ok(())
    }

    async fn cancel_active_turn(&self, session_id: &str) -> TalosResult<()> {
        self.state
            .lock()
            .unwrap()
            .cancelled_sessions
            .push(session_id.to_string());
        Ok(())
    }

    async fn update_session_model(
        &self,
        _session_id: &str,
        model: &str,
    ) -> TalosResult<TalosSession> {
        let mut state = self.state.lock().unwrap();
        state.current_model = model.to_string();
        state.updated_model = Some(model.to_string());
        Ok(TalosSession {
            title: "Untitled Session".to_string(),
            user_id: "test-user".to_string(),
            context: talos_types::SessionContext {
                session_id: state.session_id.clone(),
                workspace_id: None,
                source_type: talos_types::talos::TalosSourceType::Http,
                provider: talos_types::talos::TalosProvider::Sdk,
                user_id: "test-user".to_string(),
                message_id: "message-1".to_string(),
                trace_id: "trace-1".to_string(),
                channel_id: Some("terminal".to_string()),
                thread_id: None,
                project_id: None,
                current_directory: state.current_project_root.clone(),
                project_root: None,
                environment: None,
            },
            conf: talos_client::SessionConf::default(),
            project_id: None,
            current_directory: state.current_project_root.clone(),
            config: {
                let mut config = talos_types::TalosConfig::default();
                config.agent.model = state.current_model.clone();
                config.llm.model = state.current_model.clone();
                config
            },
            state: talos_types::SessionState {
                created_at: 1,
                last_activity: 2,
                message_count: 3,
                usage: state.session_usage.clone(),
            },
        })
    }

    async fn compact_session(&self, _session_id: &str) -> TalosResult<TalosSession> {
        let mut state = self.state.lock().unwrap();
        state.compact_calls = state.compact_calls.saturating_add(1);
        state.session_usage.total_tokens = state.session_usage.total_tokens.saturating_add(42);
        Ok(TalosSession {
            title: "Untitled Session".to_string(),
            user_id: "test-user".to_string(),
            context: talos_types::SessionContext {
                session_id: state.session_id.clone(),
                workspace_id: None,
                source_type: talos_types::talos::TalosSourceType::Http,
                provider: talos_types::talos::TalosProvider::Sdk,
                user_id: "test-user".to_string(),
                message_id: "message-1".to_string(),
                trace_id: "trace-1".to_string(),
                channel_id: Some("terminal".to_string()),
                thread_id: None,
                project_id: None,
                current_directory: state.current_project_root.clone(),
                project_root: None,
                environment: None,
            },
            conf: talos_client::SessionConf::default(),
            project_id: None,
            current_directory: state.current_project_root.clone(),
            config: {
                let mut config = talos_types::TalosConfig::default();
                config.agent.model = state.current_model.clone();
                config.llm.model = state.current_model.clone();
                config
            },
            state: talos_types::SessionState {
                created_at: 1,
                last_activity: 2,
                message_count: 3,
                usage: state.session_usage.clone(),
            },
        })
    }

    async fn clear_session(&self, _session_id: &str) -> TalosResult<TalosSession> {
        let mut state = self.state.lock().unwrap();
        state.clear_calls = state.clear_calls.saturating_add(1);
        state.session_usage = talos_types::talos::TalosUsage::default();
        Ok(TalosSession {
            title: "Untitled Session".to_string(),
            user_id: "test-user".to_string(),
            context: talos_types::SessionContext {
                session_id: state.session_id.clone(),
                workspace_id: None,
                source_type: talos_types::talos::TalosSourceType::Http,
                provider: talos_types::talos::TalosProvider::Sdk,
                user_id: "test-user".to_string(),
                message_id: "message-1".to_string(),
                trace_id: "trace-1".to_string(),
                channel_id: Some("terminal".to_string()),
                thread_id: None,
                project_id: None,
                current_directory: state.current_project_root.clone(),
                project_root: None,
                environment: None,
            },
            conf: talos_client::SessionConf::default(),
            project_id: None,
            current_directory: state.current_project_root.clone(),
            config: {
                let mut config = talos_types::TalosConfig::default();
                config.agent.model = state.current_model.clone();
                config.llm.model = state.current_model.clone();
                config
            },
            state: talos_types::SessionState {
                created_at: 1,
                last_activity: 2,
                message_count: 0,
                usage: state.session_usage.clone(),
            },
        })
    }

    async fn send_message_events(
        &self,
        session_id: &str,
        request: &TalosMessageRequest,
    ) -> TalosResult<mpsc::Receiver<TalosEvent>> {
        let (delay, should_fail) = {
            let mut state = self.state.lock().unwrap();
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
            return Err(TalosError::ClientError(
                "mock stream connect failure".to_string(),
            ));
        }

        let (tx, rx) = mpsc::channel(32);
        let prompt = request
            .input
            .iter()
            .find_map(|item| match item {
                TalosInputItem::Text { text } => Some(text.clone()),
                _ => None,
            })
            .unwrap_or_else(|| "unknown".to_string());

        let message_id = {
            let mut state = self.state.lock().unwrap();
            state.stream_counter += 1;
            format!("msg-{}", state.stream_counter)
        };

        tx.send(started_event(message_id.clone(), 0)).await.unwrap();

        tx.send(output_delta_event(
            message_id.clone(),
            session_id.to_string(),
            1,
            "# Title\n",
        ))
        .await
        .unwrap();

        tx.send(output_delta_event(
            message_id.clone(),
            session_id.to_string(),
            2,
            "- item\n",
        ))
        .await
        .unwrap();

        tx.send(output_delta_event(
            message_id.clone(),
            session_id.to_string(),
            3,
            format!("reply({})", prompt),
        ))
        .await
        .unwrap();

        tx.send(completed_event(
            message_id,
            session_id.to_string(),
            4,
            TalosUsage::default(),
        ))
        .await
        .unwrap();

        drop(tx);
        Ok(rx)
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

    async fn get_session_turns(&self, _session_id: &str) -> TalosResult<Vec<TalosMessageResponse>> {
        Ok(vec![])
    }

    async fn shutdown(&self) -> TalosResult<()> {
        let mut state = self.state.lock().unwrap();
        state.shutdown_calls = state.shutdown_calls.saturating_add(1);
        Ok(())
    }
}

pub(crate) fn create_embedded_app() -> TuiApp {
    let client: Arc<dyn GatewayClient> = Arc::new(MockGatewayClient::new());
    TuiApp::new_with_gateway_client(client, TalosConnectionMode::Embedded)
}

pub(crate) fn create_remote_app() -> TuiApp {
    let client: Arc<dyn GatewayClient> = Arc::new(MockGatewayClient::new());
    TuiApp::new_with_gateway_client(client, TalosConnectionMode::Remote)
}

pub(crate) fn create_remote_app_with_connect_delay(delay_ms: u64) -> TuiApp {
    let client: Arc<dyn GatewayClient> = Arc::new(MockGatewayClient::with_connect_delay(delay_ms));
    TuiApp::new_with_gateway_client(client, TalosConnectionMode::Remote)
}

pub(crate) fn create_remote_app_with_model_list_failures(failures: u8) -> TuiApp {
    let client: Arc<dyn GatewayClient> =
        Arc::new(MockGatewayClient::with_list_models_failures(failures));
    TuiApp::new_with_gateway_client(client, TalosConnectionMode::Remote)
}

pub(crate) fn create_remote_app_with_session_delay(delay_ms: u64) -> TuiApp {
    let client: Arc<dyn GatewayClient> = Arc::new(MockGatewayClient::with_session_delay(delay_ms));
    TuiApp::new_with_gateway_client(client, TalosConnectionMode::Remote)
}

pub(crate) fn create_remote_app_with_create_session_failures(failures: u8) -> TuiApp {
    let client: Arc<dyn GatewayClient> =
        Arc::new(MockGatewayClient::with_create_session_failures(failures));
    TuiApp::new_with_gateway_client(client, TalosConnectionMode::Remote)
}

pub(crate) fn create_remote_app_with_stream_failures(failures: u8) -> TuiApp {
    let client: Arc<dyn GatewayClient> =
        Arc::new(MockGatewayClient::with_stream_failures(failures));
    TuiApp::new_with_gateway_client(client, TalosConnectionMode::Remote)
}

pub(crate) fn create_remote_app_with_close_delay(delay_ms: u64) -> TuiApp {
    let client: Arc<dyn GatewayClient> = Arc::new(MockGatewayClient::with_close_delay(delay_ms));
    TuiApp::new_with_gateway_client(client, TalosConnectionMode::Remote)
}

pub(crate) fn set_test_model_info(app: &mut TuiApp, model_id: &str, context_window: u32) {
    app.available_model_info = vec![mock_model_info(model_id, context_window)];
    app.available_models = vec![model_id.to_string()];
}

pub(crate) fn line_plain_text(line: &ratatui::text::Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>()
}

pub(crate) fn line_display_width(line: &ratatui::text::Line<'_>) -> usize {
    line.spans
        .iter()
        .flat_map(|span| span.content.chars())
        .map(char_display_width)
        .sum()
}
