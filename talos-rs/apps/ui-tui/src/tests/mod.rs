    use super::*;
    use crate::markdown::render_markdown_lines;
    use crate::utils::char_display_width;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    use ratatui::style::{Color, Modifier};
    
    use async_trait::async_trait;
    use dotenv::dotenv;
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};
    use talos_client::{
        ClientConfig, SessionCreateParams, StopReason, TalosClient, TalosContentItem,
        TalosEventPayload, TalosInputItem, TalosMessageCompletedEvent, TalosMessageOutputDeltaEvent,
        TalosMessageReasoningDeltaEvent, TalosMessageRequest, TalosMessageResponse,
        TalosMessageStartedEvent, TalosOutputItem, TalosThinkingSource, TalosThinkingStage,
    };
    use talos_error::TalosError;
    use talos_types::talos::TalosEventType;
    use talos_config::Config;
    use talos_server::{start_server, TalosServerOptions};

    #[derive(Default)]
    struct MockGatewayState {
        models: Vec<ModelInfo>,
        session_id: String,
        current_model: String,
        updated_model: Option<String>,
        stream_counter: u64,
        connect_delay_ms: u64,
        create_session_delay_ms: u64,
        stream_failures_remaining: u8,
        close_session_delay_ms: u64,
        cancelled_sessions: Vec<String>,
    }

    struct MockGatewayClient {
        state: Arc<Mutex<MockGatewayState>>,
    }

    fn mock_model_info(id: &str, context_window: u32) -> ModelInfo {
        ModelInfo {
            id: id.to_string(),
            name: id.to_string(),
            provider: "mock".to_string(),
            context_window,
            max_tokens: 8_192,
            reasoning: false,
            input: vec![talos_types::ModelInputKind::Text],
            cost: talos_types::ModelCostConfig::default(),
            compat: talos_types::ModelCompatConfig::default(),
        }
    }

    fn started_event(message_id: impl Into<String>, sequence: u64) -> TalosEvent {
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

    fn output_delta_event(
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

    fn reasoning_delta_event(
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

    fn completed_event(
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
        fn new() -> Self {
            Self {
                state: Arc::new(Mutex::new(MockGatewayState {
                    models: vec![
                        mock_model_info("qwen-plus", 128_000),
                        mock_model_info("gpt-5.3 medium", 128_000),
                    ],
                    session_id: "sess-test".to_string(),
                    current_model: "qwen-plus".to_string(),
                    updated_model: None,
                    stream_counter: 0,
                    connect_delay_ms: 0,
                    create_session_delay_ms: 0,
                    stream_failures_remaining: 0,
                    close_session_delay_ms: 0,
                    cancelled_sessions: Vec::new(),
                })),
            }
        }

        fn with_connect_delay(connect_delay_ms: u64) -> Self {
            let client = Self::new();
            client.state.lock().unwrap().connect_delay_ms = connect_delay_ms;
            client
        }

        fn with_session_delay(create_session_delay_ms: u64) -> Self {
            let client = Self::new();
            client.state.lock().unwrap().create_session_delay_ms = create_session_delay_ms;
            client
        }

        fn with_stream_failures(stream_failures_remaining: u8) -> Self {
            let client = Self::new();
            client.state.lock().unwrap().stream_failures_remaining = stream_failures_remaining;
            client
        }

        fn with_close_delay(close_session_delay_ms: u64) -> Self {
            let client = Self::new();
            client.state.lock().unwrap().close_session_delay_ms = close_session_delay_ms;
            client
        }
    }

    #[async_trait]
    impl GatewayClient for MockGatewayClient {
        async fn list_models(&self) -> TalosResult<Vec<ModelInfo>> {
            Ok(self.state.lock().unwrap().models.clone())
        }

        async fn list_sessions(&self) -> TalosResult<Vec<TalosSession>> {
            Ok(vec![])
        }

        async fn create_session(&self, _params: &SessionCreateParams) -> TalosResult<TalosSession> {
            let delay = self.state.lock().unwrap().create_session_delay_ms;
            if delay > 0 {
                tokio::time::sleep(Duration::from_millis(delay)).await;
            }
            let state = self.state.lock().unwrap();
            Ok(TalosSession {
                title: "Untitled Session".to_string(),
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
                    environment: None,
                },
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
                    environment: None,
                },
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
                    usage: talos_types::talos::TalosUsage::default(),
                },
            })
        }

        async fn close_session(&self, _session_id: &str) -> TalosResult<()> {
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
                    environment: None,
                },
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
                    usage: talos_types::talos::TalosUsage::default(),
                },
            })
        }

        async fn send_message_stream(
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

        async fn get_session_turns(
            &self,
            _session_id: &str,
        ) -> TalosResult<Vec<TalosMessageResponse>> {
            Ok(vec![])
        }
    }

    fn create_embedded_app() -> TuiApp {
        let client: Arc<dyn GatewayClient> = Arc::new(MockGatewayClient::new());
        TuiApp::new_with_gateway_client(client, TalosConnectionMode::Embedded)
    }

    fn create_remote_app() -> TuiApp {
        let client: Arc<dyn GatewayClient> = Arc::new(MockGatewayClient::new());
        TuiApp::new_with_gateway_client(client, TalosConnectionMode::Remote)
    }

    fn create_remote_app_with_connect_delay(delay_ms: u64) -> TuiApp {
        let client: Arc<dyn GatewayClient> =
            Arc::new(MockGatewayClient::with_connect_delay(delay_ms));
        TuiApp::new_with_gateway_client(client, TalosConnectionMode::Remote)
    }

    fn create_remote_app_with_session_delay(delay_ms: u64) -> TuiApp {
        let client: Arc<dyn GatewayClient> =
            Arc::new(MockGatewayClient::with_session_delay(delay_ms));
        TuiApp::new_with_gateway_client(client, TalosConnectionMode::Remote)
    }

    fn create_remote_app_with_stream_failures(failures: u8) -> TuiApp {
        let client: Arc<dyn GatewayClient> =
            Arc::new(MockGatewayClient::with_stream_failures(failures));
        TuiApp::new_with_gateway_client(client, TalosConnectionMode::Remote)
    }

    fn create_remote_app_with_close_delay(delay_ms: u64) -> TuiApp {
        let client: Arc<dyn GatewayClient> =
            Arc::new(MockGatewayClient::with_close_delay(delay_ms));
        TuiApp::new_with_gateway_client(client, TalosConnectionMode::Remote)
    }

    fn real_llm_env_available() -> bool {
        [
            "BAILIAN_API_KEY",
            "BAILIAN_CODINGPLAN_API_KEY",
            "OPENAI_API_KEY",
            "ANTHROPIC_API_KEY",
        ]
        .iter()
        .any(|key| {
            std::env::var(key)
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false)
        })
    }

    fn unique_test_dir(name: &str) -> PathBuf {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_millis();
        let dir = std::env::temp_dir().join(format!("talos-ui-tui-{name}-{timestamp}"));
        std::fs::create_dir_all(&dir).expect("test directory should be created");
        dir
    }

    async fn find_free_port() -> u16 {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("port probe should bind");
        listener
            .local_addr()
            .expect("local addr should exist")
            .port()
    }

    fn set_test_model_info(app: &mut TuiApp, model_id: &str, context_window: u32) {
        app.available_model_info = vec![mock_model_info(model_id, context_window)];
        app.available_models = vec![model_id.to_string()];
    }

    fn line_plain_text(line: &ratatui::text::Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>()
    }

    fn line_display_width(line: &ratatui::text::Line<'_>) -> usize {
        line.spans
            .iter()
            .flat_map(|span| span.content.chars())
            .map(char_display_width)
            .sum()
    }

    mod input_stream_tests;
    mod scroll_render_tests;
    mod session_tool_tests;
    mod markdown_layout_tests;
