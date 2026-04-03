//! Talos Terminal User Interface (TUI)
//!
//! Gateway mode supports real session APIs and SSE streaming.

mod markdown;
pub mod run;
mod utils;

use markdown::*;
pub use run::run_tui_app_with_client;
use utils::*;

use async_trait::async_trait;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Clear, List, ListItem, ListState, Paragraph, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Wrap,
    },
    Frame,
};
use std::{
    collections::{HashMap, VecDeque},
    env,
    error::Error,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use talos_client::{
    ModelInfo, SessionCreateParams, TalosClient, TalosContentItem, TalosContext, TalosEvent,
    TalosEventPayload, TalosInputItem, TalosMessageRequest, TalosSession,
};
use talos_error::{TalosError, TalosResult};
use talos_types::talos::{TalosEventType, TalosUsage};
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TryRecvError;
use tracing::error;

const DEFAULT_MODEL: &str = "qwen-plus";
const MAX_RIGHT_EVENTS: usize = 120;
const MAX_QUEUE_LEN: usize = 3;
const STREAM_STALL_TIMEOUT: Duration = Duration::from_secs(60);
const SESSION_CLOSE_TIMEOUT: Duration = Duration::from_secs(4);
const SPINNER_FRAMES: &[char] = &['|', '/', '-', '\\'];
const CHAT_CONTENT_PADDING_LEFT: u16 = 2;

#[derive(Debug, Clone)]
struct CommandSuggestion {
    name: String,
    insert_text: String,
    display_text: String,
    description: String,
}

impl CommandSuggestion {
    fn all() -> Vec<Self> {
        vec![
            CommandSuggestion {
                name: "help".to_string(),
                insert_text: "/help".to_string(),
                display_text: "/help".to_string(),
                description: "Show help and available commands".to_string(),
            },
            CommandSuggestion {
                name: "model".to_string(),
                insert_text: "/model".to_string(),
                display_text: "/model [id]".to_string(),
                description: "Switch model (no args: show picker)".to_string(),
            },
            CommandSuggestion {
                name: "models".to_string(),
                insert_text: "/models".to_string(),
                display_text: "/models".to_string(),
                description: "List all available models".to_string(),
            },
            CommandSuggestion {
                name: "history".to_string(),
                insert_text: "/history".to_string(),
                display_text: "/history".to_string(),
                description: "Open session history panel".to_string(),
            },
            CommandSuggestion {
                name: "sessions".to_string(),
                insert_text: "/sessions".to_string(),
                display_text: "/sessions [1-9]".to_string(),
                description: "Quick session switch (supports digit shortcuts)".to_string(),
            },
            CommandSuggestion {
                name: "status".to_string(),
                insert_text: "/status".to_string(),
                display_text: "/status".to_string(),
                description: "Show connection status".to_string(),
            },
            CommandSuggestion {
                name: "quit".to_string(),
                insert_text: "/quit".to_string(),
                display_text: "/quit".to_string(),
                description: "Exit application".to_string(),
            },
        ]
    }

    fn filter(query: &str) -> Vec<CommandSuggestion> {
        let normalized = query.trim_start_matches('/');
        Self::all()
            .into_iter()
            .filter(|cmd| {
                cmd.name.starts_with(normalized)
                    || cmd
                        .insert_text
                        .trim_start_matches('/')
                        .starts_with(normalized)
                    || cmd
                        .display_text
                        .trim_start_matches('/')
                        .starts_with(normalized)
            })
            .collect()
    }
}

/// Application states.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppState {
    Chat,
    History,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ChatRole {
    User,
    Thinking,
    Assistant,
    System,
    Tool,
}

impl ChatRole {
    fn as_str(&self) -> &'static str {
        match self {
            Self::User => "User",
            Self::Thinking => "Thinking",
            Self::Assistant => "Assistant",
            Self::System => "System",
            Self::Tool => "Tool",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ToolCallStatus {
    Running,
    Completed { exit_ok: bool },
}

#[derive(Debug, Clone)]
struct ChatBlock {
    role: ChatRole,
    content: String,
    // Tool-specific (Some only when role == Tool)
    #[allow(dead_code)]
    tool_call_id: Option<String>,
    tool_name: Option<String>,
    tool_input: Option<serde_json::Value>,
    tool_status: Option<ToolCallStatus>,
    expanded: bool,
    timing_ms: Option<u64>,
}

impl ChatBlock {
    fn new(role: ChatRole, content: String) -> Self {
        Self {
            role,
            content,
            tool_call_id: None,
            tool_name: None,
            tool_input: None,
            tool_status: None,
            expanded: false,
            timing_ms: None,
        }
    }

    fn new_tool_call(
        call_id: String,
        tool_name: String,
        tool_input: serde_json::Value,
        timing_ms: Option<u64>,
    ) -> Self {
        Self {
            role: ChatRole::Tool,
            content: String::new(),
            tool_call_id: Some(call_id),
            tool_name: Some(tool_name),
            tool_input: Some(tool_input),
            tool_status: Some(ToolCallStatus::Running),
            expanded: false,
            timing_ms,
        }
    }
}

#[derive(Debug, Clone)]
struct CopyProjection {
    text: String,
    display_start_col: usize,
}

#[derive(Debug, Clone)]
struct RenderedChatLine {
    display: Line<'static>,
    copy_projection: Option<CopyProjection>,
}

#[derive(Debug, Default, Clone)]
struct ToolRenderData {
    summary: String,
    detail_lines: Vec<String>,
    preview_lines: Vec<String>,
    output_lines: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SelectionPoint {
    line_index: usize,
    column: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ChatSelection {
    anchor: SelectionPoint,
    focus: SelectionPoint,
}

#[derive(Debug, Clone)]
struct QueuedMessage {
    text: String,
    dispatch_attempts: u8,
}

#[derive(Debug, Clone)]
struct PendingApprovalState {
    call_id: String,
    tool_name: String,
    prompt: String,
    #[allow(dead_code)]
    arguments: serde_json::Value,
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
    HistoryLoaded(TalosResult<Vec<TalosSession>>),
    ModelPickerSwitched {
        requested: String,
        result: TalosResult<TalosSession>,
    },
}

#[async_trait]
pub trait GatewayClient: Send + Sync {
    async fn list_models(&self) -> TalosResult<Vec<ModelInfo>>;
    async fn list_sessions(&self) -> TalosResult<Vec<TalosSession>>;
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
    async fn submit_tool_approval(
        &self,
        session_id: &str,
        call_id: &str,
        tool_name: &str,
        approved: bool,
        reason: Option<String>,
    ) -> TalosResult<()>;
}

#[async_trait]
impl GatewayClient for TalosClient {
    async fn list_models(&self) -> TalosResult<Vec<ModelInfo>> {
        TalosClient::list_models(self).await
    }

    async fn list_sessions(&self) -> TalosResult<Vec<TalosSession>> {
        TalosClient::list_sessions(self).await
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

    async fn submit_tool_approval(
        &self,
        session_id: &str,
        call_id: &str,
        tool_name: &str,
        approved: bool,
        reason: Option<String>,
    ) -> TalosResult<()> {
        TalosClient::submit_tool_approval(self, session_id, call_id, tool_name, approved, reason)
            .await
    }
}

/// Main TUI app.
pub struct TuiApp {
    current_state: AppState,
    input_buffer: String,
    cursor_index: usize,
    startup_workspace: PathBuf,
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
    rendered_chat_lines: Vec<RenderedChatLine>,
    chat_content_rect: Option<Rect>,
    chat_viewport_height: usize,
    chat_selection: Option<ChatSelection>,
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
    pending_approval: Option<PendingApprovalState>,
    active_tool_calls: HashMap<String, usize>,
    history_sessions: Vec<TalosSession>,
    history_selected: usize,
    show_model_picker: bool,
    model_picker_selected: usize,
    show_sessions_picker: bool,
    sessions_picker_selected: usize,
    right_panel_visible: bool,
    sent_messages_history: VecDeque<String>,
    message_history_index: Option<usize>,
    show_command_autocomplete: bool,
    command_suggestions: Vec<CommandSuggestion>,
    command_autocomplete_selected: usize,
    command_input_buffer: String,
    close_in_progress: bool,
    close_started_at: Option<Instant>,
    close_timeout_reported: bool,
    closing_session_id: Option<String>,
    should_exit: bool,
    left_panel_area: Option<Rect>,
    input_area: Option<Rect>,
}

impl TuiApp {
    fn is_tool_lifecycle_event(event_type: &TalosEventType) -> bool {
        matches!(
            event_type,
            TalosEventType::MessageToolCallStarted
                | TalosEventType::MessageToolCallCompleted
                | TalosEventType::MessageToolCallPendingApproval
        )
    }

    fn build(gateway_client: Option<Arc<dyn GatewayClient>>) -> Self {
        let mut app = Self {
            current_state: AppState::Chat,
            input_buffer: String::new(),
            cursor_index: 0,
            startup_workspace: env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
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
            rendered_chat_lines: vec![],
            chat_content_rect: None,
            chat_viewport_height: 0,
            chat_selection: None,
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
            pending_approval: None,
            active_tool_calls: HashMap::new(),
            history_sessions: vec![],
            history_selected: 0,
            show_model_picker: false,
            model_picker_selected: 0,
            show_sessions_picker: false,
            sessions_picker_selected: 0,
            right_panel_visible: false,
            sent_messages_history: VecDeque::new(),
            message_history_index: None,
            show_command_autocomplete: false,
            command_suggestions: Vec::new(),
            command_autocomplete_selected: 0,
            command_input_buffer: String::new(),
            close_in_progress: false,
            close_started_at: None,
            close_timeout_reported: false,
            closing_session_id: None,
            should_exit: false,
            left_panel_area: None,
            input_area: None,
        };

        app.push_block(
            ChatRole::System,
            build_welcome_card(APP_VERSION, &app.current_model, &app.startup_workspace),
        );
        app.push_block(
            ChatRole::System,
            "Type a message or use /help /model /history /sessions /status /quit".to_string(),
        );
        app.push_right_event("tui initialized".to_string());
        app
    }

    pub fn new_with_client(client: Arc<TalosClient>) -> Self {
        let gateway_client: Arc<dyn GatewayClient> = client;
        Self::build(Some(gateway_client))
    }

    pub fn new_with_gateway_client(gateway_client: Arc<dyn GatewayClient>) -> Self {
        Self::build(Some(gateway_client))
    }

    pub async fn initialize_remote(&mut self) {
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
        // Approval mode intercepts y/n/Enter/Esc
        if self.pending_approval.is_some() {
            match key_event.code {
                KeyCode::Char('y') | KeyCode::Enter => {
                    self.submit_approval(true).await;
                    return Ok(false);
                }
                KeyCode::Char('n') | KeyCode::Esc => {
                    self.submit_approval(false).await;
                    return Ok(false);
                }
                KeyCode::Char('c') if key_event.modifiers.intersects(KeyModifiers::CONTROL) => {
                    return Ok(true);
                }
                _ => return Ok(false),
            }
        }

        if key_event.modifiers.intersects(KeyModifiers::CONTROL) {
            match key_event.code {
                KeyCode::Char('c') => return Ok(true),
                KeyCode::Char('b') => {
                    self.toggle_right_panel();
                    return Ok(false);
                }
                KeyCode::Char('o') => {
                    self.toggle_last_tool_block();
                    self.rebuild_chat_render_state_if_rect();
                    return Ok(false);
                }
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

        // History state navigation
        if self.current_state == AppState::History {
            match key_event.code {
                KeyCode::Esc | KeyCode::Char('q') => {
                    self.current_state = AppState::Chat;
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.history_selected = self.history_selected.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let max = self.history_sessions.len().saturating_sub(1);
                    if self.history_selected < max {
                        self.history_selected += 1;
                    }
                }
                KeyCode::Enter => {
                    self.switch_to_history_session();
                }
                KeyCode::Char('d') => {
                    self.delete_history_session();
                }
                _ => {}
            }
            return Ok(false);
        }

        // Model picker navigation
        if self.show_model_picker {
            match key_event.code {
                KeyCode::Esc => {
                    self.show_model_picker = false;
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.model_picker_selected = self.model_picker_selected.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let max = self.available_models.len().saturating_sub(1);
                    if self.model_picker_selected < max {
                        self.model_picker_selected += 1;
                    }
                }
                KeyCode::Enter => {
                    self.confirm_model_picker();
                }
                _ => {}
            }
            return Ok(false);
        }

        // Sessions picker navigation
        if self.show_sessions_picker {
            match key_event.code {
                KeyCode::Esc => {
                    self.show_sessions_picker = false;
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.sessions_picker_selected = self.sessions_picker_selected.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let max = self.history_sessions.len().saturating_sub(1);
                    if self.sessions_picker_selected < max {
                        self.sessions_picker_selected += 1;
                    }
                }
                KeyCode::Enter => {
                    self.switch_to_sessions_picker();
                }
                KeyCode::Char(c) if c.is_numeric() => {
                    let index = (c.to_digit(10).unwrap_or(0) as usize).saturating_sub(1);
                    if index < self.history_sessions.len() {
                        self.sessions_picker_selected = index;
                        self.switch_to_sessions_picker();
                    }
                }
                _ => {}
            }
            return Ok(false);
        }

        // Command autocomplete navigation
        if self.show_command_autocomplete {
            match key_event.code {
                KeyCode::Esc => {
                    self.show_command_autocomplete = false;
                    self.command_input_buffer.clear();
                    return Ok(false);
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.command_autocomplete_selected =
                        self.command_autocomplete_selected.saturating_sub(1);
                    return Ok(false);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let max = self.command_suggestions.len().saturating_sub(1);
                    if self.command_autocomplete_selected < max {
                        self.command_autocomplete_selected += 1;
                    }
                    return Ok(false);
                }
                KeyCode::Enter => {
                    if let Some(cmd) = self
                        .command_suggestions
                        .get(self.command_autocomplete_selected)
                    {
                        self.input_buffer = cmd.insert_text.clone();
                        self.cursor_index = self.input_buffer.chars().count();
                        self.show_command_autocomplete = false;
                        self.command_input_buffer.clear();
                    }
                    return Ok(false);
                }
                KeyCode::Backspace => {
                    self.command_input_buffer.pop();
                    if self.command_input_buffer.is_empty() {
                        self.show_command_autocomplete = false;
                    } else {
                        self.update_command_suggestions();
                    }
                    return Ok(false);
                }
                KeyCode::Char(c)
                    if !key_event
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    self.command_input_buffer.push(c);
                    self.update_command_suggestions();
                    return Ok(false);
                }
                _ => {}
            }
            return Ok(false);
        }

        match key_event.code {
            KeyCode::Esc => {
                self.chat_selection = None;
                Ok(false)
            }
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
                if self.input_buffer.is_empty() {
                    self.navigate_message_history(-1);
                } else {
                    self.scroll_chat_up(1);
                }
                Ok(false)
            }
            KeyCode::Down => {
                if self.input_buffer.is_empty() {
                    self.navigate_message_history(1);
                } else {
                    self.scroll_chat_down(1);
                }
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
            KeyCode::Tab => {
                self.toggle_last_tool_block();
                self.rebuild_chat_render_state_if_rect();
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
                        "mode: Gateway".to_string(),
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
            RemoteCommandUpdate::HistoryLoaded(result) => match result {
                Ok(sessions) => {
                    self.history_sessions = sessions;
                    self.history_selected = 0;
                    self.push_right_event(format!(
                        "history loaded: {} sessions",
                        self.history_sessions.len()
                    ));
                }
                Err(err) => {
                    self.push_right_event(format!("history load failed: {err}"));
                    self.current_state = AppState::Chat;
                }
            },
            RemoteCommandUpdate::ModelPickerSwitched { requested, result } => match result {
                Ok(info) => {
                    self.apply_session_snapshot(&info);
                    self.show_model_picker = false;
                    self.push_block(
                        ChatRole::System,
                        format!("Model switched to `{}`.", self.current_model),
                    );
                    self.push_right_event(format!("model picker switched: {}", requested));
                }
                Err(err) => {
                    self.show_model_picker = false;
                    self.push_block(ChatRole::System, format!("Failed to switch model: {}", err));
                    self.push_right_event("model picker switch failed".to_string());
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
        match self.current_state {
            AppState::History => {
                self.render_history_panel(f, f.area());
            }
            AppState::Chat => {
                if self.right_panel_visible {
                    let chunks = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([Constraint::Percentage(72), Constraint::Percentage(28)])
                        .split(f.area());
                    self.render_left_panel(f, chunks[0]);
                    self.render_right_panel(f, chunks[1]);
                } else {
                    self.render_left_panel(f, f.area());
                }
                if self.show_command_autocomplete {
                    self.render_command_autocomplete(f, f.area());
                } else if self.show_model_picker {
                    self.render_model_picker(f, f.area());
                } else if self.show_sessions_picker {
                    self.render_sessions_picker(f, f.area());
                }
            }
        }
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
                    "Commands: /help /model /models /history /sessions /status /quit\nKeys: ↑/↓/PgUp/PgDn scroll, Ctrl+O expand tool, Ctrl+B toggle panel, Ctrl+Y copy, Ctrl+U clear input"
                        .to_string(),
                );
                Ok(false)
            }
            "models" | "modes" => {
                self.refresh_models_command()?;
                Ok(false)
            }
            "history" => {
                self.open_history();
                Ok(false)
            }
            "model" => {
                let requested = args.trim();
                if requested.is_empty() {
                    // No argument: open model picker
                    self.show_model_picker = true;
                    self.model_picker_selected = self
                        .available_models
                        .iter()
                        .position(|m| m == &self.current_model)
                        .unwrap_or(0);
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

                self.switch_remote_model_command(requested.to_string())?;

                Ok(false)
            }
            "sessions" | "s" => {
                // Load sessions for quick switch
                if let Some(client) = self.gateway_client.clone() {
                    let _ = self.spawn_remote_command_task("loading sessions", async move {
                        RemoteCommandUpdate::HistoryLoaded(client.list_sessions().await)
                    });
                    self.show_sessions_picker = true;
                    Ok(false)
                } else {
                    self.push_block(
                        ChatRole::System,
                        "Sessions picker requires gateway mode.".to_string(),
                    );
                    Ok(false)
                }
            }
            "status" => {
                self.status_command()?;
                Ok(false)
            }
            "quit" => {
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
                Ok(false)
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

    fn navigate_message_history(&mut self, direction: i32) {
        if self.sent_messages_history.is_empty() {
            return;
        }

        match direction {
            -1 => {
                // Up: go to previous message in history
                let new_index = if let Some(idx) = self.message_history_index {
                    if idx == 0 {
                        self.sent_messages_history.len() - 1
                    } else {
                        idx - 1
                    }
                } else {
                    self.sent_messages_history.len() - 1
                };
                self.message_history_index = Some(new_index);
                if let Some(msg) = self.sent_messages_history.get(new_index) {
                    self.input_buffer = msg.clone();
                    self.cursor_index = self.input_buffer.chars().count();
                }
            }
            1 => {
                // Down: go to next message in history or back to empty input
                if let Some(idx) = self.message_history_index {
                    let new_index = (idx + 1) % self.sent_messages_history.len();
                    if new_index == 0 && idx > 0 {
                        // Return to empty input
                        self.input_buffer.clear();
                        self.cursor_index = 0;
                        self.message_history_index = None;
                    } else {
                        self.message_history_index = Some(new_index);
                        if let Some(msg) = self.sent_messages_history.get(new_index) {
                            self.input_buffer = msg.clone();
                            self.cursor_index = self.input_buffer.chars().count();
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn add_message_to_history(&mut self, message: String) {
        const MAX_HISTORY: usize = 100;
        self.sent_messages_history.push_back(message);
        if self.sent_messages_history.len() > MAX_HISTORY {
            self.sent_messages_history.pop_front();
        }
        self.message_history_index = None;
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
        self.add_message_to_history(user_message.clone());
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

        if self.session_id.is_none() {
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

        if self.in_flight {
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
        let Some(client) = self.gateway_client.clone() else {
            self.push_block(ChatRole::System, "Gateway client unavailable.".to_string());
            return Ok(());
        };

        let Some(session_id) = self.session_id.clone() else {
            return Err(TalosError::ClientError("session is not ready".to_string()));
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

        let thinking_idx = self.push_block(ChatRole::Thinking, String::new());
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

        Ok(())
    }

    fn handle_stream_event(&mut self, event: TalosEvent) {
        self.trace_event_count = self.trace_event_count.saturating_add(1);
        let is_tool_lifecycle_event = Self::is_tool_lifecycle_event(&event.event_type);

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
            Some(active_id) if event.message_id != active_id && !is_tool_lifecycle_event => {
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
            if event.sequence < last_seq && !is_tool_lifecycle_event {
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

        match (&event.event_type, &event.payload) {
            (TalosEventType::MessageStarted, TalosEventPayload::MessageStarted(_)) => {}
            (
                TalosEventType::MessageReasoningDelta,
                TalosEventPayload::MessageReasoningDelta(payload),
            ) => {
                if !payload.delta.is_empty() {
                    self.append_thinking(&payload.delta);
                }
            }
            (
                TalosEventType::MessageExecutionSummary,
                TalosEventPayload::MessageExecutionSummary(payload),
            ) => {
                if !payload.text.trim().is_empty() {
                    self.push_block(
                        ChatRole::System,
                        format!(
                            "Execution summary [{} #{}]: {}",
                            payload.step_id, payload.iteration, payload.text
                        ),
                    );
                }
            }
            (
                TalosEventType::MessageOutputDelta,
                TalosEventPayload::MessageOutputDelta(payload),
            ) => {
                self.append_assistant(&payload.delta);
                self.received_output_delta = true;
            }
            (
                TalosEventType::MessageToolCallStarted,
                TalosEventPayload::MessageToolCallStarted(payload),
            ) => {
                self.push_right_event(format!(
                    "tool started [{}]: {}",
                    payload.step_id, payload.name
                ));
                self.tool_events
                    .push(format!("started [{}]: {}", payload.step_id, payload.name));
                self.push_tool_call_block(
                    payload.call_id.clone(),
                    payload.name.clone(),
                    payload.arguments.clone(),
                    payload.timing_ms,
                );
            }
            (
                TalosEventType::MessageToolCallCompleted,
                TalosEventPayload::MessageToolCallCompleted(payload),
            ) => {
                self.push_right_event(format!(
                    "tool completed: {} ({})",
                    payload.call_id, payload.tool
                ));
                self.tool_events
                    .push(format!("completed: {} ({})", payload.call_id, payload.tool));
                let output = payload
                    .content
                    .iter()
                    .filter_map(|item| match item {
                        TalosContentItem::Text { text } => Some(text.clone()),
                        TalosContentItem::Json { value } => Some(value.to_string()),
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                self.complete_tool_call_block(
                    &payload.call_id,
                    output,
                    !payload.is_error,
                    payload.timing_ms,
                );
            }
            (
                TalosEventType::MessageToolCallPendingApproval,
                TalosEventPayload::MessageToolCallPendingApproval(payload),
            ) => {
                self.push_right_event(format!(
                    "approval needed: {} ({})",
                    payload.tool_name, payload.call_id
                ));
                self.pending_approval = Some(PendingApprovalState {
                    call_id: payload.call_id.clone(),
                    tool_name: payload.tool_name.clone(),
                    prompt: payload.prompt.clone(),
                    arguments: payload.arguments.clone(),
                });
            }
            (TalosEventType::MessageCompleted, TalosEventPayload::MessageCompleted(payload)) => {
                self.session_usage.input_tokens = self
                    .session_usage
                    .input_tokens
                    .saturating_add(payload.usage.input_tokens);
                self.session_usage.output_tokens = self
                    .session_usage
                    .output_tokens
                    .saturating_add(payload.usage.output_tokens);
                self.session_usage.total_tokens = self
                    .session_usage
                    .total_tokens
                    .saturating_add(payload.usage.total_tokens);
                self.finish_active_stream("stream completed", false, false);
            }
            (TalosEventType::MessageError, TalosEventPayload::MessageError(payload)) => {
                self.push_block(ChatRole::System, format!("Stream error: {}", payload.error));
                self.finish_active_stream("stream error", true, false);
            }
            _ => {}
        }
    }

    async fn refresh_models_from_gateway(&mut self) -> TalosResult<()> {
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

    fn open_history(&mut self) {
        let Some(client) = self.gateway_client.clone() else {
            self.push_block(ChatRole::System, "No gateway client connected.".to_string());
            return;
        };
        self.current_state = AppState::History;
        let _ = self.spawn_remote_command_task("loading history", async move {
            RemoteCommandUpdate::HistoryLoaded(client.list_sessions().await)
        });
    }

    #[allow(dead_code)]
    fn toggle_model_picker(&mut self) {
        if self.show_model_picker {
            self.show_model_picker = false;
            return;
        }
        if self.available_models.is_empty() {
            self.push_block(
                ChatRole::System,
                "No models loaded. Run /models first.".to_string(),
            );
            return;
        }
        // Pre-select current model
        self.model_picker_selected = self
            .available_models
            .iter()
            .position(|m| m == &self.current_model)
            .unwrap_or(0);
        self.show_model_picker = true;
    }

    fn switch_to_history_session(&mut self) {
        let Some(session) = self.history_sessions.get(self.history_selected).cloned() else {
            return;
        };
        let session_id = session.context.session_id.clone();
        let model = session.config.llm.model.clone();
        self.session_id = Some(session_id.clone());
        self.current_model = model.clone();
        self.session_usage = session.state.usage.clone();
        self.current_state = AppState::Chat;
        self.push_block(
            ChatRole::System,
            format!("Switched to session {} (model: {})", session_id, model),
        );
        self.push_right_event(format!("switched to history session: {}", session_id));
    }

    fn switch_to_sessions_picker(&mut self) {
        let Some(session) = self
            .history_sessions
            .get(self.sessions_picker_selected)
            .cloned()
        else {
            return;
        };
        let session_id = session.context.session_id.clone();
        let model = session.config.llm.model.clone();
        self.session_id = Some(session_id.clone());
        self.current_model = model.clone();
        self.session_usage = session.state.usage.clone();
        self.show_sessions_picker = false;
        self.push_block(
            ChatRole::System,
            format!("Switched to session {} (model: {})", session_id, model),
        );
        self.push_right_event(format!("switched via /sessions: {}", session_id));
    }

    fn delete_history_session(&mut self) {
        let Some(session) = self.history_sessions.get(self.history_selected).cloned() else {
            return;
        };
        let session_id = session.context.session_id.clone();
        let Some(client) = self.gateway_client.clone() else {
            return;
        };
        self.history_sessions.remove(self.history_selected);
        if self.history_selected > 0 && self.history_selected >= self.history_sessions.len() {
            self.history_selected = self.history_sessions.len().saturating_sub(1);
        }
        let sid_log = session_id.clone();
        tokio::spawn(async move {
            let _ = client.close_session(&session_id).await;
        });
        self.push_right_event(format!("deleted session: {}", sid_log));
    }

    fn confirm_model_picker(&mut self) {
        let Some(model) = self
            .available_models
            .get(self.model_picker_selected)
            .cloned()
        else {
            return;
        };
        let Some(client) = self.gateway_client.clone() else {
            self.show_model_picker = false;
            return;
        };
        let session_id = self.session_id.clone();
        let requested = model.clone();
        let _ = self.spawn_remote_command_task(
            format!("model picker switching to: {}", model),
            async move {
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
                RemoteCommandUpdate::ModelPickerSwitched { requested, result }
            },
        );
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
            "Closing session. Memory flush continues in background.",
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
        if self.session_id.is_some() || self.pending_session_connect.is_some() {
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
        // Trigger command autocomplete when typing /
        if c == '/' && self.cursor_index == 0 && self.input_buffer.is_empty() {
            self.show_command_autocomplete = true;
            self.command_suggestions = CommandSuggestion::all();
            self.command_autocomplete_selected = 0;
            self.command_input_buffer.clear();
            return;
        }

        let idx = byte_index_at_char(&self.input_buffer, self.cursor_index);
        self.input_buffer.insert(idx, c);
        self.cursor_index += 1;
    }

    fn update_command_suggestions(&mut self) {
        self.command_suggestions = CommandSuggestion::filter(&self.command_input_buffer);
        self.command_autocomplete_selected = 0;
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

    fn copy_selection_to_clipboard(&mut self) {
        let Some(text) = self.selected_chat_text() else {
            self.show_status_hint("Nothing to copy.", Duration::from_secs(2));
            self.chat_selection = None;
            return;
        };

        match copy_to_system_clipboard(&text) {
            Ok(()) => {
                self.show_status_hint("Copied selection to clipboard.", Duration::from_secs(2))
            }
            Err(err) => {
                self.show_status_hint(
                    format!("Clipboard copy failed: {}", err),
                    Duration::from_secs(3),
                );
            }
        }
        self.chat_selection = None;
    }

    fn selected_chat_text(&self) -> Option<String> {
        let (start, end) = self.normalized_selection_points()?;
        let mut copied_lines = Vec::new();

        for line_index in start.line_index..=end.line_index {
            let Some(line) = self.rendered_chat_lines.get(line_index) else {
                continue;
            };
            let Some(projection) = &line.copy_projection else {
                continue;
            };
            let line_width = line_display_width(&line.display);
            let Some((selection_start, selection_end)) =
                self.selection_display_range_for_line(line_index, line_width)
            else {
                continue;
            };
            let projection_end =
                projection.display_start_col + text_display_width(&projection.text);
            let copy_start = selection_start.max(projection.display_start_col);
            let copy_end = selection_end.min(projection_end);
            if copy_start >= copy_end {
                copied_lines.push(String::new());
                continue;
            }
            copied_lines.push(slice_text_by_display_columns(
                &projection.text,
                copy_start.saturating_sub(projection.display_start_col),
                copy_end.saturating_sub(projection.display_start_col),
            ));
        }

        if copied_lines.iter().all(|line| line.is_empty()) {
            None
        } else {
            Some(copied_lines.join("\n"))
        }
    }

    fn normalized_selection_points(&self) -> Option<(SelectionPoint, SelectionPoint)> {
        let selection = self.chat_selection?;
        if (selection.anchor.line_index, selection.anchor.column)
            <= (selection.focus.line_index, selection.focus.column)
        {
            Some((selection.anchor, selection.focus))
        } else {
            Some((selection.focus, selection.anchor))
        }
    }

    fn selection_display_range_for_line(
        &self,
        line_index: usize,
        line_width: usize,
    ) -> Option<(usize, usize)> {
        let (start, end) = self.normalized_selection_points()?;
        if line_index < start.line_index || line_index > end.line_index {
            return None;
        }

        let width = line_width.max(1);
        let range = if start.line_index == end.line_index {
            (
                start.column.min(end.column),
                start.column.max(end.column).saturating_add(1),
            )
        } else if line_index == start.line_index {
            (start.column, width)
        } else if line_index == end.line_index {
            (0, end.column.saturating_add(1))
        } else {
            (0, width)
        };

        Some((range.0.min(width), range.1.min(width)))
    }

    fn handle_mouse(&mut self, mouse_event: MouseEvent) {
        match mouse_event.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(point) = self.selection_point_from_mouse(mouse_event) {
                    self.chat_selection = Some(ChatSelection {
                        anchor: point,
                        focus: point,
                    });
                } else {
                    self.chat_selection = None;
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if let (Some(point), Some(selection)) = (
                    self.selection_point_from_mouse(mouse_event),
                    self.chat_selection.as_mut(),
                ) {
                    selection.focus = point;
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                if let (Some(point), Some(selection)) = (
                    self.selection_point_from_mouse(mouse_event),
                    self.chat_selection.as_mut(),
                ) {
                    selection.focus = point;
                }
                if self.chat_selection.is_some() {
                    self.copy_selection_to_clipboard();
                }
            }
            MouseEventKind::ScrollUp => self.scroll_chat_up(3),
            MouseEventKind::ScrollDown => self.scroll_chat_down(3),
            _ => {}
        }
    }

    fn selection_point_from_mouse(&self, mouse_event: MouseEvent) -> Option<SelectionPoint> {
        let rect = self.chat_content_rect?;
        if rect.width == 0 || rect.height == 0 {
            return None;
        }
        if mouse_event.column < rect.x
            || mouse_event.column >= rect.x.saturating_add(rect.width)
            || mouse_event.row < rect.y
            || mouse_event.row >= rect.y.saturating_add(rect.height)
        {
            return None;
        }

        let row_offset = mouse_event.row.saturating_sub(rect.y) as usize;
        let line_index = self.chat_scroll as usize + row_offset;
        if line_index >= self.rendered_chat_lines.len() {
            return None;
        }

        Some(SelectionPoint {
            line_index,
            column: mouse_event.column.saturating_sub(rect.x) as usize,
        })
    }

    fn push_block(&mut self, role: ChatRole, content: String) -> usize {
        self.chat_blocks.push(ChatBlock::new(role, content));
        let idx = self.chat_blocks.len() - 1;
        self.sync_history_entry(idx);
        self.request_follow_bottom();
        idx
    }

    fn remove_chat_block(&mut self, idx: usize) {
        if idx >= self.chat_blocks.len() {
            return;
        }

        self.chat_blocks.remove(idx);

        self.active_tool_calls = self
            .active_tool_calls
            .drain()
            .filter_map(|(call_id, block_idx)| {
                if block_idx == idx {
                    None
                } else if block_idx > idx {
                    Some((call_id, block_idx - 1))
                } else {
                    Some((call_id, block_idx))
                }
            })
            .collect();
    }

    fn push_tool_call_block(
        &mut self,
        call_id: String,
        tool_name: String,
        tool_input: serde_json::Value,
        timing_ms: Option<u64>,
    ) -> usize {
        let block = ChatBlock::new_tool_call(call_id.clone(), tool_name, tool_input, timing_ms);
        self.chat_blocks.push(block);
        let idx = self.chat_blocks.len() - 1;
        self.active_tool_calls.insert(call_id, idx);
        self.request_follow_bottom();
        idx
    }

    fn resolve_tool_call_block_idx(&mut self, call_id: &str) -> Option<usize> {
        if let Some(&idx) = self.active_tool_calls.get(call_id) {
            if self
                .chat_blocks
                .get(idx)
                .and_then(|block| block.tool_call_id.as_deref())
                == Some(call_id)
            {
                return Some(idx);
            }
        }

        let idx = self
            .chat_blocks
            .iter()
            .position(|block| block.tool_call_id.as_deref() == Some(call_id))?;
        self.active_tool_calls.insert(call_id.to_string(), idx);
        Some(idx)
    }

    fn complete_tool_call_block(
        &mut self,
        call_id: &str,
        output: String,
        exit_ok: bool,
        timing_ms: Option<u64>,
    ) {
        if let Some(idx) = self.resolve_tool_call_block_idx(call_id) {
            if let Some(block) = self.chat_blocks.get_mut(idx) {
                block.content = output;
                block.tool_status = Some(ToolCallStatus::Completed { exit_ok });
                if timing_ms.is_some() {
                    block.timing_ms = timing_ms;
                }
            }
            self.active_tool_calls.remove(call_id);
            self.request_follow_bottom();
        }
    }

    fn toggle_last_tool_block(&mut self) {
        let last_tool_idx = self
            .chat_blocks
            .iter()
            .rposition(|b| b.role == ChatRole::Tool);
        if let Some(idx) = last_tool_idx {
            if let Some(block) = self.chat_blocks.get_mut(idx) {
                block.expanded = !block.expanded;
                self.request_follow_bottom();
            }
        }
    }

    fn toggle_right_panel(&mut self) {
        self.right_panel_visible = !self.right_panel_visible;
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
            self.remove_chat_block(idx);
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

    fn append_thinking(&mut self, text: &str) {
        let idx = if let Some(idx) = self.active_thinking_idx {
            idx
        } else {
            let idx = self.push_block(ChatRole::Thinking, String::new());
            self.active_thinking_idx = Some(idx);
            idx
        };

        if let Some(block) = self.chat_blocks.get_mut(idx) {
            block.content.push_str(text);
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
        self.active_tool_calls.clear();
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

    fn chat_content_columns(&self, chat_inner: Rect) -> (Rect, Rect) {
        let chat_columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(CHAT_CONTENT_PADDING_LEFT),
                Constraint::Min(1),
                Constraint::Length(1),
            ])
            .split(chat_inner);
        (chat_columns[1], chat_columns[2])
    }

    fn render_left_panel(&mut self, f: &mut Frame, area: ratatui::layout::Rect) {
        // Save left panel area for floater positioning
        self.left_panel_area = Some(area);

        let left_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Rgb(106, 111, 123)))
            .title(format!("Talos TUI ({})", APP_VERSION))
            .title_alignment(Alignment::Left)
            .style(Style::default().fg(Color::White));
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
                Constraint::Length(8),
            ])
            .split(left_inner);

        let chat_panel = Block::default().style(Style::default().bg(Color::Rgb(33, 38, 49)));
        let chat_inner = chat_panel.inner(left_chunks[0]);
        f.render_widget(chat_panel, left_chunks[0]);

        let (chat_content_rect, chat_scrollbar_rect) = self.chat_content_columns(chat_inner);

        self.rebuild_chat_render_state(chat_content_rect);
        let visible_lines = self.visible_chat_lines();
        let viewport_height = self.chat_viewport_height;
        let total_lines = self.rendered_chat_lines.len();
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

        let conversation = Paragraph::new(visible_lines)
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
        self.input_area = Some(input_inner);

        if let Some(approval) = &self.pending_approval {
            self.render_approval_card(f, input_inner, approval);
        } else {
            let input_bg_style = Style::default().bg(Color::Rgb(72, 76, 88));
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
        }

        let wait_style = if self.in_flight {
            Style::default().bg(Color::Rgb(36, 39, 46)).fg(Color::Cyan)
        } else {
            Style::default().bg(Color::Rgb(36, 39, 46)).fg(Color::White)
        };
        let status_block = Block::default().style(Style::default().bg(Color::Rgb(36, 39, 46)));
        f.render_widget(status_block, left_chunks[3]);
        let wait_row = Paragraph::new(self.input_status_line(left_chunks[3].width as usize))
            .style(wait_style)
            .wrap(Wrap { trim: false });
        f.render_widget(wait_row, left_chunks[3]);

        let bottom_gap = Block::default().style(Style::default().bg(Color::Rgb(33, 38, 49)));
        f.render_widget(bottom_gap, left_chunks[4]);
    }

    async fn submit_approval(&mut self, approved: bool) {
        let Some(approval) = self.pending_approval.take() else {
            return;
        };
        let Some(client) = self.gateway_client.clone() else {
            self.push_block(ChatRole::System, "Gateway client unavailable.".to_string());
            return;
        };
        let Some(session_id) = self.session_id.clone() else {
            self.push_block(ChatRole::System, "No active session.".to_string());
            return;
        };
        let label = if approved { "approved" } else { "rejected" };
        self.push_right_event(format!("tool approval {}: {}", label, approval.tool_name));

        if let Err(err) = client
            .submit_tool_approval(
                &session_id,
                &approval.call_id,
                &approval.tool_name,
                approved,
                None,
            )
            .await
        {
            self.push_block(
                ChatRole::System,
                format!("Failed to submit approval: {}", err),
            );
            return;
        }

        // Re-invoke session to continue execution after approval
        let continuation = if approved {
            String::new()
        } else {
            format!("[Tool `{}` was rejected by user.]", approval.tool_name)
        };
        if let Err(err) = self.start_stream_for_message(continuation).await {
            self.push_block(
                ChatRole::System,
                format!("Failed to continue after approval: {}", err),
            );
        }
    }

    fn render_approval_card(
        &self,
        f: &mut Frame,
        area: ratatui::layout::Rect,
        approval: &PendingApprovalState,
    ) {
        let card_style = Style::default().bg(Color::Rgb(50, 35, 20)).fg(Color::White);
        f.render_widget(Block::default().style(card_style), area);
        let title = format!(" ⚠  {} — 需要确认 ", approval.tool_name);
        let card = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .title(Span::styled(
                title,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ))
            .style(card_style);
        let inner = card.inner(area);
        f.render_widget(card, area);
        let prompt_text = if approval.prompt.is_empty() {
            format!("执行工具: {}", approval.tool_name)
        } else {
            approval.prompt.clone()
        };
        let lines = vec![
            Line::from(Span::styled(prompt_text, Style::default().fg(Color::White))),
            Line::from(""),
            Line::from(vec![
                Span::styled(
                    " [y / Enter: 批准] ",
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(
                    " [n / Esc: 拒绝] ",
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Red)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
        ];
        let para = Paragraph::new(lines).style(card_style);
        f.render_widget(para, inner);
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
        lines.push(Line::from("mode: Gateway"));
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
        lines.push(Line::from("/help /model /history /sessions /status /quit"));
        lines.push(Line::from("↑/↓/PgUp/PgDn scroll  Ctrl+O expand tool"));
        lines.push(Line::from("Ctrl+B panel  Ctrl+Y copy  Ctrl+U clear"));

        let right = Paragraph::new(lines).wrap(Wrap { trim: false });
        f.render_widget(right, right_inner);
    }

    fn render_history_panel(&self, f: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Rgb(80, 130, 200)))
            .title(" History  [Enter] switch  [d] delete  [Esc] back ");
        let inner = block.inner(area);
        f.render_widget(block, area);

        if self.history_sessions.is_empty() {
            let msg = Paragraph::new("No sessions found.  (loading...)")
                .style(Style::default().fg(Color::DarkGray));
            f.render_widget(msg, inner);
            return;
        }

        let items: Vec<ListItem> = self
            .history_sessions
            .iter()
            .map(|s| {
                let sid = &s.context.session_id;
                let model = &s.config.llm.model;
                let msgs = s.state.message_count;
                ListItem::new(format!(" {sid}  [{model}]  {msgs} msgs"))
            })
            .collect();

        let mut list_state = ListState::default();
        list_state.select(Some(self.history_selected));

        let list = List::new(items)
            .highlight_style(
                Style::default()
                    .bg(Color::Rgb(50, 80, 130))
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▶ ");

        f.render_stateful_widget(list, inner, &mut list_state);
    }

    fn render_model_picker(&self, f: &mut Frame, area: Rect) {
        // Use left panel area for popup positioning
        let popup_area = self.left_panel_area.unwrap_or(area);

        // Center a popup of width=50, height=min(models+4, 20)
        let popup_height = (self.available_models.len() as u16 + 4).min(20);
        let popup_width = 52u16;
        let x = popup_area.x + popup_area.width.saturating_sub(popup_width) / 2;
        let y = popup_area.y + popup_area.height.saturating_sub(popup_height) / 2;
        let popup_rect = Rect::new(
            x.min(popup_area.x + popup_area.width.saturating_sub(popup_width)),
            y.min(popup_area.y + popup_area.height.saturating_sub(popup_height)),
            popup_width.min(popup_area.width),
            popup_height.min(popup_area.height),
        );

        f.render_widget(Clear, popup_rect);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Rgb(100, 180, 240)))
            .title(" Switch Model  [Enter] confirm  [Esc] cancel ");
        let inner = block.inner(popup_rect);
        f.render_widget(block, popup_rect);

        let items: Vec<ListItem> = self
            .available_models
            .iter()
            .map(|m| {
                if m == &self.current_model {
                    ListItem::new(format!(" {m}  ✓"))
                } else {
                    ListItem::new(format!(" {m}"))
                }
            })
            .collect();

        let mut list_state = ListState::default();
        list_state.select(Some(self.model_picker_selected));

        let list = List::new(items)
            .highlight_style(
                Style::default()
                    .bg(Color::Rgb(40, 80, 140))
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▶ ");

        f.render_stateful_widget(list, inner, &mut list_state);
    }

    fn render_sessions_picker(&self, f: &mut Frame, area: Rect) {
        // Use left panel area for popup positioning
        let popup_area = self.left_panel_area.unwrap_or(area);

        let popup_height = (self.history_sessions.len() as u16 + 4).min(20);
        let popup_width = 70u16;
        let x = popup_area.x + popup_area.width.saturating_sub(popup_width) / 2;
        let y = popup_area.y + popup_area.height.saturating_sub(popup_height) / 2;
        let popup_rect = Rect::new(
            x.min(popup_area.x + popup_area.width.saturating_sub(popup_width)),
            y.min(popup_area.y + popup_area.height.saturating_sub(popup_height)),
            popup_width.min(popup_area.width),
            popup_height.min(popup_area.height),
        );

        f.render_widget(Clear, popup_rect);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Rgb(100, 180, 240)))
            .title(" Recent Sessions  [↑↓] navigate  [1-9] quick  [Enter] switch  [Esc] cancel ");
        let inner = block.inner(popup_rect);
        f.render_widget(block, popup_rect);

        let items: Vec<ListItem> = self
            .history_sessions
            .iter()
            .enumerate()
            .map(|(idx, s)| {
                let session_id = &s.context.session_id;
                let model = &s.config.llm.model;
                let msgs = s.state.message_count;
                let idx_display = (idx + 1) % 10;
                ListItem::new(format!(
                    " [{}] {} [{}] {} msgs",
                    idx_display,
                    &session_id[0..8.min(session_id.len())],
                    model,
                    msgs
                ))
            })
            .collect();

        let mut list_state = ListState::default();
        list_state.select(Some(self.sessions_picker_selected));

        let list = List::new(items)
            .highlight_style(
                Style::default()
                    .bg(Color::Rgb(40, 80, 140))
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▶ ");

        f.render_stateful_widget(list, inner, &mut list_state);
    }

    fn command_autocomplete_panel_height(&self) -> u16 {
        if !self.show_command_autocomplete {
            return 0;
        }
        self.command_suggestions.len().clamp(1, 7) as u16
    }

    fn render_command_autocomplete(&self, f: &mut Frame, area: Rect) {
        let Some(input_area) = self.input_area else {
            return;
        };
        let popup_area = self.left_panel_area.unwrap_or(area);
        let popup_height = self.command_autocomplete_panel_height();
        if popup_height == 0 || input_area.width == 0 {
            return;
        }

        let max_width = popup_area.width.saturating_sub(2).max(1);
        let popup_width = input_area.width.min(max_width);
        let popup_x = input_area.x;
        let popup_y = input_area.y.saturating_add(input_area.height);
        let available_height = popup_area
            .y
            .saturating_add(popup_area.height)
            .saturating_sub(popup_y);
        let popup_rect = Rect::new(
            popup_x,
            popup_y,
            popup_width,
            popup_height.min(available_height),
        );
        if popup_rect.width == 0 || popup_rect.height == 0 {
            return;
        }

        let bg_style = Style::default().bg(Color::Rgb(40, 44, 58));
        f.render_widget(Clear, popup_rect);
        f.render_widget(Block::default().style(bg_style), popup_rect);

        let items: Vec<ListItem> = if self.command_suggestions.is_empty() {
            vec![ListItem::new(Line::from(vec![Span::styled(
                "No matching commands",
                Style::default().fg(Color::Rgb(150, 154, 165)),
            )]))]
        } else {
            self.command_suggestions
                .iter()
                .enumerate()
                .map(|(index, cmd)| {
                    let is_selected = index == self.command_autocomplete_selected;
                    let command_style = if is_selected {
                        Style::default()
                            .fg(Color::Rgb(134, 233, 255))
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Rgb(242, 243, 245))
                    };
                    let description_style = if is_selected {
                        Style::default()
                            .fg(Color::Rgb(134, 233, 255))
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Rgb(186, 186, 186))
                    };
                    ListItem::new(vec![Line::from(vec![
                        Span::styled(&cmd.display_text, command_style),
                        Span::raw("  "),
                        Span::styled(&cmd.description, description_style),
                    ])])
                })
                .collect()
        };

        let mut list_state = ListState::default();
        if !self.command_suggestions.is_empty() {
            list_state.select(Some(self.command_autocomplete_selected));
        }

        let list = List::new(items)
            .style(bg_style)
            .highlight_style(Style::default().bg(Color::Rgb(40, 44, 58)))
            .highlight_symbol("› ")
            .highlight_spacing(ratatui::widgets::HighlightSpacing::Always);

        f.render_stateful_widget(list, popup_rect, &mut list_state);
    }

    fn is_agent_working(&self) -> bool {
        // Show spinner only if there's an empty Thinking block (streaming not yet started outputting content)
        self.chat_blocks
            .last()
            .map(|b| b.role == ChatRole::Thinking && b.content.is_empty())
            .unwrap_or(false)
    }

    fn thinking_spinner_line(&self, spinner_index: usize) -> RenderedChatLine {
        let color = Self::spinning_color(spinner_index);
        let text = "✢ Thinking…".to_string();

        let display = Line::from(vec![Span::styled(text.clone(), Style::default().fg(color))]);

        RenderedChatLine {
            display,
            copy_projection: Some(CopyProjection {
                text,
                display_start_col: 0,
            }),
        }
    }

    fn rebuild_chat_render_state_if_rect(&mut self) {
        if let Some(rect) = self.chat_content_rect {
            self.rebuild_chat_render_state(rect);
        }
    }

    fn rebuild_chat_render_state(&mut self, chat_content_rect: Rect) {
        self.chat_content_rect = Some(chat_content_rect);
        self.chat_viewport_height = chat_content_rect.height as usize;
        self.rendered_chat_lines.clear();

        let content_width = chat_content_rect.width.saturating_sub(2).max(1) as usize;
        for block in &self.chat_blocks {
            self.rendered_chat_lines
                .extend(self.chat_card_lines(block, content_width));
            self.rendered_chat_lines.push(RenderedChatLine {
                display: Line::from(""),
                copy_projection: Some(CopyProjection {
                    text: String::new(),
                    display_start_col: 0,
                }),
            });
        }

        // Append thinking spinner if agent is working
        if self.is_agent_working() {
            self.rendered_chat_lines
                .push(self.thinking_spinner_line(self.spinner_index));
        }

        if self.rendered_chat_lines.is_empty() {
            self.rendered_chat_lines.push(RenderedChatLine {
                display: Line::from("No messages yet."),
                copy_projection: Some(CopyProjection {
                    text: "No messages yet.".to_string(),
                    display_start_col: 0,
                }),
            });
        }
    }

    fn visible_chat_lines(&self) -> Vec<Line<'static>> {
        self.rendered_chat_lines
            .iter()
            .enumerate()
            .map(|(line_index, line)| self.decorate_selection(line_index, line))
            .collect()
    }

    fn decorate_selection(&self, line_index: usize, line: &RenderedChatLine) -> Line<'static> {
        let mut display = line.display.clone();
        let Some(projection) = &line.copy_projection else {
            return display;
        };
        let line_width = line_display_width(&display);
        let Some((selection_start, selection_end)) =
            self.selection_display_range_for_line(line_index, line_width)
        else {
            return display;
        };
        let projection_end = projection.display_start_col + text_display_width(&projection.text);
        let highlight_start = selection_start.max(projection.display_start_col);
        let highlight_end = selection_end.min(projection_end);
        if highlight_start >= highlight_end {
            return display;
        }
        highlight_line_columns(&mut display, highlight_start, highlight_end);
        display
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

        // Add message history indicator if browsing history
        let history_indicator = if let Some(idx) = self.message_history_index {
            let total = self.sent_messages_history.len();
            format!("  [History {}/{}]", idx + 1, total)
        } else {
            String::new()
        };

        let base = if self.close_in_progress {
            let session_id = self
                .closing_session_id
                .clone()
                .unwrap_or_else(|| "unknown".to_string());
            format!("closing {} · flushing memory · {}", session_id, workspace)
        } else if self.in_flight {
            let motion = self.stripe_line(6);
            format!(
                "{} {} · {} · {}{}",
                motion, self.current_model, token_left, workspace, history_indicator
            )
        } else {
            format!(
                "{} · {} · {}{}",
                self.current_model, token_left, workspace, history_indicator
            )
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

    fn chat_card_lines(&self, block: &ChatBlock, content_width: usize) -> Vec<RenderedChatLine> {
        match block.role {
            ChatRole::User => self.user_block_lines(block, content_width),
            ChatRole::Tool => self.tool_block_lines(block, content_width),
            ChatRole::Assistant => self.assistant_block_lines(block, content_width),
            ChatRole::Thinking => self.thinking_block_lines(block, content_width),
            ChatRole::System => self.system_block_lines(block, content_width),
        }
    }

    fn user_block_lines(&self, block: &ChatBlock, content_width: usize) -> Vec<RenderedChatLine> {
        let mut lines = vec![];
        let label_style = Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD);
        let body_style = Style::default().fg(Color::White);

        // Add role label
        lines.push(RenderedChatLine {
            display: Line::from(Span::styled("👤 User", label_style)),
            copy_projection: Some(CopyProjection {
                text: "User".to_string(),
                display_start_col: 0,
            }),
        });

        let content_lines = block
            .content
            .lines()
            .map(|line| Line::from(line.to_string()))
            .collect::<Vec<_>>();

        // Content lines with wrapping
        for line in content_lines.iter() {
            for wrapped in wrap_line_visual(line.clone(), content_width) {
                lines.push(self.decorate_chat_line(wrapped, body_style, 0));
            }
        }

        lines
    }

    fn tool_block_lines(&self, block: &ChatBlock, content_width: usize) -> Vec<RenderedChatLine> {
        let tool_name = block.tool_name.as_deref().unwrap_or("unknown");
        let mut lines = vec![];

        // Tool call header: ● ToolName(arg_summary)
        let arg_summary = if let Some(input) = &block.tool_input {
            Self::tool_arg_summary(tool_name, input)
        } else {
            String::new()
        };

        let (icon_str, icon_color) = match block.tool_status {
            Some(ToolCallStatus::Running) => ("⏳", Color::Yellow),
            Some(ToolCallStatus::Completed { exit_ok: true }) => ("●", Color::Green),
            Some(ToolCallStatus::Completed { exit_ok: false }) => ("●", Color::Red),
            None => ("●", Color::Gray),
        };

        let header_text = if arg_summary.is_empty() {
            format!("{} {}", icon_str, tool_name)
        } else {
            format!("{} {}({})", icon_str, tool_name, arg_summary)
        };

        let header_style = Style::default().fg(icon_color).add_modifier(Modifier::BOLD);
        lines.push(RenderedChatLine {
            display: Line::from(vec![Span::styled(header_text.clone(), header_style)]),
            copy_projection: Some(CopyProjection {
                text: header_text,
                display_start_col: 0,
            }),
        });

        let render_data = Self::build_tool_render_data(block, tool_name);
        let (summary_text, summary_color) = match block.tool_status {
            Some(ToolCallStatus::Running) => (render_data.summary, Color::Yellow),
            Some(ToolCallStatus::Completed { exit_ok: true }) => {
                (render_data.summary, Color::Green)
            }
            Some(ToolCallStatus::Completed { exit_ok: false }) => (render_data.summary, Color::Red),
            None => (render_data.summary, Color::Gray),
        };

        let summary_style = Style::default().fg(summary_color);
        lines.push(RenderedChatLine {
            display: Line::from(Span::styled(summary_text.clone(), summary_style)),
            copy_projection: Some(CopyProjection {
                text: summary_text,
                display_start_col: 0,
            }),
        });

        let detail_style = Style::default().fg(Color::Gray);
        let output_style = Style::default().fg(Color::White);
        let detail_lines = render_data.detail_lines;
        let preview_lines = render_data.preview_lines;
        let output_lines = render_data.output_lines;
        let has_output = !output_lines.is_empty();
        let has_extra_details = !detail_lines.is_empty() || !preview_lines.is_empty() || has_output;

        if !block.expanded {
            for detail_line in detail_lines.iter() {
                lines.push(RenderedChatLine {
                    display: Line::from(Span::styled(detail_line.clone(), detail_style)),
                    copy_projection: Some(CopyProjection {
                        text: detail_line.clone(),
                        display_start_col: 0,
                    }),
                });
            }

            for preview_line in preview_lines.iter().take(3) {
                let line = Line::from(preview_line.to_string());
                for wrapped in wrap_line_visual(line, content_width.saturating_sub(2)) {
                    let mut spans = vec![Span::raw("  ")];
                    for span in &wrapped.spans {
                        spans.push(Span::styled(
                            span.content.to_string(),
                            span.style.patch(output_style),
                        ));
                    }
                    lines.push(RenderedChatLine {
                        display: Line::from(spans),
                        copy_projection: Some(CopyProjection {
                            text: format!("  {}", line_plain_text(&wrapped)),
                            display_start_col: 0,
                        }),
                    });
                }
            }

            if has_extra_details {
                let expand_hint = "  (ctrl+o to expand)";
                lines.push(RenderedChatLine {
                    display: Line::from(Span::styled(
                        expand_hint,
                        detail_style.add_modifier(Modifier::DIM),
                    )),
                    copy_projection: Some(CopyProjection {
                        text: expand_hint.to_string(),
                        display_start_col: 0,
                    }),
                });
            }
        } else {
            for detail_line in detail_lines.iter() {
                lines.push(RenderedChatLine {
                    display: Line::from(Span::styled(detail_line.clone(), detail_style)),
                    copy_projection: Some(CopyProjection {
                        text: detail_line.clone(),
                        display_start_col: 0,
                    }),
                });
            }

            if has_output {
                let separator = "  ─────────────────";
                lines.push(RenderedChatLine {
                    display: Line::from(Span::styled(separator, detail_style)),
                    copy_projection: Some(CopyProjection {
                        text: separator.to_string(),
                        display_start_col: 0,
                    }),
                });
            }

            for output in output_lines.iter() {
                let line = Line::from(output.to_string());
                for wrapped in wrap_line_visual(line, content_width.saturating_sub(2)) {
                    let mut spans = vec![Span::raw("  ")];
                    for span in &wrapped.spans {
                        spans.push(Span::styled(
                            span.content.to_string(),
                            span.style.patch(output_style),
                        ));
                    }
                    lines.push(RenderedChatLine {
                        display: Line::from(spans),
                        copy_projection: Some(CopyProjection {
                            text: format!("  {}", line_plain_text(&wrapped)),
                            display_start_col: 0,
                        }),
                    });
                }
            }

            if has_extra_details {
                let collapse_hint = "  (ctrl+o to collapse)";
                lines.push(RenderedChatLine {
                    display: Line::from(Span::styled(
                        collapse_hint,
                        detail_style.add_modifier(Modifier::DIM),
                    )),
                    copy_projection: Some(CopyProjection {
                        text: collapse_hint.to_string(),
                        display_start_col: 0,
                    }),
                });
            }
        }

        lines
    }

    fn build_tool_render_data(block: &ChatBlock, tool_name: &str) -> ToolRenderData {
        match tool_name {
            "memory_search" => Self::build_memory_search_render_data(block),
            "exec_command" => Self::build_exec_command_render_data(block),
            "read_file" | "write_file" => Self::build_read_file_render_data(block),
            _ => Self::build_fallback_tool_render_data(block, tool_name),
        }
    }

    fn build_memory_search_render_data(block: &ChatBlock) -> ToolRenderData {
        let query = block
            .tool_input
            .as_ref()
            .and_then(|input| input.get("query"))
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string();
        let parsed = serde_json::from_str::<serde_json::Value>(&block.content).ok();
        let results = parsed
            .as_ref()
            .and_then(|value| value.get("results"))
            .and_then(|value| value.as_array())
            .cloned()
            .unwrap_or_default();
        let hit_count = results.len();
        let timing = Self::timing_suffix(block.timing_ms);
        let summary = match block.tool_status {
            Some(ToolCallStatus::Running) => "└ Running…".to_string(),
            Some(ToolCallStatus::Completed { exit_ok: true }) => {
                format!("└ Done ({} hits{})", hit_count, timing)
            }
            Some(ToolCallStatus::Completed { exit_ok: false }) => {
                format!("└ Error ({} hits{})", hit_count, timing)
            }
            None => "└ Unknown".to_string(),
        };

        let mut detail_lines = vec![];
        if !query.is_empty() {
            detail_lines.push(format!("  query: {}", query));
        }

        let mut preview_lines = vec![];
        if results.is_empty() && matches!(block.tool_status, Some(ToolCallStatus::Completed { .. }))
        {
            preview_lines.push("0 hits".to_string());
        } else {
            for result in results.iter().take(2) {
                if let Some(path) = result.get("path").and_then(|value| value.as_str()) {
                    preview_lines.push(format!("hit: {}", path));
                }
            }
        }

        let mut output_lines = vec![];
        for (index, result) in results.iter().enumerate() {
            if index > 0 {
                output_lines.push(String::new());
            }
            if let Some(path) = result.get("path").and_then(|value| value.as_str()) {
                output_lines.push(format!("hit: {}", path));
            }
            if let Some(score) = result.get("score").and_then(|value| value.as_f64()) {
                output_lines.push(format!("score: {:.2}", score));
            }
            if let Some(snippet) = result.get("snippet").and_then(|value| value.as_str()) {
                output_lines.extend(
                    snippet
                        .lines()
                        .take(3)
                        .map(|line| format!("snippet: {}", line)),
                );
            }
        }

        ToolRenderData {
            summary,
            detail_lines,
            preview_lines,
            output_lines,
        }
    }

    fn build_exec_command_render_data(block: &ChatBlock) -> ToolRenderData {
        let input = block.tool_input.as_ref();
        let cmd = input.and_then(|value| {
            value
                .get("cmd")
                .or_else(|| value.get("command"))
                .and_then(|value| value.as_str())
        });
        let dir = input.and_then(|value| {
            value
                .get("workdir")
                .or_else(|| value.get("cwd"))
                .and_then(|value| value.as_str())
        });
        let parsed = serde_json::from_str::<serde_json::Value>(&block.content).ok();
        let exit_code = parsed
            .as_ref()
            .and_then(|value| value.get("exit_code"))
            .and_then(|value| value.as_i64());
        let output = parsed
            .as_ref()
            .and_then(|value| value.get("output"))
            .and_then(|value| value.as_str())
            .unwrap_or_else(|| block.content.as_str());
        let status = parsed
            .as_ref()
            .and_then(|value| value.get("status"))
            .and_then(|value| value.as_str());
        let timing = Self::timing_suffix(block.timing_ms);

        let summary = match block.tool_status {
            Some(ToolCallStatus::Running) => "└ Running…".to_string(),
            Some(ToolCallStatus::Completed { exit_ok: true }) => {
                if let Some(exit) = exit_code {
                    format!("└ Done (exit:{}{})", exit, timing)
                } else {
                    format!("└ Done{}", timing)
                }
            }
            Some(ToolCallStatus::Completed { exit_ok: false }) => {
                if let Some(exit) = exit_code {
                    format!("└ Error (exit:{}{})", exit, timing)
                } else {
                    format!("└ Error{}", timing)
                }
            }
            None => "└ Unknown".to_string(),
        };

        let mut detail_lines = vec![];
        if let Some(cmd) = cmd {
            detail_lines.push(format!("  cmd:  {}", cmd));
        }
        if let Some(dir) = dir {
            detail_lines.push(format!("  dir:  {}", dir));
        }
        if let Some(status) = status {
            let normalized = match status {
                "exited" => "success",
                other => other,
            };
            detail_lines.push(format!("  status: {}", normalized));
        }

        let mut preview_lines = vec![];
        if let Some(first_line) = output.lines().find(|line| !line.trim().is_empty()) {
            preview_lines.push(format!("stdout: {}", first_line));
        }

        let output_lines = output
            .lines()
            .map(|line| line.to_string())
            .collect::<Vec<_>>();

        ToolRenderData {
            summary,
            detail_lines,
            preview_lines,
            output_lines,
        }
    }

    fn build_read_file_render_data(block: &ChatBlock) -> ToolRenderData {
        let path = block
            .tool_input
            .as_ref()
            .and_then(|input| input.get("path"))
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string();
        let line_count = block.content.lines().count();
        let timing = Self::timing_suffix(block.timing_ms);
        let summary = match block.tool_status {
            Some(ToolCallStatus::Running) => "└ Running…".to_string(),
            Some(ToolCallStatus::Completed { exit_ok: true }) => {
                format!("└ Done ({} lines{})", line_count, timing)
            }
            Some(ToolCallStatus::Completed { exit_ok: false }) => {
                format!("└ Error ({} lines{})", line_count, timing)
            }
            None => "└ Unknown".to_string(),
        };

        let detail_lines = if path.is_empty() {
            vec![]
        } else {
            vec![format!("  path:  {}", path)]
        };
        let preview_lines = block
            .content
            .lines()
            .take(1)
            .map(|line| format!("preview: {}", line))
            .collect::<Vec<_>>();
        let output_lines = block
            .content
            .lines()
            .map(|line| line.to_string())
            .collect::<Vec<_>>();

        ToolRenderData {
            summary,
            detail_lines,
            preview_lines,
            output_lines,
        }
    }

    fn build_fallback_tool_render_data(block: &ChatBlock, tool_name: &str) -> ToolRenderData {
        let detail_lines = block
            .tool_input
            .as_ref()
            .map(|input| Self::fallback_tool_detail_lines(tool_name, input))
            .unwrap_or_default();
        let preview_lines = block
            .content
            .lines()
            .take(2)
            .map(|line| line.to_string())
            .collect::<Vec<_>>();
        let output_lines = block
            .content
            .lines()
            .map(|line| line.to_string())
            .collect::<Vec<_>>();
        let timing = Self::timing_suffix(block.timing_ms);
        let summary = match block.tool_status {
            Some(ToolCallStatus::Running) => "└ Running…".to_string(),
            Some(ToolCallStatus::Completed { exit_ok: true }) => format!("└ Done{}", timing),
            Some(ToolCallStatus::Completed { exit_ok: false }) => format!("└ Error{}", timing),
            None => "└ Unknown".to_string(),
        };

        ToolRenderData {
            summary,
            detail_lines,
            preview_lines,
            output_lines,
        }
    }

    fn fallback_tool_detail_lines(tool_name: &str, input: &serde_json::Value) -> Vec<String> {
        match tool_name {
            "web_search" => input
                .get("query")
                .and_then(|v| v.as_str())
                .map(|query| vec![format!("  query: {}", query)])
                .unwrap_or_default(),
            "http_request" => input
                .get("url")
                .and_then(|v| v.as_str())
                .map(|url| {
                    let method = input
                        .get("method")
                        .and_then(|v| v.as_str())
                        .unwrap_or("GET");
                    vec![format!("  {} {}", method, url)]
                })
                .unwrap_or_default(),
            _ => input
                .as_object()
                .and_then(|m| {
                    m.iter().find_map(|(key, value)| {
                        value.as_str().map(|value| format!("  {}: {}", key, value))
                    })
                })
                .map(|line| vec![line])
                .unwrap_or_default(),
        }
    }

    fn timing_suffix(timing_ms: Option<u64>) -> String {
        timing_ms
            .map(|ms| format!(" · {:.1}s", ms as f64 / 1000.0))
            .unwrap_or_default()
    }

    fn assistant_block_lines(
        &self,
        block: &ChatBlock,
        content_width: usize,
    ) -> Vec<RenderedChatLine> {
        let mut lines = vec![];
        let label_style = Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD);
        lines.push(RenderedChatLine {
            display: Line::from(Span::styled("🤖 Assistant", label_style)),
            copy_projection: Some(CopyProjection {
                text: "Assistant".to_string(),
                display_start_col: 0,
            }),
        });
        let content_lines = render_markdown_lines(&block.content);

        // Content lines with wrapping and proper color
        for line in content_lines.iter() {
            for wrapped in wrap_line_visual(line.clone(), content_width) {
                // Ensure all text is white by default, unless it has a specific color
                let mut colored_spans = vec![];
                for span in &wrapped.spans {
                    let final_style = if span.style.fg.is_some() {
                        span.style // Keep original color if set
                    } else {
                        span.style.fg(Color::White) // Default to white
                    };
                    colored_spans.push(Span::styled(span.content.to_string(), final_style));
                }
                lines.push(RenderedChatLine {
                    display: Line::from(colored_spans),
                    copy_projection: Some(CopyProjection {
                        text: line_plain_text(&wrapped),
                        display_start_col: 0,
                    }),
                });
            }
        }

        lines
    }

    fn thinking_block_lines(
        &self,
        block: &ChatBlock,
        content_width: usize,
    ) -> Vec<RenderedChatLine> {
        let mut lines = vec![];
        let label_style = Style::default()
            .fg(Color::Magenta)
            .add_modifier(Modifier::BOLD | Modifier::ITALIC);
        let body_style = Style::default()
            .fg(Color::Gray)
            .add_modifier(Modifier::ITALIC | Modifier::DIM);

        // Only show if there is content (not empty thinking block)
        if block.content.is_empty() {
            return lines;
        }

        // Add role label
        lines.push(RenderedChatLine {
            display: Line::from(Span::styled("💭 Thinking", label_style)),
            copy_projection: Some(CopyProjection {
                text: "Thinking".to_string(),
                display_start_col: 0,
            }),
        });

        let content_lines = block
            .content
            .lines()
            .map(|line| Line::from(line.to_string()))
            .collect::<Vec<_>>();

        // Content lines with wrapping
        for line in content_lines.iter() {
            for wrapped in wrap_line_visual(line.clone(), content_width) {
                lines.push(self.decorate_chat_line(wrapped, body_style, 0));
            }
        }

        lines
    }

    fn system_block_lines(
        &self,
        block: &ChatBlock,
        _content_width: usize,
    ) -> Vec<RenderedChatLine> {
        let mut lines = vec![];
        let gear_style = Style::default().fg(Color::Yellow);

        let content_lines: Vec<&str> = block.content.lines().collect();
        if content_lines.is_empty() {
            return lines;
        }

        // Check if it's a welcome card (starts with ╭)
        let is_welcome_card = content_lines[0].starts_with('╭');

        if is_welcome_card {
            // Welcome card: render all lines without icon prefix
            for line in &content_lines {
                let display_line = Line::from(line.to_string());
                lines.push(RenderedChatLine {
                    display: display_line,
                    copy_projection: Some(CopyProjection {
                        text: line.to_string(),
                        display_start_col: 0,
                    }),
                });
            }
        } else {
            // Regular system message: add icon prefix
            let first_line = content_lines[0];
            let mut first_spans = vec![Span::styled("⚙  ", gear_style)];
            first_spans.push(Span::styled(
                first_line.to_string(),
                Style::default().fg(Color::White),
            ));

            lines.push(RenderedChatLine {
                display: Line::from(first_spans),
                copy_projection: Some(CopyProjection {
                    text: format!("⚙  {}", first_line),
                    display_start_col: 0,
                }),
            });

            // Additional lines without prefix
            for line in content_lines.iter().skip(1) {
                let display_line = Line::from(vec![
                    Span::styled(
                        "│  ",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::DIM),
                    ),
                    Span::styled(line.to_string(), Style::default().fg(Color::White)),
                ]);
                lines.push(RenderedChatLine {
                    display: display_line,
                    copy_projection: Some(CopyProjection {
                        text: format!("│  {}", line),
                        display_start_col: 0,
                    }),
                });
            }
        }

        lines
    }

    fn decorate_chat_line(
        &self,
        line: Line<'static>,
        body_style: Style,
        indent: usize,
    ) -> RenderedChatLine {
        let indent_str = " ".repeat(indent);
        let mut spans: Vec<Span<'static>> = vec![];

        if !indent_str.is_empty() {
            spans.push(Span::raw(indent_str.clone()));
        }

        if line.spans.is_empty() {
            return RenderedChatLine {
                display: Line::from(spans),
                copy_projection: Some(CopyProjection {
                    text: String::new(),
                    display_start_col: indent,
                }),
            };
        }

        let copy_text = line_plain_text(&line);
        for span in line.spans {
            spans.push(Span::styled(
                span.content.to_string(),
                span.style.patch(body_style),
            ));
        }
        RenderedChatLine {
            display: Line::from(spans),
            copy_projection: Some(CopyProjection {
                text: copy_text,
                display_start_col: indent,
            }),
        }
    }

    fn tool_arg_summary(tool_name: &str, input: &serde_json::Value) -> String {
        let summary = match tool_name {
            "exec_command" => input
                .get("cmd")
                .or_else(|| input.get("command"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_default(),
            "read_file" | "write_file" => input
                .get("path")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_default(),
            "web_search" => input
                .get("query")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_default(),
            "http_request" => input
                .get("url")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_default(),
            _ => input
                .as_object()
                .and_then(|m| m.values().find_map(|v| v.as_str()))
                .map(|s| s.to_string())
                .unwrap_or_default(),
        };
        if summary.len() > 70 {
            format!("{}...", &summary[..67])
        } else {
            summary
        }
    }

    fn spinning_color(spinner_index: usize) -> Color {
        match spinner_index % 4 {
            0 => Color::Cyan,
            1 => Color::Blue,
            2 => Color::Magenta,
            _ => Color::Cyan,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use talos_client::{
        TalosEventPayload, TalosMessageCompletedEvent, TalosMessageOutputDeltaEvent,
        TalosMessageReasoningDeltaEvent, TalosMessageStartedEvent, TalosThinkingSource,
        TalosThinkingStage,
    };

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
                stop_reason: "end_turn".to_string(),
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
    }

    fn create_embedded_app() -> TuiApp {
        create_remote_app()
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
        assert!(app.available_models.iter().any(|m| m == "qwen-plus"));
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
    async fn test_thinking_block_is_initialized_before_first_delta() {
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
        assert!(thinking.is_empty());
    }

    #[tokio::test]
    async fn test_reasoning_delta_renders_plain_text_and_preserves_paragraphs() {
        let mut app = create_remote_app();
        app.initialize_remote().await;
        app.session_id = Some("sess-test".to_string());

        app.handle_stream_event(reasoning_delta_event(
            "msg-think",
            "sess-test",
            1,
            "First line",
        ));
        app.handle_stream_event(reasoning_delta_event("msg-think", "sess-test", 2, "\n\n"));
        app.handle_stream_event(reasoning_delta_event(
            "msg-think",
            "sess-test",
            3,
            "Second paragraph",
        ));

        let thinking = app
            .chat_blocks
            .iter()
            .find(|b| b.role == ChatRole::Thinking)
            .map(|b| b.content.clone())
            .unwrap_or_default();

        assert_eq!(thinking, "First line\n\nSecond paragraph");
        assert!(!thinking.contains("model"));
        assert!(!thinking.contains("step-"));
        assert!(!thinking.contains("#1"));
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
        app.handle_stream_event(output_delta_event(
            stale_message_id,
            "sess-test",
            4,
            "stale",
        ));

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
    async fn test_message_completed_finishes_stream_without_output_delta() {
        let mut app = create_remote_app();
        app.initialize_remote().await;
        app.in_flight = true;
        app.received_output_delta = false;
        app.active_assistant_idx = Some(app.push_block(ChatRole::Assistant, String::new()));
        app.session_id = Some("sess-test".to_string());

        app.handle_stream_event(completed_event(
            "msg-new",
            "sess-test",
            1,
            TalosUsage::default(),
        ));

        assert!(!app.in_flight);
    }

    #[tokio::test]
    async fn test_stream_event_enables_follow_bottom_mode() {
        let mut app = create_remote_app();
        app.initialize_remote().await;
        app.in_flight = true;
        app.chat_manual_scroll = true;
        app.session_id = Some("sess-test".to_string());

        app.handle_stream_event(output_delta_event("msg-follow", "sess-test", 1, "hello"));

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

        app.handle_stream_event(completed_event(
            "msg-usage",
            "sess-test",
            1,
            TalosUsage {
                input_tokens: 964,
                output_tokens: 16,
                total_tokens: 980,
            },
        ));

        assert_eq!(app.session_usage.total_tokens, 980);
        assert_eq!(app.token_left_percent(), Some(97));
    }

    #[test]
    fn test_chat_card_wrapped_lines_keep_indent() {
        let app = create_embedded_app();
        let block = ChatBlock::new(
            ChatRole::Assistant,
            "这是一个非常长的回答，用于测试换行后是否仍然带有缩进。".to_string(),
        );
        let lines = app.chat_card_lines(&block, 12);
        assert!(!lines.is_empty());
        assert!(line_plain_text(&lines[0].display).starts_with("🤖 Assistant"));
        assert!(lines.len() > 1);
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
        for line in lines.iter() {
            let width = line_display_width(&line.display);
            assert!(
                width <= max_line_width,
                "line width {} exceeds max {}",
                width,
                max_line_width
            );
        }
    }

    #[test]
    fn test_selected_chat_text_excludes_border_glyphs() {
        let mut app = create_embedded_app();
        app.chat_blocks = vec![ChatBlock::new(
            ChatRole::Assistant,
            "Alpha Beta".to_string(),
        )];
        app.rebuild_chat_render_state(Rect::new(0, 0, 20, 8));
        app.chat_selection = Some(ChatSelection {
            anchor: SelectionPoint {
                line_index: 1,
                column: 0,
            },
            focus: SelectionPoint {
                line_index: 1,
                column: 10,
            },
        });

        assert_eq!(app.selected_chat_text().as_deref(), Some("Alpha Beta"));
    }

    #[test]
    fn test_selected_chat_text_across_wrapped_lines_preserves_newlines() {
        let mut app = create_embedded_app();
        app.chat_blocks = vec![ChatBlock::new(
            ChatRole::Assistant,
            "abcdefghij".to_string(),
        )];
        app.rebuild_chat_render_state(Rect::new(0, 0, 8, 8));
        // Line 0 is the assistant header, then content width = 6 wraps to ["abcdef", "ghij"].
        app.chat_selection = Some(ChatSelection {
            anchor: SelectionPoint {
                line_index: 1,
                column: 0,
            },
            focus: SelectionPoint {
                line_index: 2,
                column: 4,
            },
        });

        assert_eq!(app.selected_chat_text().as_deref(), Some("abcdef\nghij"));
    }

    #[test]
    fn test_mouse_drag_selection_stays_inside_chat_rect() {
        let mut app = create_embedded_app();
        app.chat_blocks = vec![ChatBlock::new(ChatRole::Assistant, "Alpha".to_string())];
        app.rebuild_chat_render_state(Rect::new(4, 3, 20, 8));

        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 0,
            row: 0,
            modifiers: KeyModifiers::empty(),
        });
        assert!(app.chat_selection.is_none());

        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 6,
            row: 4,
            modifiers: KeyModifiers::empty(),
        });
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 10,
            row: 4,
            modifiers: KeyModifiers::empty(),
        });

        assert_eq!(
            app.chat_selection,
            Some(ChatSelection {
                anchor: SelectionPoint {
                    line_index: 1,
                    column: 2,
                },
                focus: SelectionPoint {
                    line_index: 1,
                    column: 6,
                },
            })
        );
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

    // ── Phase 3: tool call inline cards ──────────────────────────────────

    #[test]
    fn test_tool_call_block_pushed_on_started_event() {
        let mut app = create_remote_app();
        app.session_id = Some("sess".to_string());

        let event = TalosEvent {
            event_type: TalosEventType::MessageToolCallStarted,
            message_id: "msg1".to_string(),
            session_id: "sess".to_string(),
            sequence: 1,
            payload: TalosEventPayload::MessageToolCallStarted(
                talos_client::TalosMessageToolCallStartedEvent {
                    request_id: "req1".to_string(),
                    call_id: "call-1".to_string(),
                    name: "bash".to_string(),
                    arguments: serde_json::json!({"command": "ls"}),
                    item_index: 0,
                    iteration: 1,
                    step_id: "step-1".to_string(),
                    timing_ms: None,
                },
            ),
        };

        app.handle_stream_event(event);

        let tool_block = app.chat_blocks.iter().find(|b| b.role == ChatRole::Tool);
        assert!(tool_block.is_some(), "expected tool block to be added");
        let block = tool_block.unwrap();
        assert_eq!(block.tool_name.as_deref(), Some("bash"));
        assert_eq!(block.tool_status, Some(ToolCallStatus::Running));
        assert!(app.active_tool_calls.contains_key("call-1"));
    }

    #[test]
    fn test_tool_call_block_completed_on_event() {
        let mut app = create_remote_app();
        app.session_id = Some("sess".to_string());

        // First push a started event
        let started = TalosEvent {
            event_type: TalosEventType::MessageToolCallStarted,
            message_id: "msg1".to_string(),
            session_id: "sess".to_string(),
            sequence: 1,
            payload: TalosEventPayload::MessageToolCallStarted(
                talos_client::TalosMessageToolCallStartedEvent {
                    request_id: "req1".to_string(),
                    call_id: "call-2".to_string(),
                    name: "read_file".to_string(),
                    arguments: serde_json::json!({"path": "/tmp/x"}),
                    item_index: 0,
                    iteration: 1,
                    step_id: "step-1".to_string(),
                    timing_ms: None,
                },
            ),
        };
        app.handle_stream_event(started);

        let completed = TalosEvent {
            event_type: TalosEventType::MessageToolCallCompleted,
            message_id: "msg1".to_string(),
            session_id: "sess".to_string(),
            sequence: 2,
            payload: TalosEventPayload::MessageToolCallCompleted(
                talos_client::TalosMessageToolCallCompletedEvent {
                    request_id: "req1".to_string(),
                    call_id: "call-2".to_string(),
                    tool: "read_file".to_string(),
                    content: vec![talos_client::TalosContentItem::Text {
                        text: "file contents".to_string(),
                    }],
                    is_error: false,
                    item_index: 0,
                    iteration: 1,
                    step_id: "step-1".to_string(),
                    timing_ms: Some(42),
                },
            ),
        };
        app.handle_stream_event(completed);

        let tool_block = app.chat_blocks.iter().find(|b| b.role == ChatRole::Tool);
        assert!(tool_block.is_some());
        let block = tool_block.unwrap();
        assert_eq!(
            block.tool_status,
            Some(ToolCallStatus::Completed { exit_ok: true })
        );
        assert_eq!(block.content, "file contents");
        assert_eq!(block.timing_ms, Some(42));
        // Should be removed from active after completion
        assert!(!app.active_tool_calls.contains_key("call-2"));
    }

    #[test]
    fn test_tool_completed_event_updates_block_even_with_different_message_id() {
        let mut app = create_remote_app();
        app.session_id = Some("sess".to_string());
        app.active_stream_message_id = Some("msg-active".to_string());
        app.last_stream_sequence = Some(10);
        app.push_tool_call_block(
            "call-stale".to_string(),
            "memory_search".to_string(),
            serde_json::json!({"query": "talos 进展"}),
            None,
        );

        let completed = TalosEvent {
            event_type: TalosEventType::MessageToolCallCompleted,
            message_id: "msg-other".to_string(),
            session_id: "sess".to_string(),
            sequence: 11,
            payload: TalosEventPayload::MessageToolCallCompleted(
                talos_client::TalosMessageToolCallCompletedEvent {
                    request_id: "req1".to_string(),
                    call_id: "call-stale".to_string(),
                    tool: "memory_search".to_string(),
                    content: vec![talos_client::TalosContentItem::Json {
                        value: serde_json::json!({"results": []}),
                    }],
                    is_error: false,
                    item_index: 0,
                    iteration: 1,
                    step_id: "step-1".to_string(),
                    timing_ms: Some(120),
                },
            ),
        };

        app.handle_stream_event(completed);

        let idx = app
            .chat_blocks
            .iter()
            .position(|b| b.tool_call_id.as_deref() == Some("call-stale"))
            .unwrap();
        assert_eq!(
            app.chat_blocks[idx].tool_status,
            Some(ToolCallStatus::Completed { exit_ok: true })
        );
        assert_eq!(app.chat_blocks[idx].timing_ms, Some(120));
        assert!(!app.active_tool_calls.contains_key("call-stale"));
    }

    #[test]
    fn test_tool_completed_event_updates_block_even_when_sequence_is_older() {
        let mut app = create_remote_app();
        app.session_id = Some("sess".to_string());
        app.active_stream_message_id = Some("msg-active".to_string());
        app.last_stream_sequence = Some(20);
        app.push_tool_call_block(
            "call-older".to_string(),
            "exec_command".to_string(),
            serde_json::json!({"cmd": "pwd"}),
            None,
        );

        let completed = TalosEvent {
            event_type: TalosEventType::MessageToolCallCompleted,
            message_id: "msg-active".to_string(),
            session_id: "sess".to_string(),
            sequence: 5,
            payload: TalosEventPayload::MessageToolCallCompleted(
                talos_client::TalosMessageToolCallCompletedEvent {
                    request_id: "req1".to_string(),
                    call_id: "call-older".to_string(),
                    tool: "exec_command".to_string(),
                    content: vec![talos_client::TalosContentItem::Json {
                        value: serde_json::json!({
                            "session_id": null,
                            "status": "exited",
                            "output": "/tmp\n",
                            "exit_code": 0,
                            "truncated": false
                        }),
                    }],
                    is_error: false,
                    item_index: 0,
                    iteration: 1,
                    step_id: "step-1".to_string(),
                    timing_ms: Some(15),
                },
            ),
        };

        app.handle_stream_event(completed);

        let idx = app
            .chat_blocks
            .iter()
            .position(|b| b.tool_call_id.as_deref() == Some("call-older"))
            .unwrap();
        assert_eq!(
            app.chat_blocks[idx].tool_status,
            Some(ToolCallStatus::Completed { exit_ok: true })
        );
        assert!(!app.active_tool_calls.contains_key("call-older"));
    }

    #[test]
    fn test_tool_block_tab_toggle_expand() {
        let mut app = create_remote_app();
        app.session_id = Some("sess".to_string());
        app.active_stream_message_id = Some("msg1".to_string());

        app.push_tool_call_block(
            "call-3".to_string(),
            "grep".to_string(),
            serde_json::json!({"pattern": "foo"}),
            None,
        );

        let idx = *app.active_tool_calls.get("call-3").unwrap();
        assert!(!app.chat_blocks[idx].expanded);

        app.toggle_last_tool_block();
        assert!(app.chat_blocks[idx].expanded);

        app.toggle_last_tool_block();
        assert!(!app.chat_blocks[idx].expanded);
    }

    #[tokio::test]
    async fn test_ctrl_o_toggles_last_tool_block() {
        let mut app = create_remote_app();
        app.push_tool_call_block(
            "call-ctrl-o".to_string(),
            "grep".to_string(),
            serde_json::json!({"pattern": "foo"}),
            None,
        );
        let idx = *app.active_tool_calls.get("call-ctrl-o").unwrap();
        assert!(!app.chat_blocks[idx].expanded);

        let _ = app
            .handle_input(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL))
            .await
            .unwrap();
        assert!(app.chat_blocks[idx].expanded);
    }

    #[test]
    fn test_tool_block_folded_shows_details_and_output_preview() {
        let app = create_embedded_app();
        let mut block = ChatBlock::new(
            ChatRole::Tool,
            r#"{"session_id":null,"status":"exited","output":"line1\nline2\nline3\nline4","exit_code":0,"truncated":false}"#
                .to_string(),
        );
        block.tool_name = Some("exec_command".to_string());
        block.tool_input = Some(serde_json::json!({
            "cmd": "git diff HEAD~1",
            "workdir": "/tmp/work"
        }));
        block.tool_status = Some(ToolCallStatus::Completed { exit_ok: true });
        block.timing_ms = Some(1200);
        block.expanded = false;

        let lines = app.tool_block_lines(&block, 80);
        let texts = lines
            .iter()
            .map(|line| line_plain_text(&line.display))
            .collect::<Vec<_>>();

        assert!(texts
            .iter()
            .any(|line| line == "● exec_command(git diff HEAD~1)"));
        assert!(texts.iter().any(|line| line == "└ Done (exit:0 · 1.2s)"));
        assert!(texts.iter().any(|line| line == "  cmd:  git diff HEAD~1"));
        assert!(texts.iter().any(|line| line == "  dir:  /tmp/work"));
        assert!(texts.iter().any(|line| line == "  status: success"));
        assert!(texts.iter().any(|line| line == "  stdout: line1"));
        assert!(texts.iter().any(|line| line == "  (ctrl+o to expand)"));
    }

    #[test]
    fn test_tool_block_expanded_shows_details_without_output() {
        let app = create_embedded_app();
        let mut block = ChatBlock::new(ChatRole::Tool, String::new());
        block.tool_name = Some("read_file".to_string());
        block.tool_input = Some(serde_json::json!({
            "path": "/tmp/example.txt"
        }));
        block.tool_status = Some(ToolCallStatus::Completed { exit_ok: true });
        block.expanded = true;

        let lines = app.tool_block_lines(&block, 80);
        let texts = lines
            .iter()
            .map(|line| line_plain_text(&line.display))
            .collect::<Vec<_>>();

        assert!(texts.iter().any(|line| line == "  path:  /tmp/example.txt"));
        assert!(!texts.iter().any(|line| line == "  ─────────────────"));
        assert!(texts.iter().any(|line| line == "  (ctrl+o to collapse)"));
    }

    #[test]
    fn test_memory_search_folded_shows_query_and_hits() {
        let app = create_embedded_app();
        let mut block = ChatBlock::new(
            ChatRole::Tool,
            r#"{"results":[{"path":"spec/tui-improvement.md","snippet":"TUI layout summary","citation":"spec/tui-improvement.md#L1","score":0.91,"start_line":1,"end_line":1},{"path":"spec/tui-claude-code-style.md","snippet":"Claude style summary","citation":"spec/tui-claude-code-style.md#L1","score":0.87,"start_line":1,"end_line":1}]}"#
                .to_string(),
        );
        block.tool_name = Some("memory_search".to_string());
        block.tool_input = Some(serde_json::json!({
            "query": "talos 项目结构"
        }));
        block.tool_status = Some(ToolCallStatus::Completed { exit_ok: true });
        block.timing_ms = Some(800);

        let texts = app
            .tool_block_lines(&block, 100)
            .into_iter()
            .map(|line| line_plain_text(&line.display))
            .collect::<Vec<_>>();

        assert!(texts
            .iter()
            .any(|line| line == "● memory_search(talos 项目结构)"));
        assert!(texts.iter().any(|line| line == "└ Done (2 hits · 0.8s)"));
        assert!(texts.iter().any(|line| line == "  query: talos 项目结构"));
        assert!(texts
            .iter()
            .any(|line| line == "  hit: spec/tui-improvement.md"));
        assert!(texts
            .iter()
            .any(|line| line == "  hit: spec/tui-claude-code-style.md"));
    }

    #[test]
    fn test_complete_tool_call_recovers_after_block_index_shift() {
        let mut app = create_embedded_app();
        let user_idx = app.push_block(ChatRole::User, "hello".to_string());
        let thinking_idx = app.push_block(ChatRole::Thinking, String::new());
        let assistant_idx = app.push_block(ChatRole::Assistant, String::new());
        let tool_idx = app.push_tool_call_block(
            "call-shift".to_string(),
            "exec_command".to_string(),
            serde_json::json!({"command": "pwd"}),
            None,
        );

        app.active_user_idx = Some(user_idx);
        app.active_thinking_idx = Some(thinking_idx);
        app.active_assistant_idx = Some(assistant_idx);
        app.discard_active_pending_blocks();

        let shifted_idx = *app
            .active_tool_calls
            .get("call-shift")
            .expect("tool block index should be remapped");
        assert!(shifted_idx < tool_idx);
        assert_eq!(
            app.chat_blocks[shifted_idx].tool_call_id.as_deref(),
            Some("call-shift")
        );

        app.complete_tool_call_block("call-shift", "ok".to_string(), true, Some(15));

        assert_eq!(app.chat_blocks[shifted_idx].content, "ok");
        assert_eq!(
            app.chat_blocks[shifted_idx].tool_status,
            Some(ToolCallStatus::Completed { exit_ok: true })
        );
        assert_eq!(app.chat_blocks[shifted_idx].timing_ms, Some(15));
        assert!(!app.active_tool_calls.contains_key("call-shift"));
    }

    #[test]
    fn test_system_block_lines_keep_multiline_structure() {
        let app = create_embedded_app();
        let block = ChatBlock::new(ChatRole::System, "System ready\nLoading tools".to_string());

        let lines = app.system_block_lines(&block, 80);
        let texts = lines
            .iter()
            .map(|line| line_plain_text(&line.display))
            .collect::<Vec<_>>();

        assert_eq!(texts, vec!["⚙  System ready", "│  Loading tools"]);
    }

    #[test]
    fn test_system_block_lines_render_welcome_card_without_prefix() {
        let app = create_embedded_app();
        let block = ChatBlock::new(ChatRole::System, "╭────╮\n│ hi │\n╰────╯".to_string());

        let lines = app.system_block_lines(&block, 80);
        let texts = lines
            .iter()
            .map(|line| line_plain_text(&line.display))
            .collect::<Vec<_>>();

        assert_eq!(texts, vec!["╭────╮", "│ hi │", "╰────╯"]);
    }

    // ── Phase 4: markdown rendering ──────────────────────────────────────

    #[test]
    fn test_markdown_bold_renders_bold_span() {
        let lines = render_markdown_lines("Hello **world** text");
        assert_eq!(lines.len(), 1);
        let has_bold = lines[0]
            .spans
            .iter()
            .any(|s| s.content.contains("world") && s.style.add_modifier.contains(Modifier::BOLD));
        assert!(has_bold, "expected 'world' to be bold");
    }

    #[test]
    fn test_markdown_italic_renders_italic_span() {
        let lines = render_markdown_lines("Hello *italic* text");
        assert_eq!(lines.len(), 1);
        let has_italic = lines[0].spans.iter().any(|s| {
            s.content.contains("italic") && s.style.add_modifier.contains(Modifier::ITALIC)
        });
        assert!(has_italic, "expected 'italic' to be italic");
    }

    #[test]
    fn test_markdown_inline_code_uses_green_fg() {
        let lines = render_markdown_lines("Run `cargo build` now");
        assert_eq!(lines.len(), 1);
        let has_code = lines[0]
            .spans
            .iter()
            .any(|s| s.content.contains("cargo build") && s.style.fg == Some(Color::LightGreen));
        assert!(has_code, "expected inline code to have LightGreen fg");
    }

    #[test]
    fn test_markdown_header_h1_has_prefix() {
        let lines = render_markdown_lines("# Title");
        assert_eq!(lines.len(), 1);
        let has_prefix = lines[0]
            .spans
            .first()
            .map(|s| s.content.contains('▌'))
            .unwrap_or(false);
        assert!(has_prefix, "H1 should have ▌ prefix span");
    }

    #[test]
    fn test_markdown_list_item_has_bullet() {
        let lines = render_markdown_lines("- item one");
        assert_eq!(lines.len(), 1);
        let has_bullet = lines[0].spans.iter().any(|s| s.content.contains('•'));
        assert!(has_bullet, "list item should have • bullet");
    }

    #[test]
    fn test_markdown_code_block_uses_distinct_style() {
        let md = "text\n```rust\nlet x = 1;\n```\nafter";
        let lines = render_markdown_lines(md);
        // Line 0: "text", line 1: fence label, line 2: code, line 3: close fence, line 4: "after"
        assert!(lines.len() >= 3);
        // The code line should have LightGreen fg
        let code_line = &lines[2];
        let is_green = code_line
            .spans
            .iter()
            .any(|s| s.style.fg == Some(Color::LightGreen));
        assert!(is_green, "code block lines should be LightGreen");
    }

    #[test]
    fn test_assistant_block_renders_full_markdown_without_role_label() {
        let app = create_embedded_app();
        let block = ChatBlock::new(
            ChatRole::Assistant,
            "## Summary\n\n1. First\n2. Second\n\n```rust\nlet x = 1;\n```".to_string(),
        );

        let lines = app.assistant_block_lines(&block, 80);
        let texts = lines
            .iter()
            .map(|line| line_plain_text(&line.display))
            .collect::<Vec<_>>();

        assert_eq!(texts.first().map(String::as_str), Some("🤖 Assistant"));
        assert!(texts.iter().any(|line| line == "▌ Summary"));
        assert!(texts.iter().any(|line| line == "1. First"));
        assert!(texts.iter().any(|line| line == "2. Second"));
        assert!(texts.iter().any(|line| line == "─── rust ───"));
        assert!(texts.iter().any(|line| line == "let x = 1;"));
        assert_eq!(
            texts
                .iter()
                .filter(|line| line.starts_with("🤖 Assistant"))
                .count(),
            1
        );
    }

    #[test]
    fn test_chat_content_columns_apply_left_padding() {
        let app = create_embedded_app();
        let (chat_rect, scrollbar_rect) = app.chat_content_columns(Rect::new(1, 1, 20, 6));

        assert_eq!(chat_rect, Rect::new(3, 1, 17, 6));
        assert_eq!(scrollbar_rect, Rect::new(20, 1, 1, 6));
    }

    #[test]
    fn test_right_panel_hidden_by_default() {
        let app = create_embedded_app();
        assert!(!app.right_panel_visible);
    }

    // ── Phase 5: history + model picker ──────────────────────────────────

    #[tokio::test]
    async fn test_ctrl_h_switches_to_history_state() {
        let mut app = create_remote_app();
        app.initialize_remote().await;
        assert_eq!(app.current_state, AppState::Chat);

        app.open_history();

        assert_eq!(app.current_state, AppState::History);
    }

    #[tokio::test]
    async fn test_ctrl_b_toggles_right_panel() {
        let mut app = create_remote_app();
        assert!(!app.right_panel_visible);

        let _ = app
            .handle_input(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL))
            .await
            .unwrap();
        assert!(app.right_panel_visible);

        let _ = app
            .handle_input(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL))
            .await
            .unwrap();
        assert!(!app.right_panel_visible);
    }

    #[test]
    fn test_toggle_model_picker_shows_picker() {
        let mut app = create_remote_app();
        app.available_models = vec!["gpt-4o".to_string(), "qwen-plus".to_string()];
        app.current_model = "gpt-4o".to_string();
        assert!(!app.show_model_picker);

        app.toggle_model_picker();
        assert!(app.show_model_picker);
        // Pre-selects current model index
        assert_eq!(app.model_picker_selected, 0);

        // Toggle again closes
        app.toggle_model_picker();
        assert!(!app.show_model_picker);
    }

    #[test]
    fn test_toggle_model_picker_without_models_shows_hint() {
        let mut app = create_remote_app();
        app.available_models.clear();
        app.toggle_model_picker();
        assert!(
            !app.show_model_picker,
            "picker should not open without models"
        );
        assert!(app
            .chat_blocks
            .iter()
            .any(|b| b.content.contains("No models")));
    }

    #[tokio::test]
    async fn test_command_autocomplete_inserts_command_only() {
        let mut app = create_remote_app();

        let _ = app
            .handle_input(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE))
            .await
            .unwrap();
        let _ = app
            .handle_input(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE))
            .await
            .unwrap();
        let _ = app
            .handle_input(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .await
            .unwrap();

        assert_eq!(app.input_buffer, "/sessions");
        assert_eq!(app.cursor_index, "/sessions".chars().count());
        assert!(!app.show_command_autocomplete);
    }

    #[test]
    fn test_command_autocomplete_panel_height_is_inline_and_capped() {
        let mut app = create_remote_app();
        app.show_command_autocomplete = true;
        app.command_suggestions = CommandSuggestion::all();

        assert_eq!(app.command_autocomplete_panel_height(), 7);

        app.command_suggestions.clear();
        assert_eq!(app.command_autocomplete_panel_height(), 1);
    }

    #[tokio::test]
    async fn test_history_enter_switches_session() {
        let mut app = create_remote_app();
        app.initialize_remote().await;

        let mut config = talos_types::TalosConfig::default();
        config.llm.model = "claude-3".to_string();
        app.history_sessions = vec![TalosSession {
            context: talos_types::SessionContext {
                session_id: "hist-sess-1".to_string(),
                workspace_id: None,
                source_type: talos_types::talos::TalosSourceType::Http,
                provider: talos_types::talos::TalosProvider::Sdk,
                user_id: "u".to_string(),
                message_id: "m".to_string(),
                trace_id: "t".to_string(),
                channel_id: None,
                thread_id: None,
            },
            config,
            state: talos_types::SessionState {
                created_at: 0,
                last_activity: 0,
                message_count: 3,
                usage: talos_types::talos::TalosUsage::default(),
            },
        }];
        app.current_state = AppState::History;
        app.history_selected = 0;

        app.switch_to_history_session();

        assert_eq!(app.session_id.as_deref(), Some("hist-sess-1"));
        assert_eq!(app.current_model, "claude-3");
        assert_eq!(app.current_state, AppState::Chat);
    }
}
