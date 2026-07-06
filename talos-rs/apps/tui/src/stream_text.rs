use crate::types::ChatRole;
use crate::{TuiApp, MAX_RIGHT_EVENTS};

impl TuiApp {
    pub(crate) fn finish_live_text_blocks(&mut self) {
        let final_answer_idx = self.transcript.live_final_answer_idx.take();
        let commentary_idx = self.transcript.live_commentary_idx.take();
        let thinking_idx = self.transcript.live_thinking_idx.take();
        self.remove_empty_live_text_block(final_answer_idx);
        self.remove_empty_live_text_block(commentary_idx);
        self.remove_empty_live_text_block(thinking_idx);
        self.transcript.live_text_blocks.clear();
    }

    fn remove_empty_live_text_block(&mut self, idx: Option<usize>) {
        let Some(idx) = idx else {
            return;
        };
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

    pub(crate) fn append_commentary(&mut self, item_id: Option<&str>, text: &str) {
        self.append_live_text(item_id, ChatRole::Commentary, text);
    }

    pub(crate) fn append_final_answer(&mut self, item_id: Option<&str>, text: &str) {
        self.append_live_text(item_id, ChatRole::FinalAnswer, text);
    }

    pub(crate) fn buffer_final_answer(&mut self, item_id: &str, text: &str) {
        if text.is_empty() {
            return;
        }
        self.transcript
            .pending_final_answer_text
            .entry(item_id.to_string())
            .or_default()
            .push_str(text);
    }

    pub(crate) fn final_answer_is_open(&self, item_id: &str) -> bool {
        self.transcript.open_final_answer_items.contains(item_id)
    }

    pub(crate) fn open_final_answer(&mut self, item_id: &str) {
        self.transcript
            .open_final_answer_items
            .insert(item_id.to_string());
        if let Some(text) = self.transcript.pending_final_answer_text.remove(item_id) {
            self.append_final_answer(Some(item_id), &text);
            self.received_output_delta = true;
        }
    }

    pub(crate) fn append_thinking(&mut self, item_id: Option<&str>, text: &str) {
        self.append_live_text(item_id, ChatRole::Thinking, text);
    }

    pub(crate) fn append_live_text(&mut self, item_id: Option<&str>, role: ChatRole, text: &str) {
        if text.is_empty() {
            return;
        }
        let idx = self.live_text_idx(item_id, role);

        if let Some(block) = self.chat_blocks.get_mut(idx) {
            block.content.push_str(text);
        }
        self.sync_history_entry(idx);
        self.request_follow_bottom();
    }

    fn live_text_idx(&mut self, item_id: Option<&str>, role: ChatRole) -> usize {
        if let Some(item_id) = item_id {
            if let Some(&idx) = self.transcript.live_text_blocks.get(item_id) {
                if self.chat_blocks.get(idx).is_some() {
                    self.set_live_role_idx(role, idx);
                    return idx;
                }
            }
        }

        if item_id.is_none() {
            if let Some(idx) = self.live_role_idx(role) {
                if self.chat_blocks.get(idx).is_some() {
                    return idx;
                }
            }
        }

        let idx = self.push_block(role, String::new());
        self.set_live_role_idx(role, idx);
        if let Some(item_id) = item_id {
            self.transcript
                .live_text_blocks
                .insert(item_id.to_string(), idx);
        }
        idx
    }

    fn live_role_idx(&self, role: ChatRole) -> Option<usize> {
        match role {
            ChatRole::Thinking => self.transcript.live_thinking_idx,
            ChatRole::Commentary => self.transcript.live_commentary_idx,
            ChatRole::FinalAnswer => self.transcript.live_final_answer_idx,
            ChatRole::User | ChatRole::System | ChatRole::Status | ChatRole::Tool => None,
        }
    }

    fn set_live_role_idx(&mut self, role: ChatRole, idx: usize) {
        match role {
            ChatRole::Thinking => self.transcript.live_thinking_idx = Some(idx),
            ChatRole::Commentary => self.transcript.live_commentary_idx = Some(idx),
            ChatRole::FinalAnswer => self.transcript.live_final_answer_idx = Some(idx),
            ChatRole::User | ChatRole::System | ChatRole::Status | ChatRole::Tool => {}
        }
    }

    pub(crate) fn move_live_text_block(&mut self, item_id: &str, role: ChatRole) -> Option<usize> {
        let idx = self.transcript.live_text_blocks.get(item_id).copied()?;
        let block = self.chat_blocks.get_mut(idx)?;
        block.role = role;
        self.set_live_role_idx(role, idx);
        match role {
            ChatRole::Thinking => {
                if self.transcript.live_commentary_idx == Some(idx) {
                    self.transcript.live_commentary_idx = None;
                }
                if self.transcript.live_final_answer_idx == Some(idx) {
                    self.transcript.live_final_answer_idx = None;
                }
            }
            ChatRole::Commentary => {
                if self.transcript.live_thinking_idx == Some(idx) {
                    self.transcript.live_thinking_idx = None;
                }
                if self.transcript.live_final_answer_idx == Some(idx) {
                    self.transcript.live_final_answer_idx = None;
                }
            }
            ChatRole::FinalAnswer => {
                if self.transcript.live_thinking_idx == Some(idx) {
                    self.transcript.live_thinking_idx = None;
                }
                if self.transcript.live_commentary_idx == Some(idx) {
                    self.transcript.live_commentary_idx = None;
                }
            }
            ChatRole::User | ChatRole::System | ChatRole::Status | ChatRole::Tool => {}
        }
        self.sync_history_entry(idx);
        Some(idx)
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
