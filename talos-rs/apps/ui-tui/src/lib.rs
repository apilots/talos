//! Talos Terminal User Interface (TUI)
//!
//! Gateway mode supports real session APIs and SSE streaming.

mod app;
mod chat;
mod client;
mod commands;
mod input;
mod markdown;
mod render;
pub mod run;
mod selection;
mod stream;
mod types;
mod utils;

pub use client::GatewayClient;
pub use run::run_tui_app_with_client;
use types::*;

use chrono::{Datelike, Local, TimeZone};
use ratatui::layout::Rect;
use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use talos_client::{ModelInfo, TalosEvent, TalosSession};
use talos_error::TalosResult;
use talos_types::talos::{TalosConnectionMode, TalosUsage};
use tokio::sync::mpsc;

pub(crate) const WELCOME_MODEL_PLACEHOLDER: &str = "(connecting...)";
pub(crate) const MAX_RIGHT_EVENTS: usize = 120;
pub(crate) const MAX_QUEUE_LEN: usize = 3;
pub(crate) const STREAM_STALL_TIMEOUT: Duration = Duration::from_secs(60);
pub(crate) const SESSION_CLOSE_TIMEOUT: Duration = Duration::from_secs(4);
pub(crate) const SPINNER_FRAMES: &[char] = &['|', '/', '-', '\\'];
pub(crate) const CHAT_CONTENT_PADDING_LEFT: u16 = 2;
pub(crate) const CHAT_BLOCK_BODY_INDENT: usize = 2;
pub(crate) const TOOL_PREVIEW_CHAR_LIMIT: usize = 100;
pub(crate) const COPY_ROUND_BUTTON_TEXT: &str = "  [ Copy Round ]";
pub(crate) const PASTED_BLOCK_CHAR_THRESHOLD: usize = 800;

fn format_session_timestamp(timestamp_ms: u64) -> String {
    let Some(local_dt) = timestamp_ms
        .try_into()
        .ok()
        .and_then(|millis| Local.timestamp_millis_opt(millis).single())
    else {
        return "unknown time".to_string();
    };

    let now = Local::now();
    if now.date_naive() == local_dt.date_naive() {
        return local_dt.format("%H:%M").to_string();
    }

    if now.year() == local_dt.year() {
        return local_dt.format("%m-%d %H:%M").to_string();
    }

    local_dt.format("%Y-%m-%d %H:%M").to_string()
}

pub(crate) fn session_list_label(session: &TalosSession) -> String {
    let title = session.title.trim();
    let time_text = format_session_timestamp(session.state.last_activity);
    format!(" {}  {}", title, time_text)
}

/// Main TUI app.
pub struct TuiApp {
    pub(crate) current_state: AppState,
    pub(crate) input_buffer: String,
    pub(crate) cursor_index: usize,
    pub(crate) pasted_blocks: Vec<PastedTextBlock>,
    pub(crate) selected_pasted_block: Option<usize>,
    pub(crate) startup_directory: PathBuf,
    pub(crate) connection_mode: TalosConnectionMode,
    pub(crate) current_model: String,
    pub(crate) session_usage: TalosUsage,
    pub(crate) available_model_info: Vec<ModelInfo>,
    pub(crate) available_models: Vec<String>,
    pub(crate) session_id: Option<String>,
    pub(crate) connection_status: String,
    pub(crate) chat_blocks: Vec<ChatBlock>,
    pub(crate) chat_history: Vec<String>,
    pub(crate) right_panel_events: Vec<String>,
    pub(crate) tool_events: Vec<String>,
    pub(crate) rendered_chat_lines: Vec<RenderedChatLine>,
    pub(crate) rendered_round_footer_buttons: HashMap<usize, RoundFooterButton>,
    pub(crate) chat_content_rect: Option<Rect>,
    pub(crate) chat_viewport_height: usize,
    pub(crate) chat_selection: Option<ChatSelection>,
    pub(crate) message_queue: VecDeque<QueuedMessage>,
    pub(crate) chat_scroll: u16,
    pub(crate) chat_manual_scroll: bool,
    pub(crate) trace_event_count: u64,
    pub(crate) in_flight: bool,
    pub(crate) received_output_delta: bool,
    pub(crate) active_message: Option<String>,
    pub(crate) active_message_raw: Option<String>,
    pub(crate) active_retry_count: u8,
    pub(crate) pending_assistant_output: String,
    pub(crate) stream_has_tool_calls: bool,
    pub(crate) active_user_idx: Option<usize>,
    pub(crate) active_thinking_idx: Option<usize>,
    pub(crate) active_assistant_idx: Option<usize>,
    pub(crate) pending_session_connect: Option<mpsc::Receiver<TalosResult<String>>>,
    pub(crate) pending_session_task: Option<tokio::task::JoinHandle<()>>,
    pub(crate) pending_stream_connect:
        Option<mpsc::Receiver<TalosResult<mpsc::Receiver<TalosEvent>>>>,
    pub(crate) pending_stream_task: Option<tokio::task::JoinHandle<()>>,
    pub(crate) pending_stream: Option<mpsc::Receiver<TalosEvent>>,
    pub(crate) pending_command_update: Option<mpsc::Receiver<RemoteCommandUpdate>>,
    pub(crate) pending_command_task: Option<tokio::task::JoinHandle<()>>,
    pub(crate) pending_close_update: Option<mpsc::Receiver<TalosResult<String>>>,
    pub(crate) pending_close_task: Option<tokio::task::JoinHandle<()>>,
    pub(crate) active_stream_message_id: Option<String>,
    pub(crate) last_stream_sequence: Option<u64>,
    pub(crate) status_hint: Option<(String, Instant)>,
    pub(crate) spinner_index: usize,
    pub(crate) stripe_phase: usize,
    pub(crate) stream_started_at: Option<Instant>,
    pub(crate) last_stream_event_at: Option<Instant>,
    pub(crate) gateway_client: Option<Arc<dyn GatewayClient>>,
    pub(crate) pending_approval: Option<PendingApprovalState>,
    pub(crate) active_tool_calls: HashMap<String, usize>,
    pub(crate) history_sessions: Vec<TalosSession>,
    pub(crate) history_selected: usize,
    pub(crate) show_model_picker: bool,
    pub(crate) model_picker_selected: usize,
    pub(crate) show_sessions_picker: bool,
    pub(crate) sessions_picker_selected: usize,
    pub(crate) right_panel_visible: bool,
    pub(crate) sent_messages_history: VecDeque<String>,
    pub(crate) message_history_index: Option<usize>,
    pub(crate) show_command_autocomplete: bool,
    pub(crate) command_suggestions: Vec<CommandSuggestion>,
    pub(crate) command_autocomplete_selected: usize,
    pub(crate) command_input_buffer: String,
    pub(crate) close_in_progress: bool,
    pub(crate) close_started_at: Option<Instant>,
    pub(crate) close_timeout_reported: bool,
    pub(crate) closing_session_id: Option<String>,
    pub(crate) should_exit: bool,
    pub(crate) left_panel_area: Option<Rect>,
    pub(crate) input_area: Option<Rect>,
}

impl TuiApp {
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
mod test_support;

#[cfg(test)]
pub(crate) mod tests;

#[cfg(test)]
mod e2e_tests;
