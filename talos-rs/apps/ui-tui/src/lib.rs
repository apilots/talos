//! Talos Terminal User Interface (TUI)
//!
//! Gateway mode supports real session APIs and SSE streaming.

use async_trait::async_trait;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
    Frame, Terminal,
};
use std::{
    collections::VecDeque,
    env,
    error::Error,
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};
use talos_agent::Agent;
use talos_client::{
    ModelInfo, SessionCreateParams, TalosClient, TalosContext, TalosEvent, TalosInputItem,
    TalosMessageRequest, TalosSession,
};
use talos_error::{TalosError, TalosResult};
use talos_types::talos::{TalosEventType, TalosUsage};
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TryRecvError;
use tracing::{error, warn};
use unicode_width::UnicodeWidthChar;

const APP_VERSION: &str = "v0.1.0";
const DEFAULT_MODEL: &str = "gpt-5.3 high";
const MAX_RIGHT_EVENTS: usize = 120;
const MAX_QUEUE_LEN: usize = 3;
const STREAM_STALL_TIMEOUT: Duration = Duration::from_secs(20);
const SESSION_CLOSE_TIMEOUT: Duration = Duration::from_secs(4);
const SPINNER_FRAMES: &[char] = &['|', '/', '-', '\\'];

/// Application states.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppState {
    Chat,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ConnectionMode {
    Embedded,
    Remote,
}

impl ConnectionMode {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Embedded => "Embedded",
            Self::Remote => "Remote",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ChatRole {
    User,
    Thinking,
    Assistant,
    System,
}

impl ChatRole {
    fn as_str(&self) -> &'static str {
        match self {
            Self::User => "User",
            Self::Thinking => "Thinking",
            Self::Assistant => "Assistant",
            Self::System => "System",
        }
    }
}

#[derive(Debug, Clone)]
struct ChatBlock {
    role: ChatRole,
    content: String,
}

impl ChatBlock {
    fn new(role: ChatRole, content: String) -> Self {
        Self { role, content }
    }
}

#[derive(Debug, Clone)]
struct QueuedMessage {
    text: String,
    dispatch_attempts: u8,
}

impl QueuedMessage {
    fn new(text: String) -> Self {
        Self {
            text,
            dispatch_attempts: 0,
        }
    }
}

enum RemoteCommandUpdate {
    ModelsLoaded(TalosResult<Vec<ModelInfo>>),
    StatusLoaded(TalosResult<Option<TalosSession>>),
    ModelSwitched {
        requested: String,
        result: TalosResult<TalosSession>,
    },
}

#[async_trait]
pub trait GatewayClient: Send + Sync {
    async fn list_models(&self) -> TalosResult<Vec<ModelInfo>>;
    async fn create_session(&self, params: &SessionCreateParams) -> TalosResult<TalosSession>;
    async fn get_session(&self, session_id: &str) -> TalosResult<TalosSession>;
    async fn close_session(&self, session_id: &str) -> TalosResult<()>;
    async fn update_session_model(
        &self,
        session_id: &str,
        model: &str,
    ) -> TalosResult<TalosSession>;
    async fn send_message_stream(
        &self,
        session_id: &str,
        request: &TalosMessageRequest,
    ) -> TalosResult<mpsc::Receiver<TalosEvent>>;
}

#[async_trait]
impl GatewayClient for TalosClient {
    async fn list_models(&self) -> TalosResult<Vec<ModelInfo>> {
        TalosClient::list_models(self).await
    }

    async fn create_session(&self, params: &SessionCreateParams) -> TalosResult<TalosSession> {
        TalosClient::create_session(self, params).await
    }

    async fn get_session(&self, session_id: &str) -> TalosResult<TalosSession> {
        TalosClient::get_session(self, session_id).await
    }

    async fn close_session(&self, session_id: &str) -> TalosResult<()> {
        TalosClient::close_session(self, session_id).await
    }

    async fn update_session_model(
        &self,
        session_id: &str,
        model: &str,
    ) -> TalosResult<TalosSession> {
        TalosClient::update_session_model(self, session_id, model).await?;
        TalosClient::get_session(self, session_id).await
    }

    async fn send_message_stream(
        &self,
        session_id: &str,
        request: &TalosMessageRequest,
    ) -> TalosResult<mpsc::Receiver<TalosEvent>> {
        TalosClient::send_message_stream(self, session_id, request).await
    }
}

/// Main TUI app.
pub struct TuiApp {
    current_state: AppState,
    input_buffer: String,
    cursor_index: usize,
    startup_workspace: PathBuf,
    connection_mode: ConnectionMode,
    current_model: String,
    session_usage: TalosUsage,
    available_model_info: Vec<ModelInfo>,
    available_models: Vec<String>,
    session_id: Option<String>,
    connection_status: String,
    chat_blocks: Vec<ChatBlock>,
    chat_history: Vec<String>,
    right_panel_events: Vec<String>,
    tool_events: Vec<String>,
    message_queue: VecDeque<QueuedMessage>,
    chat_scroll: u16,
    chat_manual_scroll: bool,
    trace_event_count: u64,
    in_flight: bool,
    received_output_delta: bool,
    active_message: Option<String>,
    active_message_raw: Option<String>,
    active_retry_count: u8,
    active_user_idx: Option<usize>,
    active_thinking_idx: Option<usize>,
    active_assistant_idx: Option<usize>,
    pending_session_connect: Option<mpsc::Receiver<TalosResult<String>>>,
    pending_session_task: Option<tokio::task::JoinHandle<()>>,
    pending_stream_connect: Option<mpsc::Receiver<TalosResult<mpsc::Receiver<TalosEvent>>>>,
    pending_stream_task: Option<tokio::task::JoinHandle<()>>,
    pending_stream: Option<mpsc::Receiver<TalosEvent>>,
    pending_command_update: Option<mpsc::Receiver<RemoteCommandUpdate>>,
    pending_command_task: Option<tokio::task::JoinHandle<()>>,
    pending_close_update: Option<mpsc::Receiver<TalosResult<String>>>,
    pending_close_task: Option<tokio::task::JoinHandle<()>>,
    active_stream_message_id: Option<String>,
    last_stream_sequence: Option<u64>,
    status_hint: Option<(String, Instant)>,
    spinner_index: usize,
    stripe_phase: usize,
    stream_started_at: Option<Instant>,
    last_stream_event_at: Option<Instant>,
    gateway_client: Option<Arc<dyn GatewayClient>>,
    _agent: Option<Arc<dyn Agent>>,
    close_in_progress: bool,
    close_started_at: Option<Instant>,
    close_timeout_reported: bool,
    closing_session_id: Option<String>,
    should_exit: bool,
}

impl TuiApp {
    fn build(
        connection_mode: ConnectionMode,
        agent: Option<Arc<dyn Agent>>,
        gateway_client: Option<Arc<dyn GatewayClient>>,
    ) -> Self {
        let mut app = Self {
            current_state: AppState::Chat,
            input_buffer: String::new(),
            cursor_index: 0,
            startup_workspace: env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            connection_mode,
            current_model: DEFAULT_MODEL.to_string(),
            session_usage: TalosUsage::default(),
            available_model_info: vec![],
            available_models: vec![],
            session_id: None,
            connection_status: "disconnected".to_string(),
            chat_blocks: vec![],
            chat_history: vec![],
            right_panel_events: vec![],
            tool_events: vec![],
            message_queue: VecDeque::new(),
            chat_scroll: 0,
            chat_manual_scroll: false,
            trace_event_count: 0,
            in_flight: false,
            received_output_delta: false,
            active_message: None,
            active_message_raw: None,
            active_retry_count: 0,
            active_user_idx: None,
            active_thinking_idx: None,
            active_assistant_idx: None,
            pending_session_connect: None,
            pending_session_task: None,
            pending_stream_connect: None,
            pending_stream_task: None,
            pending_stream: None,
            pending_command_update: None,
            pending_command_task: None,
            pending_close_update: None,
            pending_close_task: None,
            active_stream_message_id: None,
            last_stream_sequence: None,
            status_hint: None,
            spinner_index: 0,
            stripe_phase: 0,
            stream_started_at: None,
            last_stream_event_at: None,
            gateway_client,
            _agent: agent,
            close_in_progress: false,
            close_started_at: None,
            close_timeout_reported: false,
            closing_session_id: None,
            should_exit: false,
        };

        app.push_block(
            ChatRole::System,
            build_welcome_card(&app.current_model, &app.startup_workspace),
        );
        app.push_block(
            ChatRole::System,
            "Type a message or use /help /model /models /status /quit".to_string(),
        );
        app.push_right_event("tui initialized".to_string());
        app
    }

    pub fn new(agent: Arc<dyn Agent>) -> Self {
        Self::build(ConnectionMode::Embedded, Some(agent), None)
    }

    pub fn new_with_agent(agent: Arc<dyn Agent>) -> Self {
        Self::new(agent)
    }

    pub fn new_with_client(client: Arc<TalosClient>) -> Self {
        let gateway_client: Arc<dyn GatewayClient> = client;
        Self::build(ConnectionMode::Remote, None, Some(gateway_client))
    }

    pub fn new_with_gateway_client(gateway_client: Arc<dyn GatewayClient>) -> Self {
        Self::build(ConnectionMode::Remote, None, Some(gateway_client))
    }

    pub async fn initialize_remote(&mut self) {
        if self.connection_mode != ConnectionMode::Remote {
            return;
        }

        self.connection_status = "connecting".to_string();
        self.push_right_event("connecting to gateway".to_string());

        if let Err(err) = self.refresh_models_from_gateway().await {
            self.push_block(
                ChatRole::System,
                format!("Failed to load models from gateway: {}", err),
            );
            self.connection_status = "error".to_string();
            return;
        }

        match self.ensure_remote_session().await {
            Ok(session_id) => {
                self.connection_status = "connected".to_string();
                self.push_right_event(format!("session ready: {}", session_id));
            }
            Err(err) => {
                self.push_block(
                    ChatRole::System,
                    format!("Failed to create remote session: {}", err),
                );
                self.connection_status = "error".to_string();
            }
        }
    }

    pub fn get_current_state(&self) -> &AppState {
        &self.current_state
    }

    pub fn get_chat_history(&self) -> &[String] {
        &self.chat_history
    }

    pub fn get_input_buffer(&self) -> &String {
        &self.input_buffer
    }

    pub fn get_tool_status(&self) -> &[String] {
        &self.right_panel_events
    }

    pub async fn handle_input(&mut self, key_event: KeyEvent) -> Result<bool, Box<dyn Error>> {
        if key_event.modifiers.intersects(KeyModifiers::CONTROL) {
            match key_event.code {
                KeyCode::Char('c') => return Ok(true),
                KeyCode::Char('u') => {
                    self.reset_input();
                    return Ok(false);
                }
                KeyCode::Char('y') => {
                    self.copy_chat_to_clipboard();
                    return Ok(false);
                }
                _ => {}
            }
        }

        match key_event.code {
            KeyCode::Enter => {
                let should_quit = self.submit_input().await?;
                Ok(should_quit)
            }
            KeyCode::Backspace => {
                self.handle_backspace();
                Ok(false)
            }
            KeyCode::Left => {
                self.cursor_index = self.cursor_index.saturating_sub(1);
                Ok(false)
            }
            KeyCode::Right => {
                let count = self.input_buffer.chars().count();
                if self.cursor_index < count {
                    self.cursor_index += 1;
                }
                Ok(false)
            }
            KeyCode::Up => {
                self.scroll_chat_up(1);
                Ok(false)
            }
            KeyCode::Down => {
                self.scroll_chat_down(1);
                Ok(false)
            }
            KeyCode::PageUp => {
                self.scroll_chat_up(8);
                Ok(false)
            }
            KeyCode::PageDown => {
                self.scroll_chat_down(8);
                Ok(false)
            }
            KeyCode::Home => {
                self.scroll_chat_top();
                Ok(false)
            }
            KeyCode::End => {
                self.scroll_chat_bottom();
                Ok(false)
            }
            KeyCode::Char(c) => {
                if !key_event
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                {
                    self.insert_char(c);
                }
                Ok(false)
            }
            _ => Ok(false),
        }
    }

    pub fn poll_stream_events(&mut self) {
        let mut should_drop_stream = false;

        loop {
            let Some(stream) = self.pending_stream.as_mut() else {
                break;
            };

            match stream.try_recv() {
                Ok(event) => {
                    self.last_stream_event_at = Some(Instant::now());
                    self.handle_stream_event(event);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    should_drop_stream = true;
                    break;
                }
            }
        }

        if should_drop_stream {
            self.finish_active_stream("stream disconnected", true, false);
        }
    }

    async fn poll_stream_connect(&mut self) {
        let mut connect_result: Option<TalosResult<mpsc::Receiver<TalosEvent>>> = None;
        let mut should_clear = false;

        if let Some(rx) = self.pending_stream_connect.as_mut() {
            match rx.try_recv() {
                Ok(result) => {
                    connect_result = Some(result);
                    should_clear = true;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    connect_result = Some(Err(TalosError::ClientError(
                        "stream connect channel dropped".to_string(),
                    )));
                    should_clear = true;
                }
            }
        }

        if should_clear {
            self.pending_stream_connect = None;
            self.pending_stream_task = None;
        }

        match connect_result {
            Some(Ok(receiver)) => {
                self.pending_stream = Some(receiver);
                self.last_stream_event_at = Some(Instant::now());
                self.append_thinking_line("- stream opened");
                self.push_right_event("stream channel connected".to_string());
            }
            Some(Err(err)) => {
                let retry_message = self.active_message_raw.clone();
                let can_retry_once = self.active_retry_count == 0 && retry_message.is_some();
                self.discard_active_pending_blocks();
                self.finish_active_stream("stream connect failed", true, true);

                if can_retry_once {
                    if let Some(message) = retry_message {
                        self.active_retry_count = 1;
                        self.show_status_hint(
                            "Failed to open stream. Retrying once...",
                            Duration::from_secs(3),
                        );
                        if let Err(retry_err) =
                            self.start_stream_for_message_internal(message, false).await
                        {
                            self.active_message_raw = None;
                            self.active_retry_count = 0;
                            self.push_block(
                                ChatRole::System,
                                format!("Failed to start stream: {}", retry_err),
                            );
                            self.push_right_event("stream connect retry failed".to_string());
                        }
                    }
                } else {
                    self.active_message_raw = None;
                    self.active_retry_count = 0;
                    self.push_block(ChatRole::System, format!("Failed to start stream: {}", err));
                    self.push_right_event("queued dispatch dropped".to_string());
                }
            }
            None => {}
        }
    }

    fn poll_session_connect(&mut self) {
        let mut connect_result: Option<TalosResult<String>> = None;
        let mut should_clear = false;

        if let Some(rx) = self.pending_session_connect.as_mut() {
            match rx.try_recv() {
                Ok(result) => {
                    connect_result = Some(result);
                    should_clear = true;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    connect_result = Some(Err(TalosError::ClientError(
                        "session connect channel dropped".to_string(),
                    )));
                    should_clear = true;
                }
            }
        }

        if should_clear {
            self.pending_session_connect = None;
            self.pending_session_task = None;
        }

        match connect_result {
            Some(Ok(session_id)) => {
                self.session_id = Some(session_id.clone());
                self.connection_status = "connected".to_string();
                self.push_right_event(format!("session ready: {}", session_id));
            }
            Some(Err(err)) => {
                self.connection_status = "error".to_string();
                self.push_block(
                    ChatRole::System,
                    format!("Failed to create remote session: {}", err),
                );
                self.push_right_event("session create failed".to_string());
            }
            None => {}
        }
    }

    fn poll_command_updates(&mut self) {
        let mut update: Option<RemoteCommandUpdate> = None;
        let mut should_clear = false;

        if let Some(rx) = self.pending_command_update.as_mut() {
            match rx.try_recv() {
                Ok(result) => {
                    update = Some(result);
                    should_clear = true;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    self.push_block(
                        ChatRole::System,
                        "Remote command channel dropped unexpectedly.".to_string(),
                    );
                    should_clear = true;
                }
            }
        }

        if should_clear {
            self.pending_command_update = None;
            self.pending_command_task = None;
        }

        let Some(update) = update else {
            return;
        };

        match update {
            RemoteCommandUpdate::ModelsLoaded(result) => match result {
                Ok(models) => {
                    self.available_model_info = models;
                    self.available_models = self
                        .available_model_info
                        .iter()
                        .map(|model| model.id.clone())
                        .filter(|id| !id.trim().is_empty())
                        .collect();
                    if !self
                        .available_models
                        .iter()
                        .any(|model| model == &self.current_model)
                    {
                        if let Some(first) = self.available_models.first() {
                            self.current_model = first.clone();
                        }
                    }
                    self.push_block(
                        ChatRole::System,
                        format!("Available models: {}", self.available_models.join(", ")),
                    );
                    self.push_right_event(format!(
                        "models loaded: {}",
                        self.available_models.len()
                    ));
                }
                Err(err) => {
                    self.push_block(ChatRole::System, format!("Failed to load models: {}", err));
                    self.push_right_event("models load failed".to_string());
                }
            },
            RemoteCommandUpdate::StatusLoaded(result) => match result {
                Ok(session_info) => {
                    let mut lines = vec![
                        format!("mode: {}", self.connection_mode.as_str()),
                        format!("model: {}", self.current_model),
                        format!("workspace: {}", self.startup_workspace.display()),
                        format!(
                            "session: {}",
                            self.session_id
                                .clone()
                                .unwrap_or_else(|| "none".to_string())
                        ),
                        format!("connection: {}", self.connection_status),
                        format!("in_flight: {}", self.in_flight),
                        format!("events: {}", self.trace_event_count),
                    ];
                    if let Some(info) = session_info {
                        self.apply_session_snapshot(&info);
                        lines.push(format!("server.model: {}", info.config.llm.model));
                        lines.push(format!("server.messages: {}", info.state.message_count));
                        lines.push(format!(
                            "server.usage.total: {}",
                            info.state.usage.total_tokens
                        ));
                    }
                    self.push_block(ChatRole::System, lines.join("\n"));
                    self.push_right_event("status loaded".to_string());
                }
                Err(err) => {
                    self.push_block(ChatRole::System, format!("Failed to fetch status: {}", err));
                    self.push_right_event("status load failed".to_string());
                }
            },
            RemoteCommandUpdate::ModelSwitched { requested, result } => match result {
                Ok(info) => {
                    self.apply_session_snapshot(&info);
                    self.push_block(
                        ChatRole::System,
                        format!("Model switched to `{}`.", self.current_model),
                    );
                    self.push_right_event(format!("session model updated: {}", requested));
                }
                Err(err) => {
                    self.push_block(ChatRole::System, format!("Failed to switch model: {}", err));
                    self.push_right_event("model switch failed".to_string());
                }
            },
        }
    }

    fn poll_close_updates(&mut self) {
        let mut result: Option<TalosResult<String>> = None;
        let mut should_clear = false;

        if let Some(rx) = self.pending_close_update.as_mut() {
            match rx.try_recv() {
                Ok(update) => {
                    result = Some(update);
                    should_clear = true;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    result = Some(Err(TalosError::ClientError(
                        "session close channel dropped".to_string(),
                    )));
                    should_clear = true;
                }
            }
        }

        if should_clear {
            self.pending_close_update = None;
            self.pending_close_task = None;
        }

        match result {
            Some(Ok(session_id)) => {
                self.close_in_progress = false;
                self.close_started_at = None;
                self.close_timeout_reported = false;
                self.closing_session_id = None;
                self.connection_status = "closed".to_string();
                self.push_right_event(format!("session close completed: {}", session_id));
                self.push_block(
                    ChatRole::System,
                    format!("Shutting down TUI. Closed session `{}`.", session_id),
                );
                self.should_exit = true;
            }
            Some(Err(err)) => {
                self.close_in_progress = false;
                self.close_started_at = None;
                self.close_timeout_reported = false;
                self.closing_session_id = None;
                self.push_block(
                    ChatRole::System,
                    format!("Failed to close session: {}", err),
                );
                self.push_right_event("session close failed".to_string());
            }
            None => {}
        }
    }

    fn poll_close_timeout(&mut self) {
        if !self.close_in_progress || self.close_timeout_reported {
            return;
        }

        if self.is_close_timed_out() {
            self.close_timeout_reported = true;
            self.show_status_hint(
                "Session close timed out. Press /quit again to force exit.",
                Duration::from_secs(4),
            );
            self.push_right_event("session close timed out".to_string());
        }
    }

    pub async fn on_tick(&mut self) {
        self.spinner_index = (self.spinner_index + 1) % SPINNER_FRAMES.len();
        self.stripe_phase = self.stripe_phase.wrapping_add(1);

        if let Some((_, expires_at)) = &self.status_hint {
            if Instant::now() >= *expires_at {
                self.status_hint = None;
            }
        }

        self.poll_session_connect();
        self.poll_stream_connect().await;
        self.poll_stream_events();
        self.poll_command_updates();
        self.poll_close_updates();
        self.poll_close_timeout();

        if self.in_flight {
            if let Some(last_event_at) = self.last_stream_event_at {
                if last_event_at.elapsed() > STREAM_STALL_TIMEOUT {
                    let retry_message = self.active_message_raw.clone();
                    let can_retry_once = self.active_retry_count == 0 && retry_message.is_some();
                    self.finish_active_stream("stream stalled and timed out", true, true);

                    if can_retry_once {
                        if let Some(message) = retry_message {
                            self.active_retry_count = 1;
                            self.show_status_hint(
                                "Current response timed out. Retrying once...",
                                Duration::from_secs(3),
                            );
                            if let Err(err) =
                                self.start_stream_for_message_internal(message, false).await
                            {
                                self.active_message_raw = None;
                                self.active_retry_count = 0;
                                self.push_block(ChatRole::System, format!("Retry failed: {}", err));
                                self.show_status_hint(
                                    "Retry failed. Moving to next queued message.",
                                    Duration::from_secs(4),
                                );
                            }
                        }
                    } else {
                        self.active_message_raw = None;
                        self.active_retry_count = 0;
                        self.show_status_hint(
                            "Current response timed out. Moving to next queued message.",
                            Duration::from_secs(4),
                        );
                    }
                }
            }
        }

        if !self.in_flight && !self.close_in_progress {
            self.dispatch_next_queued_message().await;
        }
    }

    pub fn render_ui(&mut self, f: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(72), Constraint::Percentage(28)])
            .split(f.area());
        self.render_left_panel(f, chunks[0]);
        self.render_right_panel(f, chunks[1]);
    }

    async fn submit_input(&mut self) -> TalosResult<bool> {
        let user_input = self.input_buffer.trim().to_string();
        if user_input.is_empty() {
            self.reset_input();
            return Ok(false);
        }

        let result = if user_input.starts_with('/') {
            self.push_block(ChatRole::User, user_input.clone());
            self.reset_input();
            self.execute_command(&user_input).await
        } else {
            self.reset_input();
            match self.send_user_message(user_input.clone()).await {
                Ok(true) => Ok(false),
                Ok(false) => {
                    self.input_buffer = user_input;
                    self.cursor_index = self.input_buffer.chars().count();
                    Ok(false)
                }
                Err(err) => Err(err),
            }
        };

        match result {
            Ok(should_quit) => Ok(should_quit),
            Err(err) => {
                self.push_block(ChatRole::System, format!("Failed to handle input: {}", err));
                self.push_right_event("input handling failed".to_string());
                Ok(false)
            }
        }
    }

    async fn execute_command(&mut self, command_line: &str) -> TalosResult<bool> {
        let trimmed = command_line.trim_start_matches('/').trim();
        let mut parts = trimmed.split_whitespace();
        let cmd = parts.next().unwrap_or_default();
        let args = parts.collect::<Vec<_>>().join(" ");

        if self.close_in_progress && cmd != "quit" {
            self.show_status_hint(
                "Session is closing. Wait or press /quit again after timeout.",
                Duration::from_secs(2),
            );
            return Ok(false);
        }

        match cmd {
            "help" => {
                self.push_block(
                    ChatRole::System,
                    "Commands: /help /model <id> /models /modes /status /quit\nKeys: ↑/↓/PgUp/PgDn scroll, Ctrl+Y copy, Ctrl+U clear input"
                        .to_string(),
                );
                Ok(false)
            }
            "models" | "modes" => {
                self.refresh_models_command()?;
                Ok(false)
            }
            "model" => {
                let requested = args.trim();
                if requested.is_empty() {
                    self.push_block(
                        ChatRole::System,
                        format!(
                            "Current model: `{}`. Usage: /model <id>",
                            self.current_model
                        ),
                    );
                    return Ok(false);
                }

                if !self.available_models.is_empty()
                    && !self.available_models.iter().any(|m| m == requested)
                {
                    self.push_block(
                        ChatRole::System,
                        format!("Unknown model `{}`. Run /models first.", requested),
                    );
                    return Ok(false);
                }

                if self.connection_mode == ConnectionMode::Remote {
                    self.switch_remote_model_command(requested.to_string())?;
                } else {
                    self.current_model = requested.to_string();
                    self.push_block(
                        ChatRole::System,
                        format!("Model switched locally to `{}`.", self.current_model),
                    );
                }

                Ok(false)
            }
            "status" => {
                self.status_command()?;
                Ok(false)
            }
            "quit" => {
                if self.connection_mode == ConnectionMode::Remote {
                    if self.close_in_progress {
                        if self.is_close_timed_out() {
                            self.push_right_event("session force exit".to_string());
                            self.push_block(
                                ChatRole::System,
                                "Forcing local exit while remote session closes in background."
                                    .to_string(),
                            );
                            self.should_exit = true;
                            return Ok(false);
                        }

                        self.show_status_hint(
                            "Session is still closing. Wait or press /quit again after timeout.",
                            Duration::from_secs(2),
                        );
                        return Ok(false);
                    }
                    self.close_remote_session_command()?;
                    return Ok(false);
                }
                self.push_block(ChatRole::System, "Shutting down TUI.".to_string());
                Ok(true)
            }
            unknown => {
                self.push_block(
                    ChatRole::System,
                    format!("Unknown command `{}`. Run /help.", unknown),
                );
                Ok(false)
            }
        }
    }

    async fn send_user_message(&mut self, user_message: String) -> TalosResult<bool> {
        if self.close_in_progress {
            self.show_status_hint(
                "Session is closing. New messages are blocked.",
                Duration::from_secs(2),
            );
            return Ok(false);
        }

        if self.in_flight || !self.message_queue.is_empty() {
            if self.message_queue.len() >= MAX_QUEUE_LEN {
                self.show_status_hint(
                    format!(
                        "Queue is full ({}/{}). Wait for current response.",
                        self.message_queue.len(),
                        MAX_QUEUE_LEN
                    ),
                    Duration::from_secs(2),
                );
                self.push_right_event("queue full".to_string());
                return Ok(false);
            }
        } else if self.message_queue.len() >= MAX_QUEUE_LEN {
            self.show_status_hint(
                format!(
                    "Queue is full ({}/{}). Wait for current response.",
                    self.message_queue.len(),
                    MAX_QUEUE_LEN
                ),
                Duration::from_secs(2),
            );
            self.push_right_event("queue full".to_string());
            return Ok(false);
        }

        self.message_queue
            .push_back(QueuedMessage::new(user_message.clone()));
        self.push_right_event(format!(
            "queued ({}/{}): {}",
            self.message_queue.len(),
            MAX_QUEUE_LEN,
            summarize_for_queue(&user_message)
        ));
        self.dispatch_next_queued_message().await;
        Ok(true)
    }

    async fn dispatch_next_queued_message(&mut self) {
        if self.in_flight {
            return;
        }
        let Some(next_message) = self.message_queue.front().cloned() else {
            return;
        };

        if self.connection_mode == ConnectionMode::Remote && self.session_id.is_none() {
            self.start_remote_session_connect();
            return;
        }

        if let Err(err) = self
            .start_stream_for_message(next_message.text.clone())
            .await
        {
            if let Some(front) = self.message_queue.front_mut() {
                front.dispatch_attempts = front.dispatch_attempts.saturating_add(1);
                if front.dispatch_attempts <= 1 {
                    self.show_status_hint(
                        "Queued message dispatch failed. Retrying once...",
                        Duration::from_secs(3),
                    );
                    self.push_right_event("queued dispatch retry".to_string());
                    return;
                }
            }

            let failed = self.message_queue.pop_front();
            self.push_block(
                ChatRole::System,
                format!("Failed to dispatch queued message: {}", err),
            );
            if let Some(failed) = failed {
                self.push_right_event(format!(
                    "queued dispatch dropped: {}",
                    summarize_for_queue(&failed.text)
                ));
            } else {
                self.push_right_event("queued dispatch failed".to_string());
            }
            return;
        }

        if self.in_flight || self.connection_mode == ConnectionMode::Embedded {
            self.message_queue.pop_front();
        }
    }

    async fn start_stream_for_message(&mut self, user_message: String) -> TalosResult<()> {
        self.start_stream_for_message_internal(user_message, true)
            .await
    }

    async fn start_stream_for_message_internal(
        &mut self,
        user_message: String,
        reset_retry_state: bool,
    ) -> TalosResult<()> {
        match self.connection_mode {
            ConnectionMode::Remote => {
                let Some(client) = self.gateway_client.clone() else {
                    self.push_block(ChatRole::System, "Gateway client unavailable.".to_string());
                    return Ok(());
                };

                let Some(session_id) = self.session_id.clone() else {
                    return Err(TalosError::ClientError(
                        "remote session is not ready".to_string(),
                    ));
                };
                let active_summary = summarize_for_queue(&user_message);
                let request = TalosMessageRequest {
                    input: vec![TalosInputItem::Text {
                        text: user_message.clone(),
                    }],
                    stream: Some(true),
                    workspace_id: None,
                };

                self.abort_pending_stream_task();
                self.in_flight = true;
                self.received_output_delta = false;
                self.active_message = Some(active_summary);
                if reset_retry_state {
                    self.active_retry_count = 0;
                    self.active_message_raw = Some(user_message.clone());
                }
                self.stream_started_at = Some(Instant::now());
                self.last_stream_event_at = Some(Instant::now());
                self.pending_stream = None;
                self.pending_stream_connect = None;
                self.active_stream_message_id = None;
                self.last_stream_sequence = None;

                if reset_retry_state {
                    let user_idx = self.push_block(ChatRole::User, user_message.clone());
                    self.active_user_idx = Some(user_idx);
                }

                let thinking_idx = self.push_block(
                    ChatRole::Thinking,
                    "- waiting for model response...".to_string(),
                );
                let assistant_idx = self.push_block(ChatRole::Assistant, String::new());
                self.active_thinking_idx = Some(thinking_idx);
                self.active_assistant_idx = Some(assistant_idx);

                let (tx, rx) = mpsc::channel(1);
                self.pending_stream_connect = Some(rx);
                self.push_right_event(format!("message dispatched to session {}", session_id));

                let handle = tokio::spawn(async move {
                    let result = client.send_message_stream(&session_id, &request).await;
                    if tx.send(result).await.is_err() {
                        error!(
                            session_id = %session_id,
                            "failed to deliver stream connection result to TUI task"
                        );
                    }
                });
                self.pending_stream_task = Some(handle);
            }
            ConnectionMode::Embedded => {
                self.push_block(
                    ChatRole::Thinking,
                    "- embedded mode runtime is not wired yet".to_string(),
                );
                self.push_block(
                    ChatRole::Assistant,
                    "_Embedded mode response placeholder._".to_string(),
                );
            }
        }

        Ok(())
    }

    fn handle_stream_event(&mut self, event: TalosEvent) {
        self.trace_event_count = self.trace_event_count.saturating_add(1);

        if let Some(session_id) = self.session_id.as_deref() {
            if event.session_id != session_id {
                self.push_right_event(format!(
                    "ignored event from other session: {}",
                    event.session_id
                ));
                return;
            }
        }

        match self.active_stream_message_id.as_deref() {
            Some(active_id) if event.message_id != active_id => {
                self.push_right_event(format!(
                    "ignored stale event for {} (active {})",
                    event.message_id, active_id
                ));
                return;
            }
            None => {
                self.active_stream_message_id = Some(event.message_id.clone());
            }
            _ => {}
        }

        if let Some(last_seq) = self.last_stream_sequence {
            if event.sequence < last_seq {
                self.push_right_event(format!(
                    "ignored out-of-order event seq {} < {}",
                    event.sequence, last_seq
                ));
                return;
            }
        }
        self.last_stream_sequence = Some(event.sequence);
        self.request_follow_bottom();

        if event.event_type != TalosEventType::MessageOutputDelta {
            self.push_right_event(format!(
                "{} #{}",
                canonical_event_label(&event.event_type),
                event.sequence
            ));
        }

        match event.event_type {
            TalosEventType::MessageStarted => {
                self.append_thinking_line("- message started");
            }
            TalosEventType::MessageReasoningDelta => {
                if let Some(delta) = event.data.get("delta").and_then(|v| v.as_str()) {
                    if !delta.trim().is_empty() {
                        let source = event
                            .data
                            .get("reasoning_source")
                            .and_then(|v| v.as_str())
                            .unwrap_or("provider");
                        let step_id = event
                            .data
                            .get("step_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("-");
                        let iteration = event
                            .data
                            .get("iteration")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0);
                        let label = match source {
                            "kernel" => "summary",
                            "provider" => "thinking",
                            _ => source,
                        };
                        self.append_thinking_line(&format!(
                            "- {} [{} #{}]: {}",
                            label, step_id, iteration, delta
                        ));
                    }
                }
            }
            TalosEventType::MessageOutputDelta => {
                if let Some(delta) = event.data.get("delta").and_then(|v| v.as_str()) {
                    self.append_assistant(delta);
                    if !self.received_output_delta {
                        self.append_thinking_line("- generating response content");
                        self.received_output_delta = true;
                    }
                }
            }
            TalosEventType::MessageToolCallStarted => {
                let name = event
                    .data
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let step_id = event
                    .data
                    .get("step_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("-");
                self.append_thinking_line(&format!("- tool started [{}]: {}", step_id, name));
                self.push_right_event(format!("tool started [{}]: {}", step_id, name));
                self.tool_events
                    .push(format!("started [{}]: {}", step_id, name));
            }
            TalosEventType::MessageToolCallCompleted => {
                let call_id = event
                    .data
                    .get("call_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let tool = event
                    .data
                    .get("tool")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                self.append_thinking_line(&format!("- tool completed: {} ({})", call_id, tool));
                self.push_right_event(format!("tool completed: {} ({})", call_id, tool));
                self.tool_events
                    .push(format!("completed: {} ({})", call_id, tool));
            }
            TalosEventType::MessageCompleted => {
                if !self.received_output_delta {
                    if let Some(text) = extract_text_from_completed_event(&event.data) {
                        if !text.trim().is_empty() {
                            self.append_assistant(&text);
                            self.received_output_delta = true;
                        }
                    }
                }
                if let Some(usage) = extract_usage_from_completed_event(&event.data) {
                    self.session_usage.input_tokens = self
                        .session_usage
                        .input_tokens
                        .saturating_add(usage.input_tokens);
                    self.session_usage.output_tokens = self
                        .session_usage
                        .output_tokens
                        .saturating_add(usage.output_tokens);
                    self.session_usage.total_tokens = self
                        .session_usage
                        .total_tokens
                        .saturating_add(usage.total_tokens);
                }
                self.append_thinking_line("- stream completed");
                self.finish_active_stream("stream completed", false, false);
            }
            TalosEventType::MessageError => {
                self.append_thinking_line("- stream error");
                self.push_block(ChatRole::System, format!("Stream error: {}", event.data));
                self.finish_active_stream("stream error", true, false);
            }
        }
    }

    async fn refresh_models_from_gateway(&mut self) -> TalosResult<()> {
        if self.connection_mode != ConnectionMode::Remote {
            return Ok(());
        }

        let Some(client) = self.gateway_client.clone() else {
            return Err(TalosError::ClientError(
                "missing gateway client in remote mode".to_string(),
            ));
        };

        let models = client.list_models().await?;
        let ids: Vec<String> = models
            .iter()
            .map(|m| m.id.clone())
            .filter(|v| !v.trim().is_empty())
            .collect();
        if ids.is_empty() {
            return Err(TalosError::ClientError(
                "gateway returned empty model list".to_string(),
            ));
        }
        self.available_model_info = models;
        self.available_models = ids;
        if !self
            .available_models
            .iter()
            .any(|m| m == &self.current_model)
        {
            self.current_model = self.available_models[0].clone();
        }
        self.push_right_event(format!("models loaded: {}", self.available_models.len()));
        Ok(())
    }

    fn refresh_models_command(&mut self) -> TalosResult<()> {
        if self.connection_mode != ConnectionMode::Remote {
            return Ok(());
        }

        let Some(client) = self.gateway_client.clone() else {
            return Err(TalosError::ClientError(
                "missing gateway client in remote mode".to_string(),
            ));
        };

        self.spawn_remote_command_task("models load started", async move {
            let result = client.list_models().await;
            RemoteCommandUpdate::ModelsLoaded(result)
        })
    }

    fn status_command(&mut self) -> TalosResult<()> {
        if self.connection_mode != ConnectionMode::Remote {
            let lines = vec![
                format!("mode: {}", self.connection_mode.as_str()),
                format!("model: {}", self.current_model),
                format!("workspace: {}", self.startup_workspace.display()),
                "session: none".to_string(),
                format!("connection: {}", self.connection_status),
                format!("in_flight: {}", self.in_flight),
                format!("events: {}", self.trace_event_count),
            ];
            self.push_block(ChatRole::System, lines.join("\n"));
            return Ok(());
        }

        let Some(client) = self.gateway_client.clone() else {
            return Err(TalosError::ClientError(
                "missing gateway client in remote mode".to_string(),
            ));
        };
        let session_id = self.session_id.clone();
        self.spawn_remote_command_task("status request started", async move {
            let result = if let Some(session_id) = session_id {
                client.get_session(&session_id).await.map(Some)
            } else {
                Ok(None)
            };
            RemoteCommandUpdate::StatusLoaded(result)
        })
    }

    fn switch_remote_model_command(&mut self, requested: String) -> TalosResult<()> {
        let Some(client) = self.gateway_client.clone() else {
            return Err(TalosError::ClientError(
                "missing gateway client in remote mode".to_string(),
            ));
        };
        let session_id = self.session_id.clone();
        self.spawn_remote_command_task(format!("model switch started: {}", requested), async move {
            let result = if let Some(session_id) = session_id {
                client.update_session_model(&session_id, &requested).await
            } else {
                let context = TalosContext::cli(
                    env::var("USER").unwrap_or_else(|_| "local-user".to_string()),
                )
                .with_channel_id("terminal");
                client
                    .create_session(&SessionCreateParams {
                        context: Some(context),
                        model: Some(requested.clone()),
                    })
                    .await
            };
            RemoteCommandUpdate::ModelSwitched { requested, result }
        })
    }

    fn close_remote_session_command(&mut self) -> TalosResult<()> {
        if self.close_in_progress {
            return Ok(());
        }

        let Some(client) = self.gateway_client.clone() else {
            return Err(TalosError::ClientError(
                "missing gateway client in remote mode".to_string(),
            ));
        };
        let session_id = self
            .session_id
            .clone()
            .unwrap_or_else(|| "none".to_string());
        let (tx, rx) = mpsc::channel(1);
        self.pending_close_update = Some(rx);
        self.close_in_progress = true;
        self.close_started_at = Some(Instant::now());
        self.close_timeout_reported = false;
        self.closing_session_id = Some(session_id.clone());
        self.connection_status = "closing".to_string();
        self.show_status_hint(
            "Closing session and flushing memory...",
            Duration::from_secs(2),
        );
        self.push_right_event(format!("session close started: {}", session_id));

        let handle = tokio::spawn(async move {
            let result = client
                .close_session(&session_id)
                .await
                .map(|_| session_id.clone());
            if tx.send(result).await.is_err() {
                error!(
                    session_id = %session_id,
                    "failed to deliver close session result to TUI task"
                );
            }
        });
        self.pending_close_task = Some(handle);
        Ok(())
    }

    fn spawn_remote_command_task<F>(&mut self, event: impl Into<String>, fut: F) -> TalosResult<()>
    where
        F: std::future::Future<Output = RemoteCommandUpdate> + Send + 'static,
    {
        if self.pending_command_update.is_some() {
            self.show_status_hint(
                "Another remote command is still running.",
                Duration::from_secs(2),
            );
            return Ok(());
        }

        let (tx, rx) = mpsc::channel(1);
        self.pending_command_update = Some(rx);
        self.push_right_event(event.into());
        let handle = tokio::spawn(async move {
            let result = fut.await;
            if tx.send(result).await.is_err() {
                error!("failed to deliver remote command result to TUI task");
            }
        });
        self.pending_command_task = Some(handle);
        Ok(())
    }

    async fn ensure_remote_session(&mut self) -> TalosResult<String> {
        if self.connection_mode != ConnectionMode::Remote {
            return Ok("embedded-session".to_string());
        }

        if let Some(session_id) = self.session_id.clone() {
            return Ok(session_id);
        }

        let Some(client) = self.gateway_client.clone() else {
            return Err(TalosError::ClientError(
                "missing gateway client in remote mode".to_string(),
            ));
        };

        let context =
            TalosContext::cli(env::var("USER").unwrap_or_else(|_| "local-user".to_string()))
                .with_channel_id("terminal");
        let params = SessionCreateParams {
            context: Some(context),
            model: Some(self.current_model.clone()),
        };

        let created = client.create_session(&params).await?;
        self.apply_session_snapshot(&created);
        self.connection_status = "connected".to_string();
        Ok(created.context.session_id)
    }

    fn start_remote_session_connect(&mut self) {
        if self.connection_mode != ConnectionMode::Remote
            || self.session_id.is_some()
            || self.pending_session_connect.is_some()
        {
            return;
        }

        let Some(client) = self.gateway_client.clone() else {
            self.push_block(ChatRole::System, "Gateway client unavailable.".to_string());
            return;
        };

        let current_model = self.current_model.clone();
        let (tx, rx) = mpsc::channel(1);
        self.pending_session_connect = Some(rx);
        self.connection_status = "connecting".to_string();
        self.push_right_event("session create started".to_string());

        let handle = tokio::spawn(async move {
            let context =
                TalosContext::cli(env::var("USER").unwrap_or_else(|_| "local-user".to_string()))
                    .with_channel_id("terminal");
            let result = client
                .create_session(&SessionCreateParams {
                    context: Some(context),
                    model: Some(current_model),
                })
                .await
                .map(|created| created.context.session_id);
            if tx.send(result).await.is_err() {
                error!("failed to deliver create session result to TUI task");
            }
        });
        self.pending_session_task = Some(handle);
    }

    fn is_close_timed_out(&self) -> bool {
        self.close_started_at
            .map(|started_at| started_at.elapsed() >= SESSION_CLOSE_TIMEOUT)
            .unwrap_or(false)
    }

    fn insert_char(&mut self, c: char) {
        let idx = byte_index_at_char(&self.input_buffer, self.cursor_index);
        self.input_buffer.insert(idx, c);
        self.cursor_index += 1;
    }

    fn handle_backspace(&mut self) {
        if self.cursor_index == 0 {
            return;
        }

        let current = byte_index_at_char(&self.input_buffer, self.cursor_index);
        let previous = byte_index_at_char(&self.input_buffer, self.cursor_index - 1);
        self.input_buffer.drain(previous..current);
        self.cursor_index -= 1;
    }

    fn reset_input(&mut self) {
        self.input_buffer.clear();
        self.cursor_index = 0;
    }

    fn show_status_hint(&mut self, message: impl Into<String>, ttl: Duration) {
        self.status_hint = Some((message.into(), Instant::now() + ttl));
    }

    fn scroll_chat_up(&mut self, lines: u16) {
        self.chat_manual_scroll = true;
        self.chat_scroll = self.chat_scroll.saturating_sub(lines);
    }

    fn scroll_chat_down(&mut self, lines: u16) {
        self.chat_manual_scroll = true;
        self.chat_scroll = self.chat_scroll.saturating_add(lines);
    }

    fn scroll_chat_top(&mut self) {
        self.chat_manual_scroll = true;
        self.chat_scroll = 0;
    }

    fn scroll_chat_bottom(&mut self) {
        self.chat_manual_scroll = false;
    }

    fn request_follow_bottom(&mut self) {
        self.chat_manual_scroll = false;
    }

    fn copy_chat_to_clipboard(&mut self) {
        let text = self
            .chat_blocks
            .iter()
            .map(|block| format!("{}\n{}", block.role.as_str(), block.content))
            .collect::<Vec<_>>()
            .join("\n\n");

        if text.trim().is_empty() {
            self.push_block(ChatRole::System, "Nothing to copy.".to_string());
            return;
        }

        match copy_to_system_clipboard(&text) {
            Ok(()) => self.push_block(ChatRole::System, "Copied chat to clipboard.".to_string()),
            Err(err) => {
                self.push_block(ChatRole::System, format!("Clipboard copy failed: {}", err))
            }
        };
    }

    fn push_block(&mut self, role: ChatRole, content: String) -> usize {
        self.chat_blocks.push(ChatBlock::new(role, content));
        let idx = self.chat_blocks.len() - 1;
        self.sync_history_entry(idx);
        self.request_follow_bottom();
        idx
    }

    fn discard_active_pending_blocks(&mut self) {
        let mut remove_indices = vec![];
        if let Some(idx) = self.active_assistant_idx {
            remove_indices.push(idx);
        }
        if let Some(idx) = self.active_thinking_idx {
            remove_indices.push(idx);
        }
        if let Some(idx) = self.active_user_idx {
            remove_indices.push(idx);
        }
        remove_indices.sort_unstable();
        remove_indices.dedup();

        for idx in remove_indices.into_iter().rev() {
            if idx < self.chat_blocks.len() {
                self.chat_blocks.remove(idx);
            }
        }

        self.chat_history = self
            .chat_blocks
            .iter()
            .map(|block| format!("{}\n{}", block.role.as_str(), block.content))
            .collect();
        self.active_user_idx = None;
        self.active_thinking_idx = None;
        self.active_assistant_idx = None;
    }

    fn append_assistant(&mut self, text: &str) {
        let idx = if let Some(idx) = self.active_assistant_idx {
            idx
        } else {
            let idx = self.push_block(ChatRole::Assistant, String::new());
            self.active_assistant_idx = Some(idx);
            idx
        };

        if let Some(block) = self.chat_blocks.get_mut(idx) {
            block.content.push_str(text);
        }
        self.sync_history_entry(idx);
        self.request_follow_bottom();
    }

    fn append_thinking_line(&mut self, line: &str) {
        let idx = if let Some(idx) = self.active_thinking_idx {
            idx
        } else {
            let idx = self.push_block(ChatRole::Thinking, String::new());
            self.active_thinking_idx = Some(idx);
            idx
        };

        if let Some(block) = self.chat_blocks.get_mut(idx) {
            if block.content.is_empty() {
                block.content.push_str(line);
            } else {
                block.content.push('\n');
                block.content.push_str(line);
            }
        }
        self.sync_history_entry(idx);
        self.request_follow_bottom();
    }

    fn push_right_event(&mut self, event: String) {
        self.right_panel_events.push(event);
        if self.right_panel_events.len() > MAX_RIGHT_EVENTS {
            let overflow = self.right_panel_events.len() - MAX_RIGHT_EVENTS;
            self.right_panel_events.drain(0..overflow);
        }
        if self.tool_events.len() > MAX_RIGHT_EVENTS {
            let overflow = self.tool_events.len() - MAX_RIGHT_EVENTS;
            self.tool_events.drain(0..overflow);
        }
    }

    fn abort_pending_stream_task(&mut self) {
        if let Some(handle) = self.pending_stream_task.take() {
            handle.abort();
        }
    }

    fn finish_active_stream(&mut self, reason: &str, is_error: bool, keep_active_payload: bool) {
        self.in_flight = false;
        self.pending_stream_connect = None;
        self.pending_stream = None;
        self.abort_pending_stream_task();
        self.active_user_idx = None;
        self.active_thinking_idx = None;
        self.active_assistant_idx = None;
        self.active_stream_message_id = None;
        self.last_stream_sequence = None;
        self.stream_started_at = None;
        self.last_stream_event_at = None;
        self.active_message = None;
        if !keep_active_payload {
            self.active_message_raw = None;
            self.active_retry_count = 0;
        }
        self.push_right_event(if is_error {
            format!("stream finished with error: {}", reason)
        } else {
            format!("stream finished: {}", reason)
        });
    }

    fn sync_history_entry(&mut self, idx: usize) {
        if let Some(block) = self.chat_blocks.get(idx) {
            let entry = format!("{}\n{}", block.role.as_str(), block.content);
            if idx < self.chat_history.len() {
                self.chat_history[idx] = entry;
            } else {
                self.chat_history.push(entry);
            }
        }
    }

    fn render_left_panel(&mut self, f: &mut Frame, area: ratatui::layout::Rect) {
        let left_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Rgb(106, 111, 123)))
            .title(format!("Talos TUI ({})", APP_VERSION));
        let left_inner = left_block.inner(area);
        f.render_widget(left_block, area);

        let show_queue_panel = !self.message_queue.is_empty();
        let left_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(4),
                Constraint::Length(if show_queue_panel { 5 } else { 0 }),
                Constraint::Length(3),
                Constraint::Length(1),
            ])
            .split(left_inner);

        let chat_panel = Block::default().style(Style::default().bg(Color::Rgb(33, 38, 49)));
        let chat_inner = chat_panel.inner(left_chunks[0]);
        f.render_widget(chat_panel, left_chunks[0]);

        let chat_columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(chat_inner);
        let chat_content_rect = chat_columns[0];
        let chat_scrollbar_rect = chat_columns[1];

        let mut lines: Vec<Line> = vec![];
        let content_width = chat_content_rect.width.saturating_sub(2).max(1) as usize;
        for block in &self.chat_blocks {
            lines.extend(self.chat_card_lines(block, content_width));
            lines.push(Line::from(""));
        }

        if lines.is_empty() {
            lines.push(Line::from("No messages yet."));
        }

        let viewport_height = chat_content_rect.height as usize;
        let total_lines = lines.len();
        let max_scroll = total_lines.saturating_sub(viewport_height);
        if !self.chat_manual_scroll {
            self.chat_scroll = max_scroll as u16;
        } else {
            let max_scroll_u16 = max_scroll as u16;
            if self.chat_scroll >= max_scroll_u16 {
                self.chat_scroll = max_scroll_u16;
                self.chat_manual_scroll = false;
            }
        }

        let conversation = Paragraph::new(lines)
            .scroll((self.chat_scroll, 0))
            .style(Style::default().fg(Color::White));
        f.render_widget(conversation, chat_content_rect);

        if chat_scrollbar_rect.width > 0 && chat_scrollbar_rect.height > 0 {
            f.render_widget(
                Block::default().style(Style::default().bg(Color::Rgb(18, 21, 28))),
                chat_scrollbar_rect,
            );
        }

        if max_scroll > 0 {
            let mut state = ScrollbarState::new(max_scroll).position(self.chat_scroll as usize);
            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .track_symbol(Some("│"))
                .thumb_symbol("█")
                .style(Style::default().fg(Color::Rgb(58, 63, 74)))
                .thumb_style(Style::default().fg(Color::Rgb(89, 96, 110)));
            f.render_stateful_widget(scrollbar, chat_scrollbar_rect, &mut state);
        }

        if show_queue_panel {
            let queue_lines =
                self.queue_panel_lines((left_chunks[1].height as usize).saturating_sub(1));
            let queue = Paragraph::new(queue_lines)
                .block(
                    Block::default()
                        .borders(Borders::TOP)
                        .border_style(Style::default().fg(Color::Rgb(88, 92, 102))),
                )
                .style(Style::default().fg(Color::Gray).add_modifier(Modifier::DIM))
                .wrap(Wrap { trim: false });
            f.render_widget(queue, left_chunks[1]);
        }

        let input_panel = Block::default();
        let input_inner = input_panel.inner(left_chunks[2]);
        f.render_widget(input_panel, left_chunks[2]);

        let input_bg_style = Style::default().bg(Color::Rgb(66, 70, 78));
        f.render_widget(Block::default().style(input_bg_style), input_inner);

        let input_rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .split(input_inner);

        let blank = Paragraph::new(Line::from("")).style(input_bg_style);
        f.render_widget(blank, input_rows[0]);

        let input_row = Paragraph::new(self.input_line()).style(input_bg_style);
        f.render_widget(input_row, input_rows[1]);

        let wait_style = if self.in_flight {
            Style::default()
                .bg(Color::Rgb(36, 39, 46))
                .fg(Color::Cyan)
                .add_modifier(Modifier::DIM)
        } else {
            Style::default()
                .bg(Color::Rgb(36, 39, 46))
                .fg(Color::Gray)
                .add_modifier(Modifier::DIM)
        };
        let status_block = Block::default().style(Style::default().bg(Color::Rgb(36, 39, 46)));
        f.render_widget(status_block, left_chunks[3]);
        let wait_row = Paragraph::new(self.input_status_line(left_chunks[3].width as usize))
            .style(wait_style)
            .wrap(Wrap { trim: false });
        f.render_widget(wait_row, left_chunks[3]);
    }

    fn render_right_panel(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        let right_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Rgb(106, 111, 123)))
            .title("Runtime / Tools");
        let right_inner = right_block.inner(area);
        f.render_widget(right_block, area);

        let spinner = SPINNER_FRAMES[self.spinner_index];
        let mut lines = vec![];
        lines.push(Line::from(Span::styled(
            "Context",
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::White),
        )));
        lines.push(Line::from(format!(
            "status: {} {}",
            spinner, self.connection_status
        )));
        lines.push(Line::from(format!(
            "mode: {}",
            self.connection_mode.as_str()
        )));
        lines.push(Line::from(format!("model: {}", self.current_model)));
        lines.push(Line::from(format!(
            "session: {}",
            self.session_id
                .clone()
                .unwrap_or_else(|| "none".to_string())
        )));
        lines.push(Line::from(format!(
            "queue: {}/{}",
            self.message_queue.len(),
            MAX_QUEUE_LEN
        )));
        let phase = if self.close_in_progress {
            "closing"
        } else if self.in_flight && !self.received_output_delta {
            "waiting_first_token"
        } else if self.in_flight {
            "streaming"
        } else {
            "idle"
        };
        lines.push(Line::from(format!("phase: {}", phase)));
        lines.push(Line::from(""));

        lines.push(Line::from(Span::styled(
            "Tools",
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::White),
        )));
        if self.tool_events.is_empty() {
            lines.push(Line::from("- none"));
        } else {
            for event in self.tool_events.iter().rev().take(4).rev() {
                lines.push(Line::from(format!("- {}", event)));
            }
        }
        lines.push(Line::from(""));

        lines.push(Line::from(Span::styled(
            "LSP",
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::White),
        )));
        lines.push(Line::from("- pyright Connected"));
        lines.push(Line::from("- root: ../../talos-rs"));
        lines.push(Line::from(""));

        lines.push(Line::from(Span::styled(
            "Todos",
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::White),
        )));
        if self.close_in_progress {
            let session_id = self
                .closing_session_id
                .clone()
                .unwrap_or_else(|| "unknown".to_string());
            lines.push(Line::from(format!("[~] closing {}", session_id)));
        } else if let Some(active) = &self.active_message {
            lines.push(Line::from(format!("[~] {}", active)));
        } else {
            lines.push(Line::from("[ ] idle"));
        }
        if self.message_queue.is_empty() {
            lines.push(Line::from("[ ] queue empty"));
        } else {
            for queued in self.message_queue.iter() {
                lines.push(Line::from(format!(
                    "[ ] {}",
                    summarize_for_queue(&queued.text)
                )));
            }
        }
        lines.push(Line::from(""));
        lines.push(Line::from("/help /model /models /status /quit"));
        lines.push(Line::from("↑/↓/PgUp/PgDn scroll"));
        lines.push(Line::from("Ctrl+Y copy  Ctrl+U clear"));

        let right = Paragraph::new(lines).wrap(Wrap { trim: false });
        f.render_widget(right, right_inner);
    }

    fn input_line(&self) -> Line<'static> {
        let chars: Vec<char> = self.input_buffer.chars().collect();
        let mut spans = vec![Span::styled(
            "❯ ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )];

        for (idx, ch) in chars.iter().enumerate() {
            if idx == self.cursor_index {
                spans.push(Span::styled(
                    ch.to_string(),
                    Style::default()
                        .bg(Color::White)
                        .fg(Color::Black)
                        .add_modifier(Modifier::BOLD),
                ));
            } else {
                spans.push(Span::raw(ch.to_string()));
            }
        }

        if self.cursor_index >= chars.len() {
            spans.push(Span::styled(
                " ",
                Style::default().bg(Color::White).fg(Color::Black),
            ));
        }

        Line::from(spans)
    }

    fn input_status_line(&self, width: usize) -> Line<'static> {
        let workspace = compact_workspace_path(&self.startup_workspace);
        let token_left = self
            .token_left_percent()
            .map(|percent| format!("{}% left", percent))
            .unwrap_or_else(|| "n/a left".to_string());
        let base = if self.close_in_progress {
            let session_id = self
                .closing_session_id
                .clone()
                .unwrap_or_else(|| "unknown".to_string());
            format!("closing {} · flushing memory · {}", session_id, workspace)
        } else if self.in_flight {
            let motion = self.stripe_line(6);
            format!(
                "{} {} · {} · {}",
                motion, self.current_model, token_left, workspace
            )
        } else {
            format!("{} · {} · {}", self.current_model, token_left, workspace)
        };
        let mut text = if let Some((hint, expires_at)) = &self.status_hint {
            if Instant::now() < *expires_at {
                format!("{}  |  {}", base, hint)
            } else {
                base
            }
        } else {
            base
        };

        let max = width.saturating_sub(1);
        if text.chars().count() > max {
            text = text.chars().take(max).collect();
        }
        Line::from(text)
    }

    fn apply_session_snapshot(&mut self, session: &TalosSession) {
        self.session_id = Some(session.context.session_id.clone());
        self.current_model = session.config.llm.model.clone();
        self.session_usage = session.state.usage.clone();
    }

    fn token_left_percent(&self) -> Option<u8> {
        let context_window = self
            .available_model_info
            .iter()
            .find(|model| model.id == self.current_model)
            .map(|model| model.context_window)
            .filter(|window| *window > 0)?;
        if context_window == 0 {
            return None;
        }
        let used = self.session_usage.total_tokens.min(context_window);
        let used_percent = used
            .saturating_mul(100)
            .saturating_add(context_window.saturating_sub(1))
            / context_window;
        Some(100_u8.saturating_sub(used_percent as u8))
    }

    fn queue_panel_lines(&self, max_lines: usize) -> Vec<Line<'static>> {
        if self.message_queue.is_empty() {
            return Vec::new();
        }

        let mut preview: Vec<String> = vec![format!(
            "Queue {}/{}",
            self.message_queue.len(),
            MAX_QUEUE_LEN
        )];

        if self.in_flight {
            preview.push(format!(
                "├─ [~] {}",
                self.active_message
                    .clone()
                    .unwrap_or_else(|| "waiting for response".to_string())
            ));
        }

        for (idx, msg) in self.message_queue.iter().enumerate() {
            let is_last = idx + 1 == self.message_queue.len();
            let branch = if is_last { "└─" } else { "├─" };
            preview.push(format!(
                "{} [ ] {}. {}",
                branch,
                idx + 1,
                summarize_for_queue(&msg.text)
            ));
        }

        if preview.len() > max_lines && max_lines > 0 {
            preview.truncate(max_lines);
            if let Some(last) = preview.last_mut() {
                *last = "└─ ...".to_string();
            }
        }

        preview.into_iter().map(Line::from).collect()
    }

    fn block_content_lines(&self, block: &ChatBlock) -> Vec<Line<'static>> {
        match block.role {
            ChatRole::Assistant => render_markdown_lines(&block.content),
            ChatRole::Thinking => block
                .content
                .lines()
                .map(|line| {
                    Line::from(Span::styled(
                        line.to_string(),
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::DIM | Modifier::ITALIC),
                    ))
                })
                .collect(),
            _ => block
                .content
                .lines()
                .map(|line| Line::from(line.to_string()))
                .collect(),
        }
    }

    fn chat_card_lines(&self, block: &ChatBlock, content_width: usize) -> Vec<Line<'static>> {
        let (label, label_style, body_style, border_style) = self.chat_role_styles(&block.role);
        let mut lines = vec![Line::from(vec![
            Span::styled("┌ ", border_style),
            Span::styled(format!("{} ", label), label_style),
        ])];

        let mut content_lines = self.block_content_lines(block);
        if content_lines.is_empty() {
            content_lines.push(Line::from(String::new()));
        }

        for line in content_lines {
            for wrapped in wrap_line_visual(line, content_width) {
                lines.push(self.decorate_chat_line(wrapped, body_style, border_style));
            }
        }

        lines.push(Line::from(Span::styled("└", border_style)));
        lines
    }

    fn decorate_chat_line(
        &self,
        line: Line<'static>,
        body_style: Style,
        border_style: Style,
    ) -> Line<'static> {
        let mut spans: Vec<Span<'static>> = vec![Span::styled("│ ", border_style)];
        if line.spans.is_empty() {
            spans.push(Span::styled(" ", body_style));
            return Line::from(spans);
        }

        for span in line.spans {
            spans.push(Span::styled(
                span.content.to_string(),
                span.style.patch(body_style),
            ));
        }
        Line::from(spans)
    }

    fn chat_role_styles(&self, role: &ChatRole) -> (&'static str, Style, Style, Style) {
        match role {
            ChatRole::User => (
                "User",
                Style::default()
                    .fg(Color::Rgb(202, 214, 233))
                    .add_modifier(Modifier::BOLD),
                Style::default().fg(Color::White),
                Style::default().fg(Color::Rgb(178, 189, 203)),
            ),
            ChatRole::Thinking => (
                "Thinking",
                Style::default()
                    .bg(Color::Rgb(49, 55, 66))
                    .fg(Color::Rgb(228, 234, 245))
                    .add_modifier(Modifier::BOLD),
                Style::default()
                    .bg(Color::Rgb(31, 36, 45))
                    .fg(Color::Rgb(189, 195, 209))
                    .add_modifier(Modifier::DIM | Modifier::ITALIC),
                Style::default().fg(Color::Rgb(84, 92, 110)),
            ),
            ChatRole::Assistant => (
                "Assistant",
                Style::default()
                    .fg(Color::Rgb(233, 238, 247))
                    .add_modifier(Modifier::BOLD),
                Style::default().fg(Color::White),
                Style::default().fg(Color::Rgb(99, 109, 130)),
            ),
            ChatRole::System => (
                "System",
                Style::default()
                    .bg(Color::Rgb(88, 89, 108))
                    .fg(Color::Rgb(242, 243, 247))
                    .add_modifier(Modifier::BOLD),
                Style::default().bg(Color::Rgb(39, 40, 52)).fg(Color::Gray),
                Style::default().fg(Color::Rgb(112, 115, 139)),
            ),
        }
    }

    fn stripe_line(&self, width: usize) -> String {
        if width == 0 {
            return String::new();
        }
        let pattern = [">>>", ">>>", "===", "===", "...", "..."];
        let mut result = String::with_capacity(width);
        for idx in 0..width {
            let p = (idx + self.stripe_phase) % pattern.len();
            let ch = pattern[p].chars().next().unwrap_or(' ');
            result.push(ch);
        }
        result
    }

    #[cfg(test)]
    fn input_line_plain(&self) -> String {
        let chars: Vec<char> = self.input_buffer.chars().collect();
        let mut out = String::from("❯ ");
        for (idx, ch) in chars.iter().enumerate() {
            if idx == self.cursor_index {
                out.push('█');
            }
            out.push(*ch);
        }
        if self.cursor_index >= chars.len() {
            out.push('█');
        }
        out
    }
}

fn canonical_event_label(event_type: &TalosEventType) -> &'static str {
    match event_type {
        TalosEventType::MessageStarted => "message.started",
        TalosEventType::MessageReasoningDelta => "message.reasoning.delta",
        TalosEventType::MessageOutputDelta => "message.output.delta",
        TalosEventType::MessageToolCallStarted => "message.tool_call.started",
        TalosEventType::MessageToolCallCompleted => "message.tool_call.completed",
        TalosEventType::MessageCompleted => "message.completed",
        TalosEventType::MessageError => "message.error",
    }
}

fn extract_text_from_completed_event(data: &serde_json::Value) -> Option<String> {
    for key in ["output_text", "text", "final_text", "delta"] {
        if let Some(text) = data.get(key).and_then(|v| v.as_str()) {
            if !text.is_empty() {
                return Some(text.to_string());
            }
        }
    }

    let mut chunks = Vec::new();
    if let Some(items) = data.get("output").and_then(|v| v.as_array()) {
        for item in items {
            let item_type = item
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            if item_type != "output_text" {
                continue;
            }
            if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                if !text.is_empty() {
                    chunks.push(text);
                }
            }
        }
    }

    if chunks.is_empty() {
        None
    } else {
        Some(chunks.join(""))
    }
}

fn extract_usage_from_completed_event(data: &serde_json::Value) -> Option<TalosUsage> {
    let usage = data.get("usage")?;
    match serde_json::from_value::<TalosUsage>(usage.clone()) {
        Ok(parsed) => Some(parsed),
        Err(err) => {
            warn!(error = %err, usage = %usage, "failed to parse usage payload from completed event");
            None
        }
    }
}

fn wrap_line_visual(line: Line<'static>, max_width: usize) -> Vec<Line<'static>> {
    let width_limit = max_width.max(1);
    if line.spans.is_empty() {
        return vec![Line::from(String::new())];
    }

    let mut wrapped_lines: Vec<Line<'static>> = Vec::new();
    let mut current_spans: Vec<Span<'static>> = Vec::new();
    let mut current_width = 0usize;

    for span in line.spans {
        let style = span.style;
        let mut segment = String::new();
        let mut segment_width = 0usize;

        for ch in span.content.chars() {
            let ch_width = char_display_width(ch);
            let next_width = current_width + segment_width + ch_width;
            if next_width > width_limit && (current_width + segment_width) > 0 {
                if !segment.is_empty() {
                    current_spans.push(Span::styled(std::mem::take(&mut segment), style));
                    segment_width = 0;
                }
                wrapped_lines.push(Line::from(std::mem::take(&mut current_spans)));
                current_width = 0;
            }
            segment.push(ch);
            segment_width += ch_width;
        }

        if !segment.is_empty() {
            current_spans.push(Span::styled(segment, style));
            current_width += segment_width;
        }
    }

    if current_spans.is_empty() {
        wrapped_lines.push(Line::from(String::new()));
    } else {
        wrapped_lines.push(Line::from(current_spans));
    }
    wrapped_lines
}

fn char_display_width(ch: char) -> usize {
    if ch == '\t' {
        4
    } else {
        UnicodeWidthChar::width(ch).unwrap_or(0)
    }
}

fn summarize_for_queue(input: &str) -> String {
    const MAX: usize = 42;
    let trimmed = input.trim();
    let mut chars = trimmed.chars();
    let head: String = chars.by_ref().take(MAX).collect();
    if chars.next().is_some() {
        format!("{}...", head)
    } else {
        head
    }
}

fn compact_workspace_path(path: &Path) -> String {
    let raw = path.display().to_string();
    if let Ok(home) = env::var("HOME") {
        if raw.starts_with(&home) {
            return raw.replacen(&home, "~", 1);
        }
    }
    raw
}

fn copy_to_system_clipboard(text: &str) -> Result<(), String> {
    fn try_copy(command: &str, args: &[&str], text: &str) -> Result<(), String> {
        let mut child = Command::new(command)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?;

        if let Some(stdin) = child.stdin.as_mut() {
            stdin
                .write_all(text.as_bytes())
                .map_err(|e| e.to_string())?;
        }

        let status = child.wait().map_err(|e| e.to_string())?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("{} exited with {}", command, status))
        }
    }

    let candidates: [(&str, &[&str]); 3] = [
        ("pbcopy", &[]),
        ("wl-copy", &[]),
        ("xclip", &["-selection", "clipboard"]),
    ];
    for (cmd, args) in candidates {
        if try_copy(cmd, args, text).is_ok() {
            return Ok(());
        }
    }

    Err("no clipboard command available (pbcopy/wl-copy/xclip)".to_string())
}

fn build_welcome_card(model: &str, workspace: &Path) -> String {
    const CARD_WIDTH: usize = 50;

    fn card_line(content: &str) -> String {
        let mut inner: String = content.chars().take(CARD_WIDTH).collect();
        let used = inner.chars().count();
        if used < CARD_WIDTH {
            inner.push_str(&" ".repeat(CARD_WIDTH - used));
        }
        format!("│{}│", inner)
    }

    let workspace_line = format!("workspace: {}", workspace.display());
    [
        "╭──────────────────────────────────────────────────╮".to_string(),
        card_line(&format!(" >_ Talos TUI ({})", APP_VERSION)),
        card_line(""),
        card_line(&format!(" model:     {:<12} /model to change", model)),
        card_line(&workspace_line),
        "╰──────────────────────────────────────────────────╯".to_string(),
    ]
    .join("\n")
}

fn render_markdown_lines(markdown: &str) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut in_code_block = false;

    for raw in markdown.lines() {
        let trimmed = raw.trim_start();
        if trimmed.starts_with("```") {
            in_code_block = !in_code_block;
            lines.push(Line::from(Span::styled(
                raw.to_string(),
                Style::default().fg(Color::DarkGray),
            )));
            continue;
        }

        if in_code_block {
            lines.push(Line::from(Span::styled(
                raw.to_string(),
                Style::default().fg(Color::LightGreen),
            )));
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("### ") {
            lines.push(Line::from(Span::styled(
                rest.to_string(),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )));
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("## ") {
            lines.push(Line::from(Span::styled(
                rest.to_string(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )));
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("# ") {
            lines.push(Line::from(Span::styled(
                rest.to_string(),
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            )));
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("> ") {
            lines.push(Line::from(Span::styled(
                rest.to_string(),
                Style::default()
                    .fg(Color::Gray)
                    .add_modifier(Modifier::ITALIC),
            )));
            continue;
        }

        lines.push(Line::from(raw.to_string()));
    }

    if lines.is_empty() {
        lines.push(Line::from(String::new()));
    }

    lines
}

fn byte_index_at_char(text: &str, char_index: usize) -> usize {
    if char_index == 0 {
        return 0;
    }
    text.char_indices()
        .nth(char_index)
        .map(|(byte, _)| byte)
        .unwrap_or_else(|| text.len())
}

/// Run TUI with embedded agent mode.
pub async fn run_tui_app(agent: Arc<dyn Agent>) -> Result<(), Box<dyn Error>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;

    let mut app = TuiApp::new_with_agent(agent);
    let run_result = run_event_loop(&mut terminal, &mut app).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    run_result
}

/// Run TUI with gateway mode.
pub async fn run_tui_app_with_client(client: Arc<TalosClient>) -> Result<(), Box<dyn Error>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;

    let mut app = TuiApp::new_with_client(client);
    app.initialize_remote().await;
    let run_result = run_event_loop(&mut terminal, &mut app).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    run_result
}

async fn run_event_loop<B: Backend>(
    terminal: &mut Terminal<B>,
    app: &mut TuiApp,
) -> Result<(), Box<dyn Error>> {
    loop {
        app.on_tick().await;
        if app.should_exit {
            return Ok(());
        }
        terminal.draw(|frame| app.render_ui(frame))?;

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    if app.handle_input(key).await? {
                        return Ok(());
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Mutex;
    use talos_agent::MockAgent;

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

    impl MockGatewayClient {
        fn new() -> Self {
            Self {
                state: Arc::new(Mutex::new(MockGatewayState {
                    models: vec![
                        mock_model_info("gpt-5.3 high", 128_000),
                        mock_model_info("gpt-5.3 medium", 128_000),
                    ],
                    session_id: "sess-test".to_string(),
                    current_model: "gpt-5.3 high".to_string(),
                    updated_model: None,
                    stream_counter: 0,
                    connect_delay_ms: 0,
                    create_session_delay_ms: 0,
                    stream_failures_remaining: 0,
                    close_session_delay_ms: 0,
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

        async fn create_session(&self, _params: &SessionCreateParams) -> TalosResult<TalosSession> {
            let delay = self.state.lock().unwrap().create_session_delay_ms;
            if delay > 0 {
                tokio::time::sleep(Duration::from_millis(delay)).await;
            }
            let state = self.state.lock().unwrap();
            Ok(TalosSession {
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

        async fn update_session_model(
            &self,
            _session_id: &str,
            model: &str,
        ) -> TalosResult<TalosSession> {
            let mut state = self.state.lock().unwrap();
            state.current_model = model.to_string();
            state.updated_model = Some(model.to_string());
            Ok(TalosSession {
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

            tx.send(TalosEvent {
                event_type: TalosEventType::MessageStarted,
                message_id: message_id.clone(),
                session_id: session_id.to_string(),
                sequence: 0,
                data: json!({"model":"gpt-5.3 high"}),
            })
            .await
            .unwrap();

            tx.send(TalosEvent {
                event_type: TalosEventType::MessageOutputDelta,
                message_id: message_id.clone(),
                session_id: session_id.to_string(),
                sequence: 1,
                data: json!({"delta":"# Title\n"}),
            })
            .await
            .unwrap();

            tx.send(TalosEvent {
                event_type: TalosEventType::MessageOutputDelta,
                message_id: message_id.clone(),
                session_id: session_id.to_string(),
                sequence: 2,
                data: json!({"delta":"- item\n"}),
            })
            .await
            .unwrap();

            tx.send(TalosEvent {
                event_type: TalosEventType::MessageOutputDelta,
                message_id: message_id.clone(),
                session_id: session_id.to_string(),
                sequence: 3,
                data: json!({"delta":format!("reply({})", prompt)}),
            })
            .await
            .unwrap();

            tx.send(TalosEvent {
                event_type: TalosEventType::MessageCompleted,
                message_id,
                session_id: session_id.to_string(),
                sequence: 4,
                data: json!({"stop_reason":"end_turn"}),
            })
            .await
            .unwrap();

            drop(tx);
            Ok(rx)
        }
    }

    fn create_embedded_app() -> TuiApp {
        let agent = Arc::new(MockAgent::new());
        TuiApp::new_with_agent(agent)
    }

    fn create_remote_app() -> TuiApp {
        let client: Arc<dyn GatewayClient> = Arc::new(MockGatewayClient::new());
        TuiApp::new_with_gateway_client(client)
    }

    fn create_remote_app_with_connect_delay(delay_ms: u64) -> TuiApp {
        let client: Arc<dyn GatewayClient> =
            Arc::new(MockGatewayClient::with_connect_delay(delay_ms));
        TuiApp::new_with_gateway_client(client)
    }

    fn create_remote_app_with_session_delay(delay_ms: u64) -> TuiApp {
        let client: Arc<dyn GatewayClient> =
            Arc::new(MockGatewayClient::with_session_delay(delay_ms));
        TuiApp::new_with_gateway_client(client)
    }

    fn create_remote_app_with_stream_failures(failures: u8) -> TuiApp {
        let client: Arc<dyn GatewayClient> =
            Arc::new(MockGatewayClient::with_stream_failures(failures));
        TuiApp::new_with_gateway_client(client)
    }

    fn create_remote_app_with_close_delay(delay_ms: u64) -> TuiApp {
        let client: Arc<dyn GatewayClient> =
            Arc::new(MockGatewayClient::with_close_delay(delay_ms));
        TuiApp::new_with_gateway_client(client)
    }

    fn set_test_model_info(app: &mut TuiApp, model_id: &str, context_window: u32) {
        app.available_model_info = vec![mock_model_info(model_id, context_window)];
        app.available_models = vec![model_id.to_string()];
    }

    #[test]
    fn test_input_cursor_and_backspace() {
        let mut app = create_embedded_app();
        app.insert_char('a');
        app.insert_char('b');
        app.cursor_index = 1;
        app.handle_backspace();
        assert_eq!(app.get_input_buffer(), "b");
        assert_eq!(app.input_line_plain(), "❯ █b");
    }

    #[tokio::test]
    async fn test_remote_initialize_creates_session_and_models() {
        let mut app = create_remote_app();
        app.initialize_remote().await;
        assert_eq!(app.session_id.as_deref(), Some("sess-test"));
        assert!(app.available_models.iter().any(|m| m == "gpt-5.3 high"));
        assert_eq!(app.connection_status, "connected");
    }

    #[tokio::test]
    async fn test_model_command_updates_remote_session_model() {
        let mut app = create_remote_app();
        app.initialize_remote().await;
        app.input_buffer = "/model gpt-5.3 medium".to_string();
        app.cursor_index = app.input_buffer.chars().count();
        let should_quit = app.submit_input().await.unwrap();
        for _ in 0..10 {
            app.on_tick().await;
            tokio::task::yield_now().await;
            if app.pending_command_update.is_none() {
                break;
            }
        }
        assert!(!should_quit);
        assert_eq!(app.current_model, "gpt-5.3 medium");
    }

    #[tokio::test]
    async fn test_send_message_stream_updates_assistant_block() {
        let mut app = create_remote_app();
        app.initialize_remote().await;
        assert!(app.send_user_message("hello".to_string()).await.unwrap());

        for _ in 0..40 {
            app.on_tick().await;
            tokio::task::yield_now().await;
            if !app.in_flight && app.message_queue.is_empty() {
                break;
            }
        }

        let assistant = app
            .chat_blocks
            .iter()
            .find(|b| b.role == ChatRole::Assistant)
            .map(|b| b.content.clone())
            .unwrap_or_default();
        assert!(assistant.contains("# Title"));
        assert!(assistant.contains("- item"));
        let user_messages = app
            .chat_blocks
            .iter()
            .filter(|b| b.role == ChatRole::User)
            .map(|b| b.content.clone())
            .collect::<Vec<_>>();
        assert_eq!(user_messages, vec!["hello".to_string()]);
    }

    #[tokio::test]
    async fn test_waiting_hint_is_visible_before_first_delta() {
        let mut app = create_remote_app();
        app.initialize_remote().await;
        assert!(app.send_user_message("hello".to_string()).await.unwrap());

        assert!(app.in_flight);
        assert!(!app.received_output_delta);
        let thinking = app
            .chat_blocks
            .iter()
            .find(|b| b.role == ChatRole::Thinking)
            .map(|b| b.content.clone())
            .unwrap_or_default();
        assert!(thinking.contains("waiting for model response"));
    }

    #[tokio::test]
    async fn test_modes_alias_works_as_models() {
        let mut app = create_remote_app();
        app.initialize_remote().await;
        app.input_buffer = "/modes".to_string();
        app.cursor_index = app.input_buffer.chars().count();
        let _ = app.submit_input().await.unwrap();
        for _ in 0..10 {
            app.on_tick().await;
            tokio::task::yield_now().await;
            if app.pending_command_update.is_none() {
                break;
            }
        }
        assert!(app
            .chat_history
            .iter()
            .any(|line| line.contains("Available models")));
    }

    #[tokio::test]
    async fn test_queue_limit_and_serial_dispatch() {
        let mut app = create_remote_app();
        app.initialize_remote().await;

        assert!(app.send_user_message("m1".to_string()).await.unwrap());
        assert!(app.send_user_message("m2".to_string()).await.unwrap());
        assert!(app.send_user_message("m3".to_string()).await.unwrap());
        assert!(app.send_user_message("m4".to_string()).await.unwrap());
        assert!(!app.send_user_message("m5".to_string()).await.unwrap());

        assert_eq!(app.message_queue.len(), 3);
        assert!(app.in_flight);

        for _ in 0..120 {
            app.on_tick().await;
            tokio::task::yield_now().await;
            if !app.in_flight && app.message_queue.is_empty() {
                break;
            }
        }

        assert!(app.message_queue.is_empty());
        assert!(!app.in_flight);
        let user_messages = app
            .chat_blocks
            .iter()
            .filter(|b| b.role == ChatRole::User)
            .map(|b| b.content.clone())
            .collect::<Vec<_>>();
        assert_eq!(user_messages, vec!["m1", "m2", "m3", "m4"]);
    }

    #[tokio::test]
    async fn test_queued_messages_do_not_add_system_noise_blocks() {
        let mut app = create_remote_app();
        app.initialize_remote().await;

        assert!(app.send_user_message("m1".to_string()).await.unwrap());
        let before = app.chat_blocks.len();
        assert!(app.send_user_message("m2".to_string()).await.unwrap());
        assert_eq!(app.chat_blocks.len(), before);
    }

    #[tokio::test]
    async fn test_submit_input_queues_without_adding_user_block() {
        let mut app = create_remote_app();
        app.initialize_remote().await;

        app.input_buffer = "m1".to_string();
        app.cursor_index = app.input_buffer.chars().count();
        let _ = app.submit_input().await.unwrap();

        app.input_buffer = "m2".to_string();
        app.cursor_index = app.input_buffer.chars().count();
        let before = app.chat_blocks.len();
        let _ = app.submit_input().await.unwrap();

        assert_eq!(app.chat_blocks.len(), before);
        assert_eq!(app.message_queue.len(), 1);
        assert_eq!(
            app.message_queue.front().map(|msg| msg.text.as_str()),
            Some("m2")
        );
        let user_messages = app
            .chat_blocks
            .iter()
            .filter(|b| b.role == ChatRole::User)
            .map(|b| b.content.as_str())
            .collect::<Vec<_>>();
        assert_eq!(user_messages, vec!["m1"]);
    }

    #[tokio::test]
    async fn test_submit_input_keeps_text_when_queue_is_full() {
        let mut app = create_remote_app();
        app.initialize_remote().await;

        assert!(app.send_user_message("m1".to_string()).await.unwrap());
        assert!(app.send_user_message("m2".to_string()).await.unwrap());
        assert!(app.send_user_message("m3".to_string()).await.unwrap());
        assert!(app.send_user_message("m4".to_string()).await.unwrap());
        assert_eq!(app.message_queue.len(), 3);

        let before_blocks = app.chat_blocks.len();
        app.input_buffer = "overflow".to_string();
        app.cursor_index = app.input_buffer.chars().count();

        let should_quit = app.submit_input().await.unwrap();
        assert!(!should_quit);
        assert_eq!(app.chat_blocks.len(), before_blocks);
        assert_eq!(app.input_buffer, "overflow");
        assert_eq!(app.message_queue.len(), 3);
        assert!(app
            .status_hint
            .as_ref()
            .map(|(msg, _)| msg.contains("Queue is full"))
            .unwrap_or(false));
    }

    #[tokio::test]
    async fn test_timeout_uses_status_hint_not_chat_block() {
        let mut app = create_remote_app();
        app.initialize_remote().await;
        app.in_flight = true;
        app.active_message_raw = Some("slow request".to_string());
        app.active_retry_count = 1;
        app.last_stream_event_at =
            Some(Instant::now() - STREAM_STALL_TIMEOUT - Duration::from_millis(1));

        let before_blocks = app.chat_blocks.len();
        app.on_tick().await;

        assert_eq!(app.chat_blocks.len(), before_blocks);
        assert!(app
            .status_hint
            .as_ref()
            .map(|(msg, _)| msg.contains("timed out"))
            .unwrap_or(false));
    }

    #[tokio::test]
    async fn test_stale_message_events_are_ignored() {
        let mut app = create_remote_app();
        app.initialize_remote().await;
        app.in_flight = true;
        app.active_stream_message_id = Some("msg-current".to_string());
        app.last_stream_sequence = Some(3);
        app.active_assistant_idx = Some(app.push_block(ChatRole::Assistant, String::new()));

        let stale_message_id = format!(
            "{}-stale",
            app.active_stream_message_id
                .as_deref()
                .expect("active stream message id should exist")
        );
        app.handle_stream_event(TalosEvent {
            event_type: TalosEventType::MessageOutputDelta,
            message_id: stale_message_id,
            session_id: "sess-test".to_string(),
            sequence: 4,
            data: json!({"delta":"stale"}),
        });

        let assistant = app
            .chat_blocks
            .iter()
            .find(|b| b.role == ChatRole::Assistant)
            .map(|b| b.content.clone())
            .unwrap_or_default();
        assert!(!assistant.contains("stale"));
        assert!(app
            .right_panel_events
            .iter()
            .any(|line| line.contains("ignored stale event")));
    }

    #[tokio::test]
    async fn test_message_completed_can_fill_assistant_without_output_delta() {
        let mut app = create_remote_app();
        app.initialize_remote().await;
        app.in_flight = true;
        app.received_output_delta = false;
        app.active_assistant_idx = Some(app.push_block(ChatRole::Assistant, String::new()));
        app.session_id = Some("sess-test".to_string());

        app.handle_stream_event(TalosEvent {
            event_type: TalosEventType::MessageCompleted,
            message_id: "msg-new".to_string(),
            session_id: "sess-test".to_string(),
            sequence: 1,
            data: json!({"output_text":"final answer from completed"}),
        });

        let assistant = app
            .chat_blocks
            .iter()
            .filter(|b| b.role == ChatRole::Assistant)
            .map(|b| b.content.clone())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(assistant.contains("final answer from completed"));
    }

    #[tokio::test]
    async fn test_stream_event_enables_follow_bottom_mode() {
        let mut app = create_remote_app();
        app.initialize_remote().await;
        app.in_flight = true;
        app.chat_manual_scroll = true;
        app.session_id = Some("sess-test".to_string());

        app.handle_stream_event(TalosEvent {
            event_type: TalosEventType::MessageOutputDelta,
            message_id: "msg-follow".to_string(),
            session_id: "sess-test".to_string(),
            sequence: 1,
            data: json!({"delta":"hello"}),
        });

        assert!(!app.chat_manual_scroll);
    }

    #[test]
    fn test_input_status_line_uses_dynamic_token_left_percent() {
        let mut app = create_embedded_app();
        set_test_model_info(&mut app, "qwen-max", 32_768);
        app.current_model = "qwen-max".to_string();
        app.session_usage = TalosUsage {
            input_tokens: 964,
            output_tokens: 16,
            total_tokens: 980,
        };

        let text = line_plain_text(&app.input_status_line(120));

        assert!(text.contains("97% left"));
        assert!(!text.contains("48% left"));
    }

    #[tokio::test]
    async fn test_message_completed_updates_session_usage() {
        let mut app = create_remote_app();
        app.initialize_remote().await;
        set_test_model_info(&mut app, "qwen-max", 32_768);
        app.current_model = "qwen-max".to_string();
        app.in_flight = true;
        app.received_output_delta = false;
        app.active_assistant_idx = Some(app.push_block(ChatRole::Assistant, String::new()));
        app.session_id = Some("sess-test".to_string());

        app.handle_stream_event(TalosEvent {
            event_type: TalosEventType::MessageCompleted,
            message_id: "msg-usage".to_string(),
            session_id: "sess-test".to_string(),
            sequence: 1,
            data: json!({
                "output_text":"done",
                "usage":{
                    "input_tokens":964,
                    "output_tokens":16,
                    "total_tokens":980
                }
            }),
        });

        assert_eq!(app.session_usage.total_tokens, 980);
        assert_eq!(app.token_left_percent(), Some(97));
    }

    #[test]
    fn test_chat_card_wrapped_lines_keep_border_prefix() {
        let app = create_embedded_app();
        let block = ChatBlock::new(
            ChatRole::Assistant,
            "这是一个非常长的回答，用于测试换行后是否仍然带有左侧边框前缀。".to_string(),
        );
        let lines = app.chat_card_lines(&block, 12);
        let content_lines: Vec<String> = lines
            .iter()
            .skip(1)
            .take(lines.len().saturating_sub(2))
            .map(line_plain_text)
            .collect();
        assert!(!content_lines.is_empty());
        assert!(content_lines.iter().all(|line| line.starts_with("│ ")));
    }

    #[test]
    fn test_chat_card_wrapped_lines_never_exceed_visual_width() {
        let app = create_embedded_app();
        let body_width = 10usize;
        let block = ChatBlock::new(
            ChatRole::Assistant,
            "这是一段很长很长的内容，用于验证每一行都不会超过内容窗口宽度并触发终端二次换行。"
                .to_string(),
        );
        let lines = app.chat_card_lines(&block, body_width);

        let max_line_width = body_width + 2;
        for line in lines.iter().skip(1).take(lines.len().saturating_sub(2)) {
            let width = line_display_width(line);
            assert!(
                width <= max_line_width,
                "line width {} exceeds max {}",
                width,
                max_line_width
            );
        }
    }

    #[test]
    fn test_user_assistant_styles_have_no_background() {
        let app = create_embedded_app();
        let (_, user_label, user_body, _) = app.chat_role_styles(&ChatRole::User);
        let (_, assistant_label, assistant_body, _) = app.chat_role_styles(&ChatRole::Assistant);
        let (_, thinking_label, thinking_body, _) = app.chat_role_styles(&ChatRole::Thinking);

        assert!(user_label.bg.is_none());
        assert!(user_body.bg.is_none());
        assert!(assistant_label.bg.is_none());
        assert!(assistant_body.bg.is_none());
        assert!(thinking_label.bg.is_some());
        assert!(thinking_body.bg.is_some());
    }

    fn line_plain_text(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>()
    }

    fn line_display_width(line: &Line<'_>) -> usize {
        line.spans
            .iter()
            .flat_map(|span| span.content.chars())
            .map(char_display_width)
            .sum()
    }

    #[tokio::test]
    async fn test_multiple_questions_keep_reply_order() {
        let mut app = create_remote_app();
        app.initialize_remote().await;

        assert!(app.send_user_message("q1".to_string()).await.unwrap());
        assert!(app.send_user_message("q2".to_string()).await.unwrap());
        assert!(app.send_user_message("q3".to_string()).await.unwrap());
        assert!(app.send_user_message("q4".to_string()).await.unwrap());

        for _ in 0..240 {
            app.on_tick().await;
            tokio::task::yield_now().await;
            if !app.in_flight && app.message_queue.is_empty() {
                break;
            }
        }

        let joined = app
            .chat_blocks
            .iter()
            .filter(|b| b.role == ChatRole::Assistant)
            .map(|b| b.content.clone())
            .collect::<Vec<_>>()
            .join("\n");

        let p1 = joined.find("reply(q1)").unwrap();
        let p2 = joined.find("reply(q2)").unwrap();
        let p3 = joined.find("reply(q3)").unwrap();
        let p4 = joined.find("reply(q4)").unwrap();
        assert!(p1 < p2 && p2 < p3 && p3 < p4);
    }

    #[tokio::test]
    async fn test_send_is_non_blocking_when_stream_connect_is_slow() {
        let mut app = create_remote_app_with_connect_delay(500);
        app.initialize_remote().await;

        let started = Instant::now();
        assert!(app.send_user_message("slow-1".to_string()).await.unwrap());
        let elapsed = started.elapsed();

        assert!(elapsed < Duration::from_millis(150));
        assert!(app.in_flight);
        assert!(app.pending_stream_connect.is_some());

        assert!(app.send_user_message("slow-2".to_string()).await.unwrap());
        assert_eq!(app.message_queue.len(), 1);
    }

    #[tokio::test]
    async fn test_send_is_non_blocking_when_session_create_is_slow() {
        let mut app = create_remote_app_with_session_delay(500);

        let started = Instant::now();
        assert!(app
            .send_user_message("slow-create".to_string())
            .await
            .unwrap());
        let elapsed = started.elapsed();

        assert!(elapsed < Duration::from_millis(150));
        assert_eq!(app.message_queue.len(), 1);
        assert!(app.pending_session_connect.is_some());
        assert!(app.chat_blocks.iter().all(|b| b.content != "slow-create"));
    }

    #[tokio::test]
    async fn test_failed_queued_message_is_dropped_after_one_retry() {
        let mut app = create_remote_app_with_stream_failures(2);
        app.initialize_remote().await;

        assert!(app.send_user_message("drop-me".to_string()).await.unwrap());
        assert!(app.send_user_message("next-one".to_string()).await.unwrap());

        for _ in 0..120 {
            app.on_tick().await;
            tokio::task::yield_now().await;
            if !app.in_flight && app.message_queue.is_empty() {
                break;
            }
        }

        assert!(app.message_queue.is_empty());
        let user_messages = app
            .chat_blocks
            .iter()
            .filter(|b| b.role == ChatRole::User)
            .map(|b| b.content.clone())
            .collect::<Vec<_>>();
        assert_eq!(user_messages, vec!["next-one".to_string()]);
        let assistant_text = app
            .chat_blocks
            .iter()
            .filter(|b| b.role == ChatRole::Assistant)
            .map(|b| b.content.clone())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(assistant_text.contains("reply(next-one)"));
        assert!(app
            .right_panel_events
            .iter()
            .any(|line| line.contains("queued dispatch dropped")));
    }

    #[tokio::test]
    async fn test_quit_enters_closing_state_before_remote_close_finishes() {
        let mut app = create_remote_app_with_close_delay(500);
        app.initialize_remote().await;
        app.input_buffer = "/quit".to_string();
        app.cursor_index = app.input_buffer.chars().count();

        let should_quit = app.submit_input().await.unwrap();

        assert!(!should_quit);
        assert!(app.close_in_progress);
        assert!(app.pending_close_update.is_some());
        assert_eq!(app.connection_status, "closing");
        assert!(app.input_status_line(120).to_string().contains("closing"));
    }

    #[tokio::test]
    async fn test_quit_exits_after_remote_close_completes() {
        let mut app = create_remote_app();
        app.initialize_remote().await;
        app.input_buffer = "/quit".to_string();
        app.cursor_index = app.input_buffer.chars().count();

        let should_quit = app.submit_input().await.unwrap();
        assert!(!should_quit);

        for _ in 0..20 {
            app.on_tick().await;
            tokio::task::yield_now().await;
            if app.should_exit {
                break;
            }
        }

        assert!(app.should_exit);
        assert!(!app.close_in_progress);
        assert!(app
            .right_panel_events
            .iter()
            .any(|line| line.contains("session close completed")));
    }

    #[tokio::test]
    async fn test_quit_timeout_requires_second_quit_to_force_exit() {
        let mut app = create_remote_app_with_close_delay(5_000);
        app.initialize_remote().await;
        app.input_buffer = "/quit".to_string();
        app.cursor_index = app.input_buffer.chars().count();
        let _ = app.submit_input().await.unwrap();

        app.close_started_at =
            Some(Instant::now() - SESSION_CLOSE_TIMEOUT - Duration::from_millis(1));
        app.on_tick().await;

        assert!(app.close_in_progress);
        assert!(app.close_timeout_reported);
        assert!(app
            .status_hint
            .as_ref()
            .map(|(msg, _)| msg.contains("Press /quit again"))
            .unwrap_or(false));

        app.input_buffer = "/quit".to_string();
        app.cursor_index = app.input_buffer.chars().count();
        let should_quit = app.submit_input().await.unwrap();
        assert!(!should_quit);
        assert!(app.should_exit);
        assert!(app
            .right_panel_events
            .iter()
            .any(|line| line.contains("session force exit")));
    }

    #[tokio::test]
    async fn test_closing_state_blocks_new_messages() {
        let mut app = create_remote_app_with_close_delay(5_000);
        app.initialize_remote().await;
        app.close_remote_session_command().unwrap();

        let accepted = app.send_user_message("blocked".to_string()).await.unwrap();

        assert!(!accepted);
        assert!(app.message_queue.is_empty());
        assert!(app
            .status_hint
            .as_ref()
            .map(|(msg, _)| msg.contains("closing"))
            .unwrap_or(false));
    }
}
