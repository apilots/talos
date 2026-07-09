//! Talos Terminal User Interface (TUI)
//!
//! Gateway mode supports real session APIs and SSE streaming.

mod app;
mod chat;
mod client;
mod commands;
mod input;
mod model_picker;
mod render;
mod render_runtime;
pub mod run;
mod session_picker;
mod status_picker;
mod stream;
mod stream_events;
mod stream_lifecycle;
mod stream_updates;
mod terminal_adapter;
mod turn_request;
mod types;
mod utils;

pub use client::GatewayClient;
pub use run::run_tui_app_with_gateway_client;
pub use turn_request::TerminalTurnRequest;
use types::*;
pub use utils::display_tool_name;

use chrono::{Datelike, Local, TimeZone};
use model_picker::ModelPickerState;
use ratatui::layout::Rect;
use render_runtime::runtime::engine::BufferedRenderRuntimeEngine;
use render_runtime::transcript::block::TranscriptBlockId;
use session_picker::SessionPickerState;
use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use talos_error::TalosResult;
use talos_sdk::{
    LlmReasoningEffort, ModelInfo, SkillSuggestion, TalosTimelineItem, TimelineEventEnvelope,
};
use talos_types::talos::{TokenUsage, TurnExecutionMetrics};
use tokio::sync::mpsc;

pub(crate) const MAX_RIGHT_EVENTS: usize = 120;
pub(crate) const MAX_QUEUE_LEN: usize = 3;
pub(crate) const STREAM_STALL_TIMEOUT: Duration = Duration::from_secs(60);
pub(crate) const SESSION_CLOSE_TIMEOUT: Duration = Duration::from_secs(4);
pub(crate) const SPINNER_FRAMES: &[char] = &['✻', '✢', '✳', '✶'];
pub(crate) const CHAT_CONTENT_PADDING_LEFT: u16 = 0;
pub(crate) const WELCOME_TIP_LINE: &str =
    " Tip: Type a message or use /help /model /sessions /status /compact /clear /quit";
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

/// Main TUI app.
pub struct TuiApp {
    pub(crate) input_buffer: String,
    pub(crate) cursor_index: usize,
    pub(crate) pasted_blocks: Vec<PastedTextBlock>,
    pub(crate) selected_pasted_block: Option<usize>,
    pub(crate) startup_directory: PathBuf,
    pub(crate) current_project_root: Option<PathBuf>,
    pub(crate) current_model: String,
    pub(crate) current_reasoning_effort: Option<LlmReasoningEffort>,
    pub(crate) session_usage: TokenUsage,
    pub(crate) turn_metrics: Option<TurnExecutionMetrics>,
    pub(crate) turn_metrics_live: bool,
    pub(crate) available_model_info: Vec<ModelInfo>,
    pub(crate) available_models: Vec<String>,
    pub(crate) session_id: Option<String>,
    pub(crate) connection_status: String,
    pub(crate) right_panel_events: Vec<String>,
    pub(crate) rendered_chat_lines: Vec<RenderedChatLine>,
    pub(crate) rendered_chat_cache_key: Option<ChatRenderCacheKey>,
    pub(crate) chat_content_rect: Option<Rect>,
    pub(crate) chat_viewport_height: usize,
    pub(crate) message_queue: VecDeque<QueuedMessage>,
    pub(crate) trace_event_count: u64,
    pub(crate) in_flight: bool,
    pub(crate) received_output_delta: bool,
    pub(crate) active_message: Option<String>,
    pub(crate) active_message_raw: Option<String>,
    pub(crate) active_message_skills: Vec<SkillSuggestion>,
    pub(crate) active_retry_count: u8,
    pub(crate) pending_session_connect: Option<mpsc::Receiver<TalosResult<String>>>,
    pub(crate) pending_session_task: Option<tokio::task::JoinHandle<()>>,
    pub(crate) pending_stream_connect:
        Option<mpsc::Receiver<TalosResult<mpsc::Receiver<TimelineEventEnvelope>>>>,
    pub(crate) pending_stream_task: Option<tokio::task::JoinHandle<()>>,
    pub(crate) pending_stream: Option<mpsc::Receiver<TimelineEventEnvelope>>,
    pub(crate) pending_command_update: Option<mpsc::Receiver<CommandUpdate>>,
    pub(crate) pending_command_task: Option<tokio::task::JoinHandle<()>>,
    pub(crate) pending_close_update: Option<mpsc::Receiver<TalosResult<String>>>,
    pub(crate) pending_close_task: Option<tokio::task::JoinHandle<()>>,
    pub(crate) active_stream_message_id: Option<String>,
    pub(crate) last_stream_sequence: Option<u64>,
    pub(crate) last_stream_event_id: Option<String>,
    pub(crate) timeline_items: HashMap<String, TalosTimelineItem>,
    pub(crate) status_hint: Option<(String, Instant)>,
    pub(crate) spinner_index: usize,
    pub(crate) stream_started_at: Option<Instant>,
    pub(crate) last_stream_event_at: Option<Instant>,
    pub(crate) gateway_client: Option<Arc<dyn GatewayClient>>,
    pub(crate) model_picker: Option<ModelPickerState>,
    pub(crate) session_picker: Option<SessionPickerState>,
    pub(crate) sent_messages_history: VecDeque<String>,
    pub(crate) message_history_index: Option<usize>,
    pub(crate) show_command_autocomplete: bool,
    pub(crate) command_suggestions: Vec<CommandSuggestion>,
    pub(crate) command_autocomplete_selected: usize,
    pub(crate) command_input_buffer: String,
    pub(crate) show_skill_autocomplete: bool,
    pub(crate) skill_suggestions: Vec<SkillSuggestion>,
    pub(crate) skill_autocomplete_selected: usize,
    pub(crate) selected_skills: Vec<SkillSuggestion>,
    pub(crate) close_in_progress: bool,
    pub(crate) close_started_at: Option<Instant>,
    pub(crate) close_timeout_reported: bool,
    pub(crate) closing_session_id: Option<String>,
    pub(crate) should_exit: bool,
    pub(crate) left_panel_area: Option<Rect>,
    pub(crate) input_area: Option<Rect>,
    pub(crate) render_runtime: BufferedRenderRuntimeEngine,
    pub(crate) suppress_transcript_for_terminal_flush: bool,
    pub(crate) terminal_footer_synced_for: Option<TranscriptBlockId>,
}

impl TuiApp {
    pub(crate) fn displayed_usage(&self) -> TokenUsage {
        let mut usage = self.session_usage.clone();
        if self.turn_metrics_live {
            if let Some(metrics) = &self.turn_metrics {
                usage.accumulate(&metrics.usage);
            }
        }
        usage
    }

    pub(crate) fn compact_turn_metrics_text(&self, width: usize) -> Option<String> {
        let metrics = self.turn_metrics.as_ref()?;
        let state = if self.turn_metrics_live {
            "live"
        } else {
            "last"
        };
        let duration = metrics
            .duration_ms
            .map(format_duration_ms)
            .unwrap_or_else(|| "--".to_string());
        if width >= 92 {
            Some(format!(
                "{state} · {} rounds · {} tools · {} ok · {} err · {} tokens · {duration}",
                metrics.model_rounds,
                metrics.tool_calls,
                metrics.tool_successes,
                metrics.tool_errors,
                format_metric_number(metrics.usage.total_tokens),
            ))
        } else {
            Some(format!(
                "{state} · {}r · {}t · {}ok · {}err · {} tok · {duration}",
                metrics.model_rounds,
                metrics.tool_calls,
                metrics.tool_successes,
                metrics.tool_errors,
                format_metric_number(metrics.usage.total_tokens),
            ))
        }
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

fn format_duration_ms(duration_ms: u64) -> String {
    if duration_ms >= 1_000 {
        format!("{:.1}s", duration_ms as f64 / 1_000.0)
    } else {
        format!("{duration_ms}ms")
    }
}

fn format_metric_number(value: u32) -> String {
    if value >= 1_000_000 {
        format!("{:.1}m", value as f64 / 1_000_000.0)
    } else if value >= 1_000 {
        format!("{:.1}k", value as f64 / 1_000.0)
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod test_support;

#[cfg(test)]
pub(crate) mod tests;
