use std::collections::hash_map::DefaultHasher;
use std::fmt::Debug;
use std::hash::{Hash, Hasher};

use crate::TuiApp;

pub(crate) fn tui_frame_fingerprint(app: &TuiApp) -> u64 {
    let mut hasher = DefaultHasher::new();
    app.input_buffer.hash(&mut hasher);
    app.cursor_index.hash(&mut hasher);
    (&app.pasted_blocks, &app.pasted_image_attachments).hash(&mut hasher);
    app.selected_pasted_block.hash(&mut hasher);
    (&app.current_model_provider, &app.current_model).hash(&mut hasher);
    hash_debug(&app.reasoning_effort_override, &mut hasher);
    app.connection_status.hash(&mut hasher);
    hash_debug(&app.rendered_chat_cache_key, &mut hasher);
    hash_debug(&app.render_runtime.live_projection_lines(), &mut hasher);
    hash_debug(&app.message_queue, &mut hasher);
    app.in_flight.hash(&mut hasher);
    app.received_output_delta.hash(&mut hasher);
    hash_debug(&app.turn_metrics, &mut hasher);
    app.turn_metrics_live.hash(&mut hasher);
    app.session_usage.total_tokens.hash(&mut hasher);
    hash_debug(&app.session_context_usage, &mut hasher);
    app.trace_event_count.hash(&mut hasher);
    app.status_hint
        .as_ref()
        .map(|(text, _)| text)
        .hash(&mut hasher);
    if app.is_agent_working() {
        app.spinner_index.hash(&mut hasher);
    }
    hash_debug(&app.model_picker, &mut hasher);
    hash_debug(&app.session_picker, &mut hasher);
    app.show_command_autocomplete.hash(&mut hasher);
    app.command_autocomplete_selected.hash(&mut hasher);
    app.command_input_buffer.hash(&mut hasher);
    hash_debug(&app.command_suggestions, &mut hasher);
    app.show_skill_autocomplete.hash(&mut hasher);
    app.skill_autocomplete_selected.hash(&mut hasher);
    hash_debug(&app.skill_suggestions, &mut hasher);
    app.close_in_progress.hash(&mut hasher);
    app.close_slow_notice_reported.hash(&mut hasher);
    app.client_shutdown_completed.hash(&mut hasher);
    app.force_exit_without_shutdown.hash(&mut hasher);
    app.should_exit.hash(&mut hasher);
    hasher.finish()
}

fn hash_debug<T: Debug>(value: &T, hasher: &mut DefaultHasher) {
    format!("{value:?}").hash(hasher);
}
