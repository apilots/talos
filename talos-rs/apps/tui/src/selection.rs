use std::time::Duration;

use crate::types::{ChatBlock, ChatRole, SelectionPoint};
use crate::utils::{
    copy_to_system_clipboard, display_tool_name, line_display_width, slice_text_by_display_columns,
    text_display_width,
};
use crate::TuiApp;

impl TuiApp {
    pub(crate) fn copy_chat_to_clipboard(&mut self) {
        let text = self
            .chat_blocks
            .iter()
            .map(|block| format!("{}\n{}", block.role.as_str(), block.content))
            .collect::<Vec<_>>()
            .join("\n\n");

        if text.trim().is_empty() {
            self.push_block(ChatRole::System, "Nothing to copy.".to_string());
            return;
        }

        match copy_to_system_clipboard(&text) {
            Ok(()) => self.push_block(ChatRole::System, "Copied chat to clipboard.".to_string()),
            Err(err) => {
                self.push_block(ChatRole::System, format!("Clipboard copy failed: {}", err))
            }
        };
    }

    pub(crate) fn format_block_for_round_copy(block: &ChatBlock) -> String {
        match block.role {
            ChatRole::Tool => {
                let tool_name = block.tool_name.as_deref().unwrap_or("unknown");
                let mut lines = vec![format!("### Tool `{}`", display_tool_name(tool_name))];
                if let Some(input) = &block.tool_input {
                    lines.push(String::new());
                    lines.push("Input:".to_string());
                    lines.push("```json".to_string());
                    lines.push(
                        serde_json::to_string_pretty(input).unwrap_or_else(|_| input.to_string()),
                    );
                    lines.push("```".to_string());
                }
                if !block.content.trim().is_empty() {
                    lines.push(String::new());
                    lines.push("Output:".to_string());
                    if serde_json::from_str::<serde_json::Value>(&block.content).is_ok() {
                        lines.push("```json".to_string());
                    } else {
                        lines.push("```text".to_string());
                    }
                    lines.push(block.content.clone());
                    lines.push("```".to_string());
                }
                lines.join("\n")
            }
            ChatRole::User => format!("## User\n\n{}", block.content),
            ChatRole::Assistant => format!("## Assistant\n\n{}", block.content),
            ChatRole::Thinking => format!("## Thinking\n\n{}", block.content),
            ChatRole::System => format!("## System\n\n{}", block.content),
        }
    }

    #[cfg(test)]
    pub(crate) fn round_bounds_for_block(&self, block_index: usize) -> Option<(usize, usize)> {
        if block_index >= self.chat_blocks.len() {
            return None;
        }

        let start = (0..=block_index)
            .rev()
            .find(|&index| self.chat_blocks[index].role == ChatRole::User)
            .unwrap_or(0);
        let end_exclusive = ((block_index + 1)..self.chat_blocks.len())
            .find(|&index| self.chat_blocks[index].role == ChatRole::User)
            .unwrap_or(self.chat_blocks.len());

        Some((start, end_exclusive))
    }

    #[cfg(test)]
    pub(crate) fn round_text_for_block(&self, block_index: usize) -> Option<String> {
        let (start, end_exclusive) = self.round_bounds_for_block(block_index)?;
        self.round_markdown(start, end_exclusive)
    }

    pub(crate) fn round_markdown(&self, start: usize, end_exclusive: usize) -> Option<String> {
        let text = self.chat_blocks[start..end_exclusive]
            .iter()
            .map(Self::format_block_for_round_copy)
            .collect::<Vec<_>>()
            .join("\n\n");
        (!text.trim().is_empty()).then_some(text)
    }

    pub(crate) fn copy_round_for_start_block(&mut self, start_block_index: usize) {
        let end_exclusive = ((start_block_index + 1)..self.chat_blocks.len())
            .find(|&index| self.chat_blocks[index].role == ChatRole::User)
            .unwrap_or(self.chat_blocks.len());
        let Some(text) = self.round_markdown(start_block_index, end_exclusive) else {
            self.show_status_hint("Nothing to copy.", Duration::from_secs(2));
            return;
        };

        match copy_to_system_clipboard(&text) {
            Ok(()) => self.show_status_hint("Copied round to clipboard.", Duration::from_secs(2)),
            Err(err) => self.show_status_hint(
                format!("Clipboard copy failed: {}", err),
                Duration::from_secs(3),
            ),
        }
    }

    pub(crate) fn copy_selection_to_clipboard(&mut self) {
        let Some(text) = self.selected_chat_text() else {
            self.show_status_hint("Nothing to copy.", Duration::from_secs(2));
            self.chat_selection = None;
            return;
        };

        match copy_to_system_clipboard(&text) {
            Ok(()) => {
                self.show_status_hint("Copied selection to clipboard.", Duration::from_secs(2))
            }
            Err(err) => {
                self.show_status_hint(
                    format!("Clipboard copy failed: {}", err),
                    Duration::from_secs(3),
                );
            }
        }
        self.chat_selection = None;
    }

    pub(crate) fn selected_chat_text(&self) -> Option<String> {
        let (start, end) = self.normalized_selection_points()?;
        let mut copied_lines = Vec::new();

        for line_index in start.line_index..=end.line_index {
            let Some(line) = self.rendered_chat_lines.get(line_index) else {
                continue;
            };
            let Some(projection) = &line.copy_projection else {
                continue;
            };
            let line_width = line_display_width(&line.display);
            let Some((selection_start, selection_end)) =
                self.selection_display_range_for_line(line_index, line_width)
            else {
                continue;
            };
            let projection_end =
                projection.display_start_col + text_display_width(&projection.text);
            let copy_start = selection_start.max(projection.display_start_col);
            let copy_end = selection_end.min(projection_end);
            if copy_start >= copy_end {
                copied_lines.push(String::new());
                continue;
            }
            copied_lines.push(slice_text_by_display_columns(
                &projection.text,
                copy_start.saturating_sub(projection.display_start_col),
                copy_end.saturating_sub(projection.display_start_col),
            ));
        }

        if copied_lines.iter().all(|line| line.is_empty()) {
            None
        } else {
            Some(copied_lines.join("\n"))
        }
    }

    pub(crate) fn normalized_selection_points(&self) -> Option<(SelectionPoint, SelectionPoint)> {
        let selection = self.chat_selection?;
        if (selection.anchor.line_index, selection.anchor.column)
            <= (selection.focus.line_index, selection.focus.column)
        {
            Some((selection.anchor, selection.focus))
        } else {
            Some((selection.focus, selection.anchor))
        }
    }

    pub(crate) fn selection_display_range_for_line(
        &self,
        line_index: usize,
        line_width: usize,
    ) -> Option<(usize, usize)> {
        let (start, end) = self.normalized_selection_points()?;
        if line_index < start.line_index || line_index > end.line_index {
            return None;
        }

        let width = line_width.max(1);
        let range = if start.line_index == end.line_index {
            (
                start.column.min(end.column),
                start.column.max(end.column).saturating_add(1),
            )
        } else if line_index == start.line_index {
            (start.column, width)
        } else if line_index == end.line_index {
            (0, end.column.saturating_add(1))
        } else {
            (0, width)
        };

        Some((range.0.min(width), range.1.min(width)))
    }
}
