use std::collections::{HashMap, HashSet};

use crate::types::ChatBlock;

#[derive(Debug, Default)]
pub(crate) struct TranscriptState {
    pub(crate) live_thinking_idx: Option<usize>,
    pub(crate) live_commentary_idx: Option<usize>,
    pub(crate) live_final_answer_idx: Option<usize>,
    pub(crate) live_text_blocks: HashMap<String, usize>,
    pub(crate) pending_final_answer_text: HashMap<String, String>,
    pub(crate) open_final_answer_items: HashSet<String>,
    pub(crate) active_tool_items: HashMap<String, usize>,
    pub(crate) active_tool_calls: HashMap<String, usize>,
    pub(crate) active_bash_sessions: HashMap<i64, usize>,
}

impl TranscriptState {
    pub(crate) fn reset_for_new_stream(&mut self) {
        self.live_thinking_idx = None;
        self.live_commentary_idx = None;
        self.live_final_answer_idx = None;
        self.live_text_blocks.clear();
        self.pending_final_answer_text.clear();
        self.open_final_answer_items.clear();
        self.active_tool_items.clear();
        self.active_tool_calls.clear();
        self.active_bash_sessions.clear();
    }

    pub(crate) fn on_insert(&mut self, inserted_idx: usize) {
        self.live_thinking_idx =
            self.live_thinking_idx
                .map(|idx| if idx >= inserted_idx { idx + 1 } else { idx });
        self.live_commentary_idx =
            self.live_commentary_idx
                .map(|idx| if idx >= inserted_idx { idx + 1 } else { idx });
        self.live_final_answer_idx =
            self.live_final_answer_idx
                .map(|idx| if idx >= inserted_idx { idx + 1 } else { idx });

        self.live_text_blocks = self
            .live_text_blocks
            .drain()
            .map(|(item_id, block_idx)| {
                if block_idx >= inserted_idx {
                    (item_id, block_idx + 1)
                } else {
                    (item_id, block_idx)
                }
            })
            .collect();

        self.active_tool_calls = self
            .active_tool_calls
            .drain()
            .map(|(call_id, block_idx)| {
                if block_idx >= inserted_idx {
                    (call_id, block_idx + 1)
                } else {
                    (call_id, block_idx)
                }
            })
            .collect();

        self.active_tool_items = self
            .active_tool_items
            .drain()
            .map(|(item_id, block_idx)| {
                if block_idx >= inserted_idx {
                    (item_id, block_idx + 1)
                } else {
                    (item_id, block_idx)
                }
            })
            .collect();

        self.active_bash_sessions = self
            .active_bash_sessions
            .drain()
            .map(|(session_id, block_idx)| {
                if block_idx >= inserted_idx {
                    (session_id, block_idx + 1)
                } else {
                    (session_id, block_idx)
                }
            })
            .collect();
    }

    pub(crate) fn on_remove(&mut self, removed_idx: usize) {
        self.live_thinking_idx = match self.live_thinking_idx {
            Some(idx) if idx == removed_idx => None,
            Some(idx) if idx > removed_idx => Some(idx - 1),
            other => other,
        };
        self.live_commentary_idx = match self.live_commentary_idx {
            Some(idx) if idx == removed_idx => None,
            Some(idx) if idx > removed_idx => Some(idx - 1),
            other => other,
        };
        self.live_final_answer_idx = match self.live_final_answer_idx {
            Some(idx) if idx == removed_idx => None,
            Some(idx) if idx > removed_idx => Some(idx - 1),
            other => other,
        };

        self.live_text_blocks = self
            .live_text_blocks
            .drain()
            .filter_map(|(item_id, block_idx)| {
                if block_idx == removed_idx {
                    None
                } else if block_idx > removed_idx {
                    Some((item_id, block_idx - 1))
                } else {
                    Some((item_id, block_idx))
                }
            })
            .collect();

        self.active_tool_calls = self
            .active_tool_calls
            .drain()
            .filter_map(|(call_id, block_idx)| {
                if block_idx == removed_idx {
                    None
                } else if block_idx > removed_idx {
                    Some((call_id, block_idx - 1))
                } else {
                    Some((call_id, block_idx))
                }
            })
            .collect();

        self.active_tool_items = self
            .active_tool_items
            .drain()
            .filter_map(|(item_id, block_idx)| {
                if block_idx == removed_idx {
                    None
                } else if block_idx > removed_idx {
                    Some((item_id, block_idx - 1))
                } else {
                    Some((item_id, block_idx))
                }
            })
            .collect();

        self.active_bash_sessions = self
            .active_bash_sessions
            .drain()
            .filter_map(|(session_id, block_idx)| {
                if block_idx == removed_idx {
                    None
                } else if block_idx > removed_idx {
                    Some((session_id, block_idx - 1))
                } else {
                    Some((session_id, block_idx))
                }
            })
            .collect();
    }

    pub(crate) fn resolve_tool_call_idx(
        &mut self,
        chat_blocks: &[ChatBlock],
        item_id: Option<&str>,
        call_id: &str,
    ) -> Option<usize> {
        if let Some(item_id) = item_id {
            if let Some(&idx) = self.active_tool_items.get(item_id) {
                if chat_blocks
                    .get(idx)
                    .map(|block| block.role == crate::types::ChatRole::Tool)
                    .unwrap_or(false)
                {
                    self.active_tool_calls.insert(call_id.to_string(), idx);
                    return Some(idx);
                }
            }

            if let Some(idx) = chat_blocks
                .iter()
                .position(|block| block.tool_item_id.as_deref() == Some(item_id))
            {
                self.active_tool_items.insert(item_id.to_string(), idx);
                self.active_tool_calls.insert(call_id.to_string(), idx);
                return Some(idx);
            }
        }

        if let Some(&idx) = self.active_tool_calls.get(call_id) {
            if chat_blocks
                .get(idx)
                .map(|block| block.role == crate::types::ChatRole::Tool)
                .unwrap_or(false)
            {
                if let Some(item_id) = item_id {
                    self.active_tool_items.insert(item_id.to_string(), idx);
                }
                return Some(idx);
            }
        }

        let idx = chat_blocks
            .iter()
            .position(|block| block.tool_call_id.as_deref() == Some(call_id))?;
        self.active_tool_calls.insert(call_id.to_string(), idx);
        if let Some(item_id) = item_id {
            self.active_tool_items.insert(item_id.to_string(), idx);
        }
        Some(idx)
    }

    pub(crate) fn finalize(&mut self, chat_blocks: &mut Vec<ChatBlock>) {
        let mut remove_indices = Vec::new();

        if let Some(idx) = self.live_final_answer_idx {
            if chat_blocks
                .get(idx)
                .map(|block| block.content.trim().is_empty())
                .unwrap_or(false)
            {
                remove_indices.push(idx);
            }
        }

        if let Some(idx) = self.live_commentary_idx {
            if chat_blocks
                .get(idx)
                .map(|block| block.content.trim().is_empty())
                .unwrap_or(false)
            {
                remove_indices.push(idx);
            }
        }

        if let Some(idx) = self.live_thinking_idx {
            if chat_blocks
                .get(idx)
                .map(|block| block.content.trim().is_empty())
                .unwrap_or(false)
            {
                remove_indices.push(idx);
            }
        }

        remove_indices.sort_unstable();
        remove_indices.dedup();
        for idx in remove_indices.into_iter().rev() {
            if idx < chat_blocks.len() {
                chat_blocks.remove(idx);
                self.on_remove(idx);
            }
        }

        self.live_thinking_idx = None;
        self.live_commentary_idx = None;
        self.live_final_answer_idx = None;
        self.live_text_blocks.clear();
        self.pending_final_answer_text.clear();
        self.open_final_answer_items.clear();
        self.active_tool_items.clear();
        self.active_tool_calls.clear();
        self.active_bash_sessions.clear();
    }
}
