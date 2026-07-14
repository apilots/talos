use std::collections::{HashMap, VecDeque};
use std::env;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use talos_sdk::{
    LlmReasoningEffort, ModelInfo, ModelRef, ModelSelection, ReasoningSelection, SessionInfo,
    TalosContext, TalosProvider, TalosSourceType,
};
use talos_types::talos::TokenUsage;

use crate::client::GatewayClient;
use crate::model_picker::ReasoningOption;
use crate::render;
use crate::render_runtime::common::style::StyledLine;
use crate::types::RenderedChatLine;
use crate::utils::{build_welcome_card, APP_VERSION};
use crate::{TuiApp, CHAT_CONTENT_PADDING_LEFT, SPINNER_FRAMES, WELCOME_TIP_LINE};

const SESSION_INIT_MAX_ATTEMPTS: usize = 5;
const SESSION_INIT_RETRY_DELAY_MS: u64 = 250;

impl TuiApp {
    pub(crate) fn build(gateway_client: Option<Arc<dyn GatewayClient>>) -> Self {
        let mut app = Self {
            input_buffer: String::new(),
            cursor_index: 0,
            pasted_blocks: Vec::new(),
            selected_pasted_block: None,
            startup_directory: env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            current_project_root: None,
            current_model: String::new(),
            reasoning_effort_override: None,
            session_usage: TokenUsage::default(),
            turn_metrics: None,
            turn_metrics_live: false,
            available_model_info: vec![],
            available_models: vec![],
            session_id: None,
            connection_status: "disconnected".to_string(),
            right_panel_events: vec![],
            rendered_chat_lines: vec![],
            rendered_chat_cache_key: None,
            chat_content_rect: None,
            chat_viewport_height: 0,
            message_queue: VecDeque::new(),
            trace_event_count: 0,
            in_flight: false,
            received_output_delta: false,
            active_message: None,
            active_message_raw: None,
            active_message_skills: Vec::new(),
            active_retry_count: 0,
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
            stream_started_at: None,
            last_stream_event_at: None,
            gateway_client,
            model_picker: None,
            session_picker: None,
            input_history: crate::input_history::InputHistoryState::default(),
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
            close_slow_notice_reported: false,
            closing_session_id: None,
            client_shutdown_completed: false,
            force_exit_without_shutdown: false,
            should_exit: false,
            left_panel_area: None,
            input_area: None,
            render_runtime: crate::render_runtime::runtime::engine::RenderRuntimeEngine::default(),
            terminal_footer_synced_for: None,
        };

        app.current_project_root = Some(app.startup_directory.clone());

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

    pub(crate) fn current_model_selection(&self) -> Option<ModelSelection> {
        let model = self.current_model_info()?;
        let model_ref = ModelRef {
            provider: model.provider.clone(),
            id: model.id.clone(),
        };
        let reasoning = match self.reasoning_effort_override {
            Some(level) if model.reasoning => ReasoningSelection::Effort { level },
            _ => ReasoningSelection::ModelDefault,
        };
        Some(ModelSelection {
            model_ref,
            reasoning,
        })
    }

    pub(crate) fn current_model_info(&self) -> Option<&ModelInfo> {
        self.available_model_info
            .iter()
            .find(|model| model.id == self.current_model)
    }

    pub(crate) fn model_info_for_id(&self, model_id: &str) -> Option<&ModelInfo> {
        self.available_model_info
            .iter()
            .find(|model| model.id == model_id)
    }

    pub(crate) fn use_model_default_reasoning(&mut self) {
        self.reasoning_effort_override = None;
    }

    pub(crate) fn set_reasoning_effort_override(&mut self, effort: Option<LlmReasoningEffort>) {
        self.reasoning_effort_override = if self
            .current_model_info()
            .is_some_and(|model| model.reasoning)
        {
            effort
        } else {
            None
        };
    }

    pub(crate) fn model_status_label(&self, model_id: &str) -> String {
        let Some(model) = self.model_info_for_id(model_id) else {
            return model_id.to_string();
        };
        if !model.reasoning {
            return model_id.to_string();
        }
        let effort = if model.id == self.current_model {
            self.reasoning_effort_override
                .unwrap_or(model.reasoning_effort)
        } else {
            model.reasoning_effort
        };
        format!("{} {}", model_id, ReasoningOption::status_label_for(effort))
    }

    pub(crate) fn current_model_status_label(&self) -> String {
        self.model_status_label(&self.current_model)
    }

    pub async fn initialize_session(&mut self) {
        self.connection_status = "connecting".to_string();
        self.push_right_event("connecting to embedded runtime".to_string());

        if let Err(err) = self.refresh_models_with_retry().await {
            self.push_system_notice(format!("Failed to load models from gateway: {}", err));
            self.connection_status = "error".to_string();
            return;
        }

        self.connection_status = "connected".to_string();
        self.push_right_event("draft chat ready".to_string());
        self.sync_session_header();
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

    pub(crate) fn is_agent_working(&self) -> bool {
        self.in_flight
    }

    pub(crate) fn thinking_spinner_line(&self, spinner_index: usize) -> RenderedChatLine {
        let color = render::helpers::spinning_color(spinner_index);
        let marker = SPINNER_FRAMES[spinner_index % SPINNER_FRAMES.len()];
        let text = format!("{marker} Brewing...");

        let display = Line::from(vec![Span::styled(
            text.clone(),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )]);

        RenderedChatLine { display }
    }

    pub(crate) fn apply_session_snapshot(&mut self, session: &SessionInfo) {
        self.session_id = Some(session.id.clone());
        self.current_model = session.model_selection.model_ref.id.clone();
        match session.model_selection.reasoning {
            ReasoningSelection::ModelDefault => self.use_model_default_reasoning(),
            ReasoningSelection::Effort { level } => {
                self.set_reasoning_effort_override(Some(level));
            }
        }
        self.session_usage = session.usage.clone();
        self.turn_metrics = None;
        self.turn_metrics_live = false;
        self.current_project_root = session
            .current_directory
            .clone()
            .or_else(|| Some(self.startup_directory.clone()));
        self.sync_session_header();
        self.rebuild_chat_render_state_if_rect();
    }

    pub(crate) fn reset_chat_surface(&mut self) {
        self.render_runtime.reset_transcript();
        self.sync_session_header();
        self.rendered_chat_lines.clear();
        self.right_panel_events.clear();
        self.rendered_chat_cache_key = None;
        self.active_message = None;
        self.active_message_raw = None;
        self.active_message_skills.clear();
        self.turn_metrics = None;
        self.turn_metrics_live = false;
        self.terminal_footer_synced_for = None;
        self.received_output_delta = false;
        self.input_history.clear();
        self.rebuild_chat_render_state_if_rect();
    }

    pub(crate) fn sync_session_header(&mut self) {
        let card = build_welcome_card(
            APP_VERSION,
            &self.current_model,
            self.current_project_root.as_deref(),
        );
        self.render_runtime.set_session_header(
            card.lines()
                .map(|line| StyledLine::plain(line.to_string()))
                .chain(std::iter::once(StyledLine::plain(WELCOME_TIP_LINE)))
                .collect(),
        );
    }

    pub(crate) fn chat_content_rect(&self, chat_inner: Rect) -> Rect {
        let chat_columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(CHAT_CONTENT_PADDING_LEFT),
                Constraint::Min(1),
            ])
            .split(chat_inner);
        chat_columns[1]
    }
}

#[cfg(test)]
#[path = "app_tests.rs"]
mod tests;
