use std::collections::HashMap;

use crate::types::ChatBlock;

#[derive(Debug, Default)]
pub(crate) struct TranscriptState {
    pub(crate) live_thinking_idx: Option<usize>,
    pub(crate) live_assistant_idx: Option<usize>,
    pub(crate) active_tool_calls: HashMap<String, usize>,
    pub(crate) has_visible_activity: bool,
}

impl TranscriptState {
    pub(crate) fn reset_for_new_stream(&mut self) {
        self.live_thinking_idx = None;
        self.live_assistant_idx = None;
        self.active_tool_calls.clear();
        self.has_visible_activity = false;
    }

    pub(crate) fn note_visible_activity(&mut self) {
        self.has_visible_activity = true;
    }

    pub(crate) fn on_insert(&mut self, inserted_idx: usize) {
        self.live_thinking_idx =
            self.live_thinking_idx
                .map(|idx| if idx >= inserted_idx { idx + 1 } else { idx });
        self.live_assistant_idx =
            self.live_assistant_idx
                .map(|idx| if idx >= inserted_idx { idx + 1 } else { idx });

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
    }

    pub(crate) fn on_remove(&mut self, removed_idx: usize) {
        self.live_thinking_idx = match self.live_thinking_idx {
            Some(idx) if idx == removed_idx => None,
            Some(idx) if idx > removed_idx => Some(idx - 1),
            other => other,
        };
        self.live_assistant_idx = match self.live_assistant_idx {
            Some(idx) if idx == removed_idx => None,
            Some(idx) if idx > removed_idx => Some(idx - 1),
            other => other,
        };

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
    }

    pub(crate) fn resolve_tool_call_idx(
        &mut self,
        chat_blocks: &[ChatBlock],
        call_id: &str,
    ) -> Option<usize> {
        if let Some(&idx) = self.active_tool_calls.get(call_id) {
            if chat_blocks
                .get(idx)
                .and_then(|block| block.tool_call_id.as_deref())
                == Some(call_id)
            {
                return Some(idx);
            }
        }

        let idx = chat_blocks
            .iter()
            .position(|block| block.tool_call_id.as_deref() == Some(call_id))?;
        self.active_tool_calls.insert(call_id.to_string(), idx);
        Some(idx)
    }

    pub(crate) fn finalize(&mut self, chat_blocks: &mut Vec<ChatBlock>) {
        let mut remove_indices = Vec::new();

        if let Some(idx) = self.live_assistant_idx {
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
        self.live_assistant_idx = None;
        self.active_tool_calls.clear();
        self.has_visible_activity = false;
    }
}
