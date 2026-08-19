use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use std::time::Instant;

use crate::message_queue::{queued_message_is_steerable, queued_message_summary};
use crate::utils::{char_display_width, compact_directory_path, text_display_width};
use crate::{TuiApp, CHAT_CONTENT_PADDING_LEFT};

impl TuiApp {
    pub(crate) fn input_lines(&self, width: usize, height: usize) -> Vec<Line<'static>> {
        let draft_lines = self.draft_input_lines(width);
        let content_len = draft_lines.len();

        if height == 0 {
            return draft_lines;
        }

        if content_len > height {
            return draft_lines
                .into_iter()
                .skip(content_len.saturating_sub(height))
                .collect();
        }

        if content_len == 1 && height > 1 {
            let top_padding = (height - 1) / 2;
            let mut result: Vec<Line<'static>> = (0..top_padding).map(|_| Line::from("")).collect();
            result.extend(draft_lines);
            return result;
        }

        draft_lines
    }

    pub(crate) fn draft_input_lines(&self, width: usize) -> Vec<Line<'static>> {
        let chars: Vec<char> = self.input_buffer.chars().collect();
        let mut display_text = String::new();

        for (idx, ch) in chars.iter().enumerate() {
            if idx == self.cursor_index {
                display_text.push('█');
            }
            display_text.push(*ch);
        }

        if self.cursor_index >= chars.len() {
            display_text.push('█');
        }

        let left_padding = " ".repeat(CHAT_CONTENT_PADDING_LEFT as usize);
        let first_prefix = format!("{}❯ ", left_padding);
        let continuation_prefix = format!("{}  ", left_padding);
        let first_prefix_width = text_display_width(&first_prefix);
        let continuation_prefix_width = text_display_width(&continuation_prefix);
        let mut rendered_lines = Vec::new();

        for (logical_idx, logical_line) in display_text.split('\n').enumerate() {
            let line_prefix = if logical_idx == 0 {
                first_prefix.as_str()
            } else {
                continuation_prefix.as_str()
            };
            let mut current_line = line_prefix.to_string();
            let mut current_width = text_display_width(line_prefix);

            if logical_line.is_empty() {
                rendered_lines.push(current_line);
                continue;
            }

            for ch in logical_line.chars() {
                let ch_width = char_display_width(ch).max(1);
                let wrap_prefix_width = if current_line.starts_with(&first_prefix) {
                    first_prefix_width
                } else {
                    continuation_prefix_width
                };
                if width > 0
                    && current_width + ch_width > width
                    && current_width > wrap_prefix_width
                {
                    rendered_lines.push(current_line);
                    current_line = continuation_prefix.clone();
                    current_width = continuation_prefix_width;
                }
                current_line.push(ch);
                current_width += ch_width;
            }

            rendered_lines.push(current_line);
        }

        rendered_lines
            .into_iter()
            .map(|line| {
                if let Some(rest) = line.strip_prefix(first_prefix.as_str()) {
                    Line::from(vec![
                        Span::styled(
                            first_prefix.clone(),
                            Style::default()
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::raw(rest.to_string()),
                    ])
                } else {
                    Line::from(line)
                }
            })
            .collect()
    }

    pub(crate) fn input_status_line(&self, width: usize) -> Line<'static> {
        let directory = compact_directory_path(&self.startup_directory);
        let token_left = self
            .token_left_percent()
            .map(|percent| format!("{}% left", percent))
            .unwrap_or_else(|| "n/a left".to_string());

        let history_indicator = if let Some((index, total)) = self.input_history.selected_position()
        {
            format!("  [History {index}/{total}]")
        } else {
            String::new()
        };
        let pasted_indicator = match (
            self.pasted_blocks.len(),
            self.pasted_image_attachments.len(),
        ) {
            (0, 0) => String::new(),
            (text_count, 0) => format!("  [{text_count} pasted]"),
            (0, image_count) => format!("  [{image_count} image(s)]"),
            (text_count, image_count) => {
                format!("  [{text_count} pasted · {image_count} image(s)]")
            }
        };
        let base = if self.close_in_progress {
            let session_id = self
                .closing_session_id
                .clone()
                .unwrap_or_else(|| "unknown".to_string());
            format!(
                "closing {} · {}{}{}",
                session_id, directory, history_indicator, pasted_indicator
            )
        } else if let Some(stage) = self.active_context_compaction_stage() {
            format!(
                "Context compacting... {} · {}{}{}",
                crate::stream_events::context_compaction_stage_label(stage),
                directory,
                history_indicator,
                pasted_indicator
            )
        } else if self.context_compaction_in_flight() {
            format!(
                "Context compacting... preparing · {}{}{}",
                directory, history_indicator, pasted_indicator
            )
        } else {
            format!(
                "{} · {} · {}{}{}",
                self.current_model_status_label(),
                token_left,
                directory,
                history_indicator,
                pasted_indicator
            )
        };
        let mut text = if let Some((hint, expires_at)) = &self.status_hint {
            if Instant::now() < *expires_at {
                format!("{}  |  {}", base, hint)
            } else {
                base
            }
        } else {
            base
        };
        text = format!("{}{}", " ".repeat(CHAT_CONTENT_PADDING_LEFT as usize), text);

        let max = width.saturating_sub(1);
        if text.chars().count() > max {
            text = text.chars().take(max).collect();
        }
        Line::from(text)
    }

    pub(crate) fn turn_metrics_status_line(&self, width: usize) -> Option<Line<'static>> {
        let metrics_width = width.saturating_sub(CHAT_CONTENT_PADDING_LEFT as usize);
        let mut text = self.compact_turn_metrics_text(metrics_width)?;
        text = format!("{}{}", " ".repeat(CHAT_CONTENT_PADDING_LEFT as usize), text);
        let max = width.saturating_sub(1);
        if text.chars().count() > max {
            text = text.chars().take(max).collect();
        }
        Some(Line::from(text))
    }

    pub(crate) fn token_left_percent(&self) -> Option<u8> {
        let context_window = self
            .available_model_info
            .iter()
            .find(|model| {
                model.provider == self.current_model_provider
                    && model.model.id == self.current_model
            })
            .map(|model| model.model.context_window)
            .filter(|window| *window > 0)?;
        if context_window == 0 {
            return None;
        }
        let used = self.displayed_usage().total_tokens.min(context_window);
        let used_percent = used
            .saturating_mul(100)
            .saturating_add(context_window.saturating_sub(1))
            / context_window;
        Some(100_u8.saturating_sub(used_percent as u8))
    }

    pub(crate) fn queue_panel_lines(&self, max_lines: usize) -> Vec<Line<'static>> {
        if self.message_queue.messages.is_empty() {
            return Vec::new();
        }

        let action_hint = self.selected_queue_message.and_then(|index| {
            let message = self.message_queue.messages.get(index)?;
            if self.in_flight && queued_message_is_steerable(message) {
                Some("Enter Steer  e Edit  d Delete  Ctrl+Up/Down Move  Esc Close")
            } else if self.in_flight {
                Some("e Edit  d Delete  Ctrl+Up/Down Move  Esc Close")
            } else {
                Some("Enter Start  e Edit  d Delete  Ctrl+Up/Down Move  Esc Close")
            }
        });
        let mut preview: Vec<String> = vec![format!(
            "Waiting messages {}/{}",
            self.message_queue.messages.len(),
            self.message_queue.max_messages
        )];

        let visible_messages = max_lines.saturating_sub(1 + usize::from(action_hint.is_some()));
        let start = self
            .selected_queue_message
            .unwrap_or_default()
            .saturating_sub(visible_messages.saturating_sub(1));
        for (idx, message) in self
            .message_queue
            .messages
            .iter()
            .enumerate()
            .skip(start)
            .take(visible_messages)
        {
            let selected = self.selected_queue_message == Some(idx);
            let marker = if selected { "❯" } else { " " };
            preview.push(format!(
                "{marker} {}. {}",
                idx + 1,
                queued_message_summary(message)
            ));
        }

        if let Some(action_hint) = action_hint {
            preview.push(action_hint.to_string());
        }

        if preview.len() > max_lines && max_lines > 0 {
            preview.truncate(max_lines);
            if let Some(last) = preview.last_mut() {
                *last = "└─ ...".to_string();
            }
        }

        preview.into_iter().map(Line::from).collect()
    }
}
