use std::collections::{HashMap, VecDeque};
use std::env;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use talos_sdk::{SessionInfo, TalosContext, TalosProvider, TalosSourceType};
use talos_types::talos::TalosUsage;

use crate::client::GatewayClient;
use crate::render;
use crate::types::{AppState, ChatRole, CopyProjection, RenderedChatLine};
use crate::utils::{build_welcome_card, APP_VERSION};
use crate::{TuiApp, CHAT_CONTENT_PADDING_LEFT, WELCOME_MODEL_PLACEHOLDER};

const SESSION_INIT_MAX_ATTEMPTS: usize = 5;
const SESSION_INIT_RETRY_DELAY_MS: u64 = 250;

impl TuiApp {
    pub(crate) fn build(gateway_client: Option<Arc<dyn GatewayClient>>) -> Self {
        let mut app = Self {
            current_state: AppState::Chat,
            input_buffer: String::new(),
            cursor_index: 0,
            pasted_blocks: Vec::new(),
            selected_pasted_block: None,
            startup_directory: env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            current_project_root: None,
            current_model: String::new(),
            session_usage: TalosUsage::default(),
            turn_metrics: None,
            turn_metrics_live: false,
            available_model_info: vec![],
            available_models: vec![],
            session_id: None,
            connection_status: "disconnected".to_string(),
            chat_blocks: vec![],
            chat_history: vec![],
            right_panel_events: vec![],
            tool_events: vec![],
            rendered_chat_lines: vec![],
            rendered_round_footer_buttons: HashMap::new(),
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
            active_message_skills: Vec::new(),
            active_retry_count: 0,
            active_user_idx: None,
            transcript: Default::default(),
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
            last_stream_event_id: None,
            timeline_items: HashMap::new(),
            status_hint: None,
            spinner_index: 0,
            stripe_phase: 0,
            stream_started_at: None,
            last_stream_event_at: None,
            gateway_client,
            history_sessions: vec![],
            history_selected: 0,
            show_model_picker: false,
            model_picker_selected: 0,
            show_sessions_picker: false,
            sessions_picker_selected: 0,
            sent_messages_history: VecDeque::new(),
            message_history_index: None,
            show_command_autocomplete: false,
            command_suggestions: Vec::new(),
            command_autocomplete_selected: 0,
            command_input_buffer: String::new(),
            show_skill_autocomplete: false,
            skill_suggestions: Vec::new(),
            skill_autocomplete_selected: 0,
            selected_skills: Vec::new(),
            close_in_progress: false,
            close_started_at: None,
            close_timeout_reported: false,
            closing_session_id: None,
            should_exit: false,
            left_panel_area: None,
            input_area: None,
        };

        app.current_project_root = Some(app.startup_directory.clone());

        app.push_block(
            ChatRole::System,
            build_welcome_card(
                APP_VERSION,
                WELCOME_MODEL_PLACEHOLDER,
                app.current_project_root.as_deref(),
            ),
        );
        app.push_block(
            ChatRole::System,
            "Type a message or use /help /model /history /sessions /status /compact /clear /quit"
                .to_string(),
        );
        app.push_right_event("tui initialized".to_string());
        app
    }

    pub fn new_with_gateway_client(gateway_client: Arc<dyn GatewayClient>) -> Self {
        Self::build(Some(gateway_client))
    }

    pub(crate) fn build_talos_context(&self) -> TalosContext {
        let user_id = env::var("USER").unwrap_or_else(|_| "local-user".to_string());
        TalosContext {
            source_type: TalosSourceType::Cli,
            provider: TalosProvider::Cli,
            user_id: user_id.clone(),
            message_id: format!("msg-{}", uuid::Uuid::new_v4().simple()),
            trace_id: format!("trace-{}", uuid::Uuid::new_v4().simple()),
            channel_id: Some("terminal".to_string()),
            thread_id: Some(user_id),
            project_root: None,
            environment: None,
        }
    }

    pub async fn initialize_session(&mut self) {
        self.connection_status = "connecting".to_string();
        self.push_right_event("connecting to embedded runtime".to_string());

        if let Err(err) = self.refresh_models_with_retry().await {
            self.push_block(
                ChatRole::System,
                format!("Failed to load models from gateway: {}", err),
            );
            self.connection_status = "error".to_string();
            return;
        }

        self.connection_status = "connected".to_string();
        self.push_right_event("draft chat ready".to_string());
    }

    async fn refresh_models_with_retry(&mut self) -> talos_error::TalosResult<()> {
        let mut last_error = None;
        for attempt in 1..=SESSION_INIT_MAX_ATTEMPTS {
            match self.refresh_models_from_gateway().await {
                Ok(()) => return Ok(()),
                Err(err) => {
                    last_error = Some(err);
                    if attempt == SESSION_INIT_MAX_ATTEMPTS {
                        break;
                    }
                    self.push_right_event(format!(
                        "model load retry {}/{}",
                        attempt + 1,
                        SESSION_INIT_MAX_ATTEMPTS
                    ));
                    tokio::time::sleep(Self::session_init_retry_delay(attempt)).await;
                }
            }
        }
        Err(last_error.expect("retry loop should capture the final model load error"))
    }

    fn session_init_retry_delay(attempt: usize) -> Duration {
        Duration::from_millis(SESSION_INIT_RETRY_DELAY_MS.saturating_mul(attempt as u64))
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

    pub(crate) fn is_agent_working(&self) -> bool {
        self.in_flight && !self.transcript.has_visible_activity
    }

    pub(crate) fn thinking_spinner_line(&self, spinner_index: usize) -> RenderedChatLine {
        let color = render::helpers::spinning_color(spinner_index);
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

    pub(crate) fn apply_session_snapshot(&mut self, session: &SessionInfo) {
        self.session_id = Some(session.id.clone());
        self.current_model = session.model.clone();
        self.session_usage = session.usage.clone();
        self.turn_metrics = None;
        self.turn_metrics_live = false;
        self.current_project_root = session
            .current_directory
            .clone()
            .or_else(|| Some(self.startup_directory.clone()));
        if let Some(welcome_block) = self.chat_blocks.first_mut() {
            if welcome_block.content.starts_with('╭') {
                welcome_block.content = build_welcome_card(
                    APP_VERSION,
                    &self.current_model,
                    self.current_project_root.as_deref(),
                );
            }
        }
        self.rebuild_chat_render_state_if_rect();
    }

    pub(crate) fn reset_chat_surface(&mut self) {
        self.chat_blocks.clear();
        self.rendered_chat_lines.clear();
        self.chat_history.clear();
        self.tool_events.clear();
        self.right_panel_events.clear();
        self.rendered_round_footer_buttons.clear();
        self.chat_selection = None;
        self.active_message = None;
        self.active_message_raw = None;
        self.active_message_skills.clear();
        self.active_user_idx = None;
        self.turn_metrics = None;
        self.turn_metrics_live = false;
        self.received_output_delta = false;
        self.chat_scroll = 0;
        self.chat_manual_scroll = false;
        self.sent_messages_history.clear();
        self.message_history_index = None;
        self.transcript = Default::default();
        self.push_block(
            ChatRole::System,
            build_welcome_card(
                APP_VERSION,
                &self.current_model,
                self.current_project_root.as_deref(),
            ),
        );
        self.push_block(
            ChatRole::System,
            "Type a message or use /help /model /history /sessions /status /compact /clear /quit"
                .to_string(),
        );
        self.rebuild_chat_render_state_if_rect();
    }

    pub(crate) fn stripe_line(&self, width: usize) -> String {
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

    pub(crate) fn chat_content_columns(&self, chat_inner: Rect) -> (Rect, Rect) {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_session_context_uses_terminal_channel() {
        let app = TuiApp::build(None);
        let context = app.build_talos_context();

        assert_eq!(context.source_type, TalosSourceType::Cli);
        assert_eq!(context.provider, TalosProvider::Cli);
        assert_eq!(context.channel_id.as_deref(), Some("terminal"));
    }
}
