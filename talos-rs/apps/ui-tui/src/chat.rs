use talos_client::{TalosContentItem, TalosOutputItem};

use crate::types::{ChatBlock, ChatRole, ToolCallStatus};
use crate::TuiApp;

impl TuiApp {

    pub(crate) fn push_block(&mut self, role: ChatRole, content: String) -> usize {
        self.chat_blocks.push(ChatBlock::new(role, content));
        let idx = self.chat_blocks.len() - 1;
        self.sync_history_entry(idx);
        self.request_follow_bottom();
        idx
    }


    pub(crate) fn remove_chat_block(&mut self, idx: usize) {
        if idx >= self.chat_blocks.len() {
            return;
        }

        self.chat_blocks.remove(idx);

        self.active_tool_calls = self
            .active_tool_calls
            .drain()
            .filter_map(|(call_id, block_idx)| {
                if block_idx == idx {
                    None
                } else if block_idx > idx {
                    Some((call_id, block_idx - 1))
                } else {
                    Some((call_id, block_idx))
                }
            })
            .collect();
    }


    pub(crate) fn rebuild_chat_history(&mut self) {
        self.chat_history = self
            .chat_blocks
            .iter()
            .map(|block| format!("{}\n{}", block.role.as_str(), block.content))
            .collect();
    }


    pub(crate) fn shift_tool_indices_on_insert(&mut self, inserted_idx: usize) {
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


    pub(crate) fn tool_result_output_text(content: &[TalosContentItem]) -> String {
        content
            .iter()
            .map(|item| match item {
                TalosContentItem::Text { text } => text.clone(),
                TalosContentItem::Json { value } => value.to_string(),
            })
            .collect::<Vec<_>>()
            .join("\n")
    }


    pub(crate) fn replay_history_item(&mut self, item: &TalosOutputItem, turn_index: u64, item_index: u64) {
        match item {
            TalosOutputItem::UserInput { text } => {
                self.push_block(ChatRole::User, text.clone());
                self.add_message_to_history(text.clone());
            }
            TalosOutputItem::OutputText { text } => {
                self.push_block(ChatRole::Assistant, text.clone());
            }
            TalosOutputItem::Reasoning { text } => {
                self.push_block(ChatRole::Thinking, text.clone());
            }
            TalosOutputItem::ToolCall {
                call_id,
                name,
                arguments,
            } => {
                self.push_tool_call_block(
                    call_id.clone(),
                    name.clone(),
                    arguments.clone(),
                    turn_index,
                    item_index,
                    None,
                );
            }
            TalosOutputItem::ToolResult {
                call_id,
                tool_name: _,
                content,
                is_error,
            } => {
                self.complete_tool_call_block(
                    call_id,
                    Self::tool_result_output_text(content),
                    !is_error,
                    None,
                );
            }
            TalosOutputItem::ExecutionSummary { .. } => {}
        }
    }


    pub(crate) fn push_tool_call_block(
        &mut self,
        call_id: String,
        tool_name: String,
        tool_input: serde_json::Value,
        iteration: u64,
        item_index: u64,
        timing_ms: Option<u64>,
    ) -> usize {
        let block = ChatBlock::new_tool_call(
            call_id.clone(),
            tool_name,
            tool_input,
            iteration,
            item_index,
            timing_ms,
        );
        let idx = self
            .chat_blocks
            .iter()
            .position(|existing| {
                if existing.role != ChatRole::Tool {
                    return false;
                }
                let existing_iteration = existing.tool_iteration.unwrap_or(u64::MAX);
                let existing_item_index = existing.tool_item_index.unwrap_or(u64::MAX);
                (existing_iteration, existing_item_index) > (iteration, item_index)
            })
            .unwrap_or(self.chat_blocks.len());
        self.chat_blocks.insert(idx, block);
        self.shift_tool_indices_on_insert(idx);
        self.active_tool_calls.insert(call_id, idx);
        self.request_follow_bottom();
        idx
    }


    pub(crate) fn resolve_tool_call_block_idx(&mut self, call_id: &str) -> Option<usize> {
        if let Some(&idx) = self.active_tool_calls.get(call_id) {
            if self
                .chat_blocks
                .get(idx)
                .and_then(|block| block.tool_call_id.as_deref())
                == Some(call_id)
            {
                return Some(idx);
            }
        }

        let idx = self
            .chat_blocks
            .iter()
            .position(|block| block.tool_call_id.as_deref() == Some(call_id))?;
        self.active_tool_calls.insert(call_id.to_string(), idx);
        Some(idx)
    }


    pub(crate) fn complete_tool_call_block(
        &mut self,
        call_id: &str,
        output: String,
        exit_ok: bool,
        timing_ms: Option<u64>,
    ) {
        if let Some(idx) = self.resolve_tool_call_block_idx(call_id) {
            if let Some(block) = self.chat_blocks.get_mut(idx) {
                block.content = output;
                block.tool_status = Some(ToolCallStatus::Completed { exit_ok });
                if timing_ms.is_some() {
                    block.timing_ms = timing_ms;
                }
            }
            self.active_tool_calls.remove(call_id);
            self.request_follow_bottom();
        }
    }


    pub(crate) fn toggle_last_tool_block(&mut self) {
        let last_tool_idx = self
            .chat_blocks
            .iter()
            .rposition(|b| b.role == ChatRole::Tool);
        if let Some(idx) = last_tool_idx {
            if let Some(block) = self.chat_blocks.get_mut(idx) {
                block.expanded = !block.expanded;
                self.request_follow_bottom();
            }
        }
    }


    pub(crate) fn toggle_right_panel(&mut self) {
        self.right_panel_visible = !self.right_panel_visible;
    }


    pub(crate) fn discard_active_pending_blocks(&mut self) {
        let mut remove_indices = vec![];
        if let Some(idx) = self.active_assistant_idx {
            remove_indices.push(idx);
        }
        if let Some(idx) = self.active_thinking_idx {
            remove_indices.push(idx);
        }
        if let Some(idx) = self.active_user_idx {
            remove_indices.push(idx);
        }
        remove_indices.sort_unstable();
        remove_indices.dedup();

        for idx in remove_indices.into_iter().rev() {
            self.remove_chat_block(idx);
        }

        self.chat_history = self
            .chat_blocks
            .iter()
            .map(|block| format!("{}\n{}", block.role.as_str(), block.content))
            .collect();
        self.active_user_idx = None;
        self.active_thinking_idx = None;
        self.active_assistant_idx = None;
    }

}
