use super::*;

/// Adds test-only convenience operations without expanding the production TUI API.
pub(crate) trait TestTuiAppExt {
    fn send_user_message(
        &mut self,
        user_message: String,
    ) -> impl std::future::Future<Output = TalosResult<bool>> + Send;
}

impl TestTuiAppExt for TuiApp {
    async fn send_user_message(&mut self, user_message: String) -> TalosResult<bool> {
        self.send_user_message_with_skills(user_message, Vec::new(), Vec::new())
            .await
    }
}

pub(crate) struct MockGatewayState {
    pub(crate) models: Vec<ModelInfo>,
    pub(crate) default_model: talos_sdk::ModelRef,
    pub(crate) session_id: String,
    pub(crate) current_model: String,
    pub(crate) current_reasoning_effort: Option<talos_sdk::ReasoningEffort>,
    pub(crate) current_permission_mode: UserPermissionMode,
    pub(crate) submitted_user_actions: Vec<SubmitUserActionRequest>,
    pub(crate) current_project_root: Option<PathBuf>,
    pub(crate) auth_email: Option<String>,
    pub(crate) workspace_root: PathBuf,
    pub(crate) updated_model: Option<String>,
    pub(crate) stream_counter: u64,
    pub(crate) list_models_failures_remaining: u8,
    pub(crate) connect_delay_ms: u64,
    pub(crate) stream_failures_remaining: u8,
    pub(crate) close_session_delay_ms: u64,
    pub(crate) close_session_failures_remaining: u8,
    pub(crate) shutdown_calls: usize,
    pub(crate) cancelled_sessions: Vec<String>,
    pub(crate) active_turn_id: Option<String>,
    pub(crate) session_usage: talos_types::talos::TokenUsage,
    pub(crate) session_context_usage: Option<talos_sdk::ContextUsage>,
    pub(crate) compact_calls: usize,
    pub(crate) compact_failures_remaining: u8,
    pub(crate) clear_calls: usize,
    pub(crate) reconnect_calls: usize,
    pub(crate) reconnect_since_event_ids: Vec<Option<String>>,
    pub(crate) reconnect_events: Vec<TimelineEventEnvelope>,
    pub(crate) session_subscription_calls: usize,
    pub(crate) session_subscription_since_event_ids: Vec<Option<String>>,
    pub(crate) session_subscription_events: Vec<TimelineEventEnvelope>,
    pub(crate) timeline_high_water_mark_event_id: Option<String>,
    pub(crate) sent_requests: Vec<StartTurnRequest>,
    pub(crate) upload_requests: Vec<(FileUploadRequest, Bytes)>,
    pub(crate) upload_failure_calls: Vec<usize>,
    pub(crate) message_queue: MessageQueue,
    pub(crate) session_history_messages: Vec<SessionHistoryMessage>,
    pub(crate) next_queued_message_id: u64,
    pub(crate) message_queue_subscribers: Vec<mpsc::Sender<MessageQueueChangedNotification>>,
}

pub(crate) struct MockGatewayClient {
    pub(crate) state: Arc<Mutex<MockGatewayState>>,
}

pub(crate) fn mock_model_info(id: &str, context_window: u32) -> ModelInfo {
    ModelInfo::from_provider_model(
        "mock",
        talos_types::ProviderModelConfig {
            id: id.to_string(),
            name: id.to_string(),
            context_window,
            reasoning: false,
            reasoning_effort: Default::default(),
            speed: Default::default(),
            input: vec![talos_types::ModelInputKind::Text],
            cost: talos_types::ModelCostConfig::default(),
        },
    )
}

impl MockGatewayState {
    pub(super) fn session_info(&self, message_count: u64) -> SessionInfo {
        SessionInfo {
            id: self.session_id.clone(),
            title: "Untitled Session".to_string(),
            user_id: "test-user".to_string(),
            source_type: talos_types::talos::TalosSourceType::Http,
            provider: talos_types::talos::TalosProvider::Sdk,
            model_selection: talos_sdk::ModelSelection {
                model_ref: talos_sdk::ModelRef {
                    provider: "mock".to_string(),
                    id: self.current_model.clone(),
                },
                reasoning_effort: self.current_reasoning_effort,
            },
            configured_default_mode: UserPermissionMode::FullAccess,
            effective_mode: self.current_permission_mode,
            created_at: 1,
            last_activity: 2,
            message_count,
            usage: talos_sdk::SessionUsage {
                tokens: self.session_usage.clone(),
                context: self.session_context_usage.clone(),
            },
            project_id: None,
            current_directory: self.current_project_root.clone(),
        }
    }

    pub(super) fn publish_message_queue_changed(&mut self) {
        let change = MessageQueueChangedNotification {
            session_id: self.session_id.clone(),
            revision: self.message_queue.revision,
        };
        self.message_queue_subscribers
            .retain(|sender| sender.try_send(change.clone()).is_ok());
    }
}

pub(crate) fn create_embedded_app() -> TuiApp {
    let client: Arc<dyn GatewayClient> = Arc::new(MockGatewayClient::new());
    TuiApp::new_with_gateway_client(client)
}

pub(crate) fn create_app_server_app() -> TuiApp {
    let client: Arc<dyn GatewayClient> = Arc::new(MockGatewayClient::new());
    TuiApp::new_with_gateway_client(client)
}

pub(crate) fn create_app_server_app_with_connect_delay(delay_ms: u64) -> TuiApp {
    let client: Arc<dyn GatewayClient> = Arc::new(MockGatewayClient::with_connect_delay(delay_ms));
    TuiApp::new_with_gateway_client(client)
}

pub(crate) fn create_app_server_app_with_model_list_failures(failures: u8) -> TuiApp {
    let client: Arc<dyn GatewayClient> =
        Arc::new(MockGatewayClient::with_list_models_failures(failures));
    TuiApp::new_with_gateway_client(client)
}

pub(crate) fn create_app_server_app_with_stream_failures(failures: u8) -> TuiApp {
    let client: Arc<dyn GatewayClient> =
        Arc::new(MockGatewayClient::with_stream_failures(failures));
    TuiApp::new_with_gateway_client(client)
}

pub(crate) fn create_app_server_app_with_close_delay(delay_ms: u64) -> TuiApp {
    let client: Arc<dyn GatewayClient> = Arc::new(MockGatewayClient::with_close_delay(delay_ms));
    TuiApp::new_with_gateway_client(client)
}

pub(crate) fn set_test_model_info(app: &mut TuiApp, model_id: &str, context_window: u32) {
    app.available_model_info = vec![mock_model_info(model_id, context_window)];
    app.available_models = vec![format!("mock:{model_id}")];
    app.current_model_provider = "mock".to_string();
}
