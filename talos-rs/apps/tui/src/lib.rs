//! Talos Terminal User Interface (TUI)
//!
//! Gateway mode supports real session APIs and SSE streaming.

mod app;
mod attachments;
mod chat;
mod client;
mod clipboard;
mod commands;
mod frame_fingerprint;
mod input;
mod input_history;
mod message_queue;
mod model_picker;
mod model_selection;
mod permission_picker;
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
mod terminal_surface;
mod turn_request;
mod types;
mod user_action;
mod utils;

pub use client::{GatewayClient, SessionSubscription};
pub use run::run_tui_app_with_gateway_client;
pub use turn_request::TerminalTurnRequest;
use types::*;
pub use utils::display_tool_name;

use input_history::InputHistoryState;
use model_picker::ModelPickerState;
use permission_picker::PermissionPickerState;
use ratatui::layout::Rect;
use render_runtime::runtime::engine::RenderRuntimeEngine;
use render_runtime::transcript::block::TranscriptBlockId;
use session_picker::SessionPickerState;
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use talos_error::TalosResult;
use talos_sdk::{
    AgentRunMetrics, ContextCompactionStage, MessageQueue, ModelInfo, ReasoningEffort,
    SkillSuggestion, TimelineEvent, UserPermissionMode,
};
use talos_types::talos::TokenUsage;
use tokio::sync::mpsc;

pub(crate) const MAX_RIGHT_EVENTS: usize = 120;
pub(crate) const STREAM_STALL_TIMEOUT: Duration = Duration::from_secs(60);
pub(crate) const SESSION_CLOSE_SLOW_NOTICE: Duration = Duration::from_secs(4);
pub(crate) const SESSION_CLOSE_FORCE_TIMEOUT: Duration = Duration::from_secs(65);
pub(crate) const SPINNER_FRAMES: &[char] = &['✻', '✢', '✳', '✶'];
pub(crate) const CHAT_CONTENT_PADDING_LEFT: u16 = 0;
pub(crate) const WELCOME_TIP_LINE: &str =
    " Tip: Type a message, Ctrl+V an image, or use /help /model /permissions /sessions /status /compact /clear /quit";
pub(crate) const PASTED_BLOCK_CHAR_THRESHOLD: usize = 800;

/// Main TUI app.
pub struct TuiApp {
    pub(crate) input_buffer: String,
    pub(crate) cursor_index: usize,
    pub(crate) pasted_blocks: Vec<PastedTextBlock>,
    pub(crate) selected_pasted_block: Option<usize>,
    pub(crate) pasted_image_attachments: Vec<PastedImageAttachment>,
    pub(crate) pasted_image_sequence: u64,
    pub(crate) startup_directory: PathBuf,
    pub(crate) current_project_root: Option<PathBuf>,
    pub(crate) current_project_id: Option<String>,
    pub(crate) current_model: String,
    pub(crate) current_model_provider: String,
    pub(crate) reasoning_effort_override: Option<ReasoningEffort>,
    pub(crate) current_permission_mode: UserPermissionMode,
    pub(crate) session_usage: TokenUsage,
    pub(crate) session_context_usage: Option<talos_sdk::ContextUsage>,
    pub(crate) turn_metrics: Option<AgentRunMetrics>,
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
    pub(crate) message_queue: MessageQueue,
    pub(crate) session_subscription: Option<client::SessionSubscription>,
    pub(crate) session_subscription_session_id: Option<String>,
    pub(crate) message_queue_retry_at: Option<Instant>,
    pub(crate) message_queue_turn_reconcile_pending: bool,
    pub(crate) selected_queue_message: Option<usize>,
    pub(crate) editing_queue_message_id: Option<String>,
    pub(crate) trace_event_count: u64,
    pub(crate) in_flight: bool,
    pub(crate) received_output_delta: bool,
    pub(crate) active_message: Option<String>,
    pub(crate) active_message_raw: Option<String>,
    pub(crate) active_message_skills: Vec<SkillSuggestion>,
    pub(crate) active_message_file_ids: Vec<String>,
    pub(crate) active_retry_count: u8,
    pub(crate) pending_session_connect: Option<mpsc::Receiver<TalosResult<String>>>,
    pub(crate) pending_session_task: Option<tokio::task::JoinHandle<()>>,
    pub(crate) pending_stream_connect:
        Option<mpsc::Receiver<TalosResult<mpsc::Receiver<TimelineEvent>>>>,
    pub(crate) pending_stream_task: Option<tokio::task::JoinHandle<()>>,
    pub(crate) pending_stream: Option<mpsc::Receiver<TimelineEvent>>,
    pub(crate) pending_command_update: Option<mpsc::Receiver<CommandUpdate>>,
    pub(crate) pending_command_task: Option<tokio::task::JoinHandle<()>>,
    pub(crate) pending_session_usage_refresh: Option<mpsc::Receiver<types::SessionUsageRefresh>>,
    pub(crate) pending_session_usage_refresh_task: Option<tokio::task::JoinHandle<()>>,
    pub(crate) pending_close_update: Option<mpsc::Receiver<types::SessionCloseUpdate>>,
    pub(crate) pending_close_task: Option<tokio::task::JoinHandle<()>>,
    pub(crate) active_stream_turn_id: Option<String>,
    pub(crate) last_stream_sequence: Option<u64>,
    pub(crate) last_stream_event_id: Option<String>,
    pub(crate) context_compaction_stage: Option<ContextCompactionStage>,
    pub(crate) status_hint: Option<(String, Instant)>,
    pub(crate) spinner_index: usize,
    pub(crate) stream_started_at: Option<Instant>,
    pub(crate) last_stream_event_at: Option<Instant>,
    pub(crate) gateway_client: Option<Arc<dyn GatewayClient>>,
    pub(crate) model_picker: Option<ModelPickerState>,
    pub(crate) permission_picker: Option<PermissionPickerState>,
    pub(crate) pending_user_action: Option<user_action::PendingActionState>,
    pub(crate) session_picker: Option<SessionPickerState>,
    pub(crate) input_history: InputHistoryState,
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
    pub(crate) close_slow_notice_reported: bool,
    pub(crate) closing_session_id: Option<String>,
    pub(crate) client_shutdown_completed: bool,
    pub(crate) force_exit_without_shutdown: bool,
    pub(crate) should_exit: bool,
    pub(crate) left_panel_area: Option<Rect>,
    pub(crate) input_area: Option<Rect>,
    pub(crate) render_runtime: RenderRuntimeEngine,
    pub(crate) terminal_footer_synced_for: Option<TranscriptBlockId>,
}

impl TuiApp {
    #[cfg(test)]
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
