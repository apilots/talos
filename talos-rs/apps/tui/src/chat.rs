use std::time::Duration;

use crate::render_runtime::common::style::StyledLine;
use crate::render_runtime::transcript::block::{FinalizedTranscriptBlock, TranscriptBlockKind};
use crate::{TuiApp, MAX_RIGHT_EVENTS};

impl TuiApp {
    pub(crate) fn request_chat_activity(&mut self) {
        self.rendered_chat_cache_key = None;
    }

    pub(crate) fn push_system_notice(&mut self, content: impl Into<String>) {
        let content = content.into();
        self.render_runtime.append_system_notice(content);
        self.request_chat_activity();
    }

    pub(crate) fn push_command_result(&mut self, content: impl Into<String>) {
        self.render_runtime
            .append_history([FinalizedTranscriptBlock::new(
                TranscriptBlockKind::FinalAnswer,
                vec![StyledLine::plain(content.into())],
            )]);
        self.request_chat_activity();
    }

    pub(crate) fn push_right_event(&mut self, event: String) {
        if self
            .right_panel_events
            .last()
            .map(|last| last == &event)
            .unwrap_or(false)
        {
            return;
        }

        let coalesced_prefixes = ["model load retry", "session create retry"];
        if let Some(prefix) = coalesced_prefixes
            .iter()
            .find(|prefix| event.starts_with(**prefix))
        {
            if let Some(last) = self.right_panel_events.last_mut() {
                if last.starts_with(prefix) {
                    *last = event;
                    return;
                }
            }
        }

        self.right_panel_events.push(event);
        if self.right_panel_events.len() > MAX_RIGHT_EVENTS {
            let overflow = self.right_panel_events.len() - MAX_RIGHT_EVENTS;
            self.right_panel_events.drain(0..overflow);
        }
    }

    pub(crate) fn show_tool_details_hint(&mut self) {
        self.show_status_hint("Tool details are shown inline.", Duration::from_secs(2));
    }
}
