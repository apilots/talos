use crate::types::ChatRole;
use crate::{TuiApp, MAX_RIGHT_EVENTS};

impl TuiApp {
    pub(crate) fn finish_live_text_blocks(&mut self) {
        if let Some(idx) = self.transcript.live_assistant_idx.take() {
            let should_remove = self
                .chat_blocks
                .get(idx)
                .map(|block| block.content.trim().is_empty())
                .unwrap_or(false);
            if should_remove {
                self.remove_chat_block(idx);
                self.rebuild_chat_history();
            }
        }
        self.transcript.live_thinking_idx = None;
    }

    pub(crate) fn append_assistant(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let idx = if let Some(idx) = self.transcript.live_assistant_idx {
            idx
        } else {
            let idx = self.push_block(ChatRole::Assistant, String::new());
            self.transcript.live_assistant_idx = Some(idx);
            idx
        };

        if let Some(block) = self.chat_blocks.get_mut(idx) {
            block.content.push_str(text);
        }
        self.sync_history_entry(idx);
        self.request_follow_bottom();
    }

    pub(crate) fn append_thinking(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let idx = if let Some(idx) = self.transcript.live_thinking_idx {
            idx
        } else {
            let idx = self.push_block(ChatRole::Thinking, String::new());
            self.transcript.live_thinking_idx = Some(idx);
            idx
        };

        if let Some(block) = self.chat_blocks.get_mut(idx) {
            block.content.push_str(text);
        }
        self.sync_history_entry(idx);
        self.request_follow_bottom();
    }

    pub(crate) fn clear_live_tool_progress(&mut self, call_id: &str) {
        let _ = call_id;
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
        if self.tool_events.len() > MAX_RIGHT_EVENTS {
            let overflow = self.tool_events.len() - MAX_RIGHT_EVENTS;
            self.tool_events.drain(0..overflow);
        }
    }

    pub(crate) fn upsert_tool_event(&mut self, call_id: String, summary: String) {
        if let Some(existing) = self
            .tool_events
            .iter_mut()
            .find(|event| event.call_id == call_id)
        {
            existing.summary = summary;
            return;
        }

        self.tool_events
            .push(crate::types::ToolPanelEvent { call_id, summary });
        if self.tool_events.len() > MAX_RIGHT_EVENTS {
            let overflow = self.tool_events.len() - MAX_RIGHT_EVENTS;
            self.tool_events.drain(0..overflow);
        }
    }

    pub(crate) fn sync_history_entry(&mut self, idx: usize) {
        if let Some(block) = self.chat_blocks.get(idx) {
            let entry = format!("{}\n{}", block.role.as_str(), block.content);
            if idx < self.chat_history.len() {
                self.chat_history[idx] = entry;
            } else {
                self.chat_history.push(entry);
            }
        }
    }
}
