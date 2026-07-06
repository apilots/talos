use talos_sdk::{TalosContentItem, ToolArguments};
use talos_types::talos::TalosToolTaskStatus;

use crate::types::{ChatBlock, ChatRole, ToolCallBlockUpdate, ToolCallStatus};
use crate::TuiApp;

#[derive(Clone, Copy)]
enum ToolContentMergeMode {
    Replace,
    Append,
}

struct CompletedToolBlockUpdate<'a> {
    idx: usize,
    item_id: Option<&'a str>,
    call_id: &'a str,
    output: String,
    content_items: Vec<TalosContentItem>,
    exit_ok: bool,
    timing_ms: Option<u64>,
    merge_mode: ToolContentMergeMode,
}

impl TuiApp {
    fn tool_input_has_detail(input: &ToolArguments) -> bool {
        match input.as_inner() {
            serde_json::Value::Null => false,
            serde_json::Value::Object(map) => !map.is_empty(),
            serde_json::Value::Array(items) => !items.is_empty(),
            serde_json::Value::String(value) => !value.trim().is_empty(),
            _ => true,
        }
    }

    fn should_replace_tool_input(
        existing: Option<&ToolArguments>,
        incoming: &ToolArguments,
    ) -> bool {
        Self::tool_input_has_detail(incoming) || existing.is_none()
    }

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
        self.transcript.on_remove(idx);
    }

    pub(crate) fn rebuild_chat_history(&mut self) {
        self.chat_history = self
            .chat_blocks
            .iter()
            .map(|block| format!("{}\n{}", block.role.as_str(), block.content))
            .collect();
    }

    pub(crate) fn shift_live_indices_on_insert(&mut self, inserted_idx: usize) {
        self.transcript.on_insert(inserted_idx);
    }

    pub(crate) fn tool_result_output_text(content: &[TalosContentItem]) -> String {
        content
            .iter()
            .map(TalosContentItem::display_text)
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub(crate) fn push_tool_call_block(&mut self, update: ToolCallBlockUpdate) -> usize {
        let ToolCallBlockUpdate {
            item_id,
            call_id,
            tool_name,
            tool_input,
            iteration,
            item_index,
            timing_ms,
        } = update;
        if let Some(idx) = self.resolve_tool_call_block_idx(item_id.as_deref(), &call_id) {
            if let Some(block) = self.chat_blocks.get_mut(idx) {
                if block.tool_item_id.is_none() {
                    block.tool_item_id.clone_from(&item_id);
                }
                block.tool_call_id = Some(call_id.clone());
                block.tool_name = Some(tool_name);
                if Self::should_replace_tool_input(block.tool_input.as_ref(), &tool_input) {
                    block.tool_input = Some(tool_input);
                }
                block.tool_iteration = Some(iteration);
                block.tool_item_index = Some(item_index);
                if timing_ms.is_some() {
                    block.timing_ms = timing_ms;
                }
            }
            self.request_follow_bottom();
            return idx;
        }

        let block = ChatBlock::new_tool_call_for_item(
            item_id.clone(),
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
        self.shift_live_indices_on_insert(idx);
        if let Some(item_id) = item_id {
            self.transcript.active_tool_items.insert(item_id, idx);
        }
        self.transcript.active_tool_calls.insert(call_id, idx);
        self.rebuild_chat_history();
        self.request_follow_bottom();
        idx
    }

    pub(crate) fn resolve_tool_call_block_idx(
        &mut self,
        item_id: Option<&str>,
        call_id: &str,
    ) -> Option<usize> {
        self.transcript
            .resolve_tool_call_idx(&self.chat_blocks, item_id, call_id)
    }

    pub(crate) fn update_tool_call_status(&mut self, call_id: &str, status: ToolCallStatus) {
        if let Some(idx) = self.resolve_tool_call_block_idx(None, call_id) {
            if let Some(block) = self.chat_blocks.get_mut(idx) {
                block.tool_status = Some(status);
            }
            self.request_follow_bottom();
        }
    }

    pub(crate) fn append_tool_call_progress(&mut self, call_id: &str, line: String) {
        if line.trim().is_empty() {
            return;
        }

        if let Some(idx) = self.resolve_tool_call_block_idx(None, call_id) {
            if let Some(block) = self.chat_blocks.get_mut(idx) {
                if block
                    .tool_progress
                    .last()
                    .map(|existing| existing == &line)
                    .unwrap_or(false)
                {
                    self.request_follow_bottom();
                    return;
                }
                block.tool_progress.push(line);
                const MAX_PROGRESS_LINES: usize = 8;
                if block.tool_progress.len() > MAX_PROGRESS_LINES {
                    let overflow = block.tool_progress.len() - MAX_PROGRESS_LINES;
                    block.tool_progress.drain(0..overflow);
                }
            }
            self.request_follow_bottom();
        }
    }

    pub(crate) fn complete_tool_call_block(
        &mut self,
        item_id: Option<&str>,
        call_id: &str,
        output: String,
        content_items: Vec<TalosContentItem>,
        exit_ok: bool,
        timing_ms: Option<u64>,
    ) {
        let bash_session_id = Self::bash_session_id(&content_items);
        let existing_idx = self.resolve_tool_call_block_idx(item_id, call_id);
        if let Some(session_id) = bash_session_id {
            if let Some(&target_idx) = self.transcript.active_bash_sessions.get(&session_id) {
                if existing_idx != Some(target_idx) {
                    self.update_completed_tool_block(CompletedToolBlockUpdate {
                        idx: target_idx,
                        item_id,
                        call_id,
                        output,
                        content_items,
                        exit_ok,
                        timing_ms,
                        merge_mode: ToolContentMergeMode::Append,
                    });
                    if let Some(idx) = existing_idx {
                        self.remove_chat_block(idx);
                    }
                    self.transcript.active_tool_calls.remove(call_id);
                    if let Some(item_id) = item_id {
                        self.transcript.active_tool_items.remove(item_id);
                    }
                    self.request_follow_bottom();
                    return;
                }
            }
        }

        if let Some(idx) = existing_idx {
            self.update_completed_tool_block(CompletedToolBlockUpdate {
                idx,
                item_id,
                call_id,
                output,
                content_items,
                exit_ok,
                timing_ms,
                merge_mode: ToolContentMergeMode::Replace,
            });
            self.transcript.active_tool_calls.remove(call_id);
            if let Some(item_id) = item_id {
                self.transcript.active_tool_items.remove(item_id);
            }
            self.request_follow_bottom();
        }
    }

    fn update_completed_tool_block(&mut self, update: CompletedToolBlockUpdate<'_>) {
        let CompletedToolBlockUpdate {
            idx,
            item_id,
            call_id,
            output,
            content_items,
            exit_ok,
            timing_ms,
            merge_mode,
        } = update;
        if let Some(block) = self.chat_blocks.get_mut(idx) {
            if block.tool_item_id.is_none() {
                block.tool_item_id = item_id.map(str::to_string);
            }
            match merge_mode {
                ToolContentMergeMode::Append => block.tool_content_items.extend(content_items),
                ToolContentMergeMode::Replace => block.tool_content_items = content_items,
            }
            block.content = if matches!(merge_mode, ToolContentMergeMode::Append) {
                Self::tool_result_output_text(&block.tool_content_items)
            } else {
                output
            };
            block.tool_status = Some(Self::tool_status_from_content(
                &block.tool_content_items,
                exit_ok,
            ));
            if timing_ms.is_some() {
                block.timing_ms = timing_ms;
            }
        }
        if let Some(session_id) = Self::bash_session_id(
            self.chat_blocks
                .get(idx)
                .map(|block| block.tool_content_items.as_slice())
                .unwrap_or(&[]),
        ) {
            self.transcript.active_bash_sessions.insert(session_id, idx);
        }
        self.transcript.active_tool_calls.remove(call_id);
        if let Some(item_id) = item_id {
            self.transcript.active_tool_items.remove(item_id);
        }
    }

    fn bash_session_id(content_items: &[TalosContentItem]) -> Option<i64> {
        content_items.iter().find_map(|item| {
            if let TalosContentItem::BashOutput { session_id, .. } = item {
                *session_id
            } else {
                None
            }
        })
    }

    fn tool_status_from_content(
        content_items: &[TalosContentItem],
        exit_ok: bool,
    ) -> ToolCallStatus {
        let Some(status) = content_items.iter().rev().find_map(|item| {
            if let TalosContentItem::BashOutput { status, .. } = item {
                Some(*status)
            } else {
                None
            }
        }) else {
            return ToolCallStatus::Completed { exit_ok };
        };
        match status {
            TalosToolTaskStatus::Queued => ToolCallStatus::Queued,
            TalosToolTaskStatus::Running => ToolCallStatus::Running,
            TalosToolTaskStatus::Completed => ToolCallStatus::Completed { exit_ok },
            TalosToolTaskStatus::Failed
            | TalosToolTaskStatus::TimedOut
            | TalosToolTaskStatus::Cancelled => ToolCallStatus::Completed { exit_ok: false },
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

    pub(crate) fn discard_active_pending_blocks(&mut self) {
        let mut remove_indices = vec![];
        if let Some(idx) = self.transcript.live_final_answer_idx {
            remove_indices.push(idx);
        }
        if let Some(idx) = self.transcript.live_thinking_idx {
            remove_indices.push(idx);
        }
        if let Some(idx) = self.transcript.live_commentary_idx {
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
        self.transcript.live_thinking_idx = None;
        self.transcript.live_commentary_idx = None;
        self.transcript.live_final_answer_idx = None;
        self.transcript.live_text_blocks.clear();
        self.transcript.pending_final_answer_text.clear();
        self.transcript.open_final_answer_items.clear();
        self.transcript.active_tool_items.clear();
        self.transcript.active_tool_calls.clear();
        self.transcript.active_bash_sessions.clear();
    }
}
