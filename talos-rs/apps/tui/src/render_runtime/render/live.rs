//! Live stream reducer for render runtime.

use std::collections::HashMap;

use crate::render_runtime::common::style::StyledLine;
use crate::render_runtime::render::tool_projection;
use crate::render_runtime::transcript::block::{FinalizedTranscriptBlock, TranscriptBlockKind};
use talos_sdk::TalosContentItem;

/// Live text role that can still receive deltas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LiveTextKind {
    Commentary,
    Thinking,
    FinalAnswer,
}

impl LiveTextKind {
    fn transcript_kind(self) -> TranscriptBlockKind {
        match self {
            LiveTextKind::Commentary => TranscriptBlockKind::Commentary,
            LiveTextKind::Thinking => TranscriptBlockKind::Thinking,
            LiveTextKind::FinalAnswer => TranscriptBlockKind::FinalAnswer,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LiveTextBlock {
    kind: LiveTextKind,
    text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LiveToolBlock {
    call_id: String,
    name: String,
    arguments: String,
    progress: Vec<String>,
}

/// Mutable live projection block consumed by the TUI viewport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LiveProjectionBlock {
    pub(crate) kind: TranscriptBlockKind,
    pub(crate) lines: Vec<StyledLine>,
}

/// Reduces mutable live stream updates into finalized transcript blocks.
#[derive(Debug, Default)]
pub(crate) struct LiveReducer {
    text_blocks: HashMap<String, LiveTextBlock>,
    tool_blocks: HashMap<String, LiveToolBlock>,
    item_order: Vec<String>,
}

impl LiveReducer {
    pub(crate) fn start_text(
        &mut self,
        item_id: impl Into<String>,
        kind: LiveTextKind,
        text: impl AsRef<str>,
    ) {
        let item_id = item_id.into();
        self.track_item_order(&item_id);
        let block = self.text_blocks.entry(item_id).or_insert(LiveTextBlock {
            kind,
            text: String::new(),
        });
        block.kind = kind;
        block.text.push_str(text.as_ref());
    }

    pub(crate) fn append_text(&mut self, item_id: &str, text: &str) {
        if text.is_empty() {
            return;
        }
        if let Some(block) = self.text_blocks.get_mut(item_id) {
            block.text.push_str(text);
        }
    }

    pub(crate) fn set_text(
        &mut self,
        item_id: impl Into<String>,
        kind: LiveTextKind,
        text: impl Into<String>,
    ) {
        let item_id = item_id.into();
        self.track_item_order(&item_id);
        self.text_blocks.insert(
            item_id,
            LiveTextBlock {
                kind,
                text: text.into(),
            },
        );
    }

    pub(crate) fn promote_text(&mut self, item_id: &str, kind: LiveTextKind) {
        if let Some(block) = self.text_blocks.get_mut(item_id) {
            block.kind = kind;
        }
    }

    pub(crate) fn finalize_text(&mut self, item_id: &str) -> Option<FinalizedTranscriptBlock> {
        let block = self.text_blocks.remove(item_id)?;
        self.remove_item_order(item_id);
        let text = block.text.trim();
        if text.is_empty() {
            return None;
        }
        Some(FinalizedTranscriptBlock::new(
            block.kind.transcript_kind(),
            StyledLine::plain_lines(text),
        ))
    }

    pub(crate) fn start_tool(
        &mut self,
        item_id: impl Into<String>,
        call_id: impl Into<String>,
        name: impl Into<String>,
    ) {
        self.start_tool_with_arguments(item_id, call_id, name, "")
    }

    pub(crate) fn start_tool_with_arguments(
        &mut self,
        item_id: impl Into<String>,
        call_id: impl Into<String>,
        name: impl Into<String>,
        arguments: impl Into<String>,
    ) {
        let item_id = item_id.into();
        let call_id = call_id.into();
        let name = name.into();
        let arguments = arguments.into();
        self.track_item_order(&item_id);
        self.tool_blocks.insert(
            item_id,
            LiveToolBlock {
                call_id: call_id.clone(),
                name: name.clone(),
                arguments: arguments.clone(),
                progress: Vec::new(),
            },
        );
    }

    pub(crate) fn append_stable_tool_progress(&mut self, item_id: &str, text: impl Into<String>) {
        let text = text.into();
        if text.trim().is_empty() {
            return;
        }
        if let Some(block) = self.tool_blocks.get_mut(item_id) {
            block.progress.push(text.clone());
        }
    }

    pub(crate) fn append_tool_arguments(&mut self, item_id: &str, arguments_delta: &str) {
        if let Some(block) = self.tool_blocks.get_mut(item_id) {
            block.arguments.push_str(arguments_delta);
        }
    }

    pub(crate) fn complete_tool(
        &mut self,
        item_id: &str,
        is_error: bool,
        result_content: Vec<TalosContentItem>,
    ) -> Option<FinalizedTranscriptBlock> {
        let block = self.tool_blocks.remove(item_id)?;
        self.remove_item_order(item_id);
        Some(tool_projection::tool_completed_block(
            &block.name,
            &block.call_id,
            &block.arguments,
            is_error,
            &result_content,
        ))
    }

    pub(crate) fn interactive_projection_lines(&self) -> Vec<StyledLine> {
        let mut lines = Vec::new();
        for block in self.interactive_projection_blocks() {
            if !lines.is_empty() {
                lines.push(StyledLine::plain(""));
            }
            lines.extend(block.lines);
        }
        lines
    }

    pub(crate) fn interactive_projection_blocks(&self) -> Vec<LiveProjectionBlock> {
        let mut lines = Vec::new();
        for item_id in &self.item_order {
            if let Some(block) = self.text_blocks.get(item_id) {
                let text = block.text.trim();
                if !text.is_empty() {
                    lines.push(LiveProjectionBlock {
                        kind: block.kind.transcript_kind(),
                        lines: StyledLine::plain_lines(text),
                    });
                }
                continue;
            }

            if let Some(block) = self.tool_blocks.get(item_id) {
                let mut tool_lines = vec![tool_projection::tool_running_line(
                    &block.name,
                    &block.call_id,
                    &block.arguments,
                )];
                for progress in &block.progress {
                    let progress = progress.trim();
                    if !progress.is_empty() {
                        tool_lines.push(StyledLine::plain(format!("  ⎿ {progress}")));
                    }
                }
                lines.push(LiveProjectionBlock {
                    kind: TranscriptBlockKind::Tool,
                    lines: tool_lines,
                });
            }
        }
        lines
    }

    pub(crate) fn active_projection_count(&self) -> usize {
        self.text_blocks
            .values()
            .filter(|block| !block.text.trim().is_empty())
            .count()
            + self.tool_blocks.len()
    }

    fn track_item_order(&mut self, item_id: &str) {
        if !self.item_order.iter().any(|existing| existing == item_id) {
            self.item_order.push(item_id.to_string());
        }
    }

    fn remove_item_order(&mut self, item_id: &str) {
        self.item_order.retain(|existing| existing != item_id);
    }
}

#[cfg(test)]
mod tests {
    use super::{LiveReducer, LiveTextKind};
    use crate::render_runtime::transcript::block::TranscriptBlockKind;
    use talos_sdk::TalosContentItem;

    #[test]
    fn finalizes_text_only_when_item_completed() {
        let mut reducer = LiveReducer::default();
        reducer.start_text("item-1", LiveTextKind::Commentary, "hello");
        reducer.append_text("item-1", " world");

        let finalized = reducer.finalize_text("item-1").unwrap();

        assert_eq!(finalized.kind, TranscriptBlockKind::Commentary);
        assert_eq!(finalized.lines[0].plain_text(), "hello world");
        assert!(reducer.finalize_text("item-1").is_none());
    }

    #[test]
    fn can_promote_unknown_text_to_final_answer_before_finalizing() {
        let mut reducer = LiveReducer::default();
        reducer.start_text("item-1", LiveTextKind::Commentary, "answer");
        reducer.promote_text("item-1", LiveTextKind::FinalAnswer);

        let finalized = reducer.finalize_text("item-1").unwrap();

        assert_eq!(finalized.kind, TranscriptBlockKind::FinalAnswer);
    }

    #[test]
    fn tool_header_progress_and_completion_are_append_only_blocks() {
        let mut reducer = LiveReducer::default();

        reducer.start_tool_with_arguments(
            "tool-1",
            "call-1",
            "Read",
            r#"{"file_path":"/tmp/demo.rs"}"#,
        );
        reducer.append_stable_tool_progress("tool-1", "Read 12 lines");
        let completed = reducer
            .complete_tool(
                "tool-1",
                false,
                vec![TalosContentItem::Text {
                    text: "result detail".to_string(),
                }],
            )
            .unwrap();

        assert_eq!(completed.kind, TranscriptBlockKind::Tool);
        assert_eq!(
            completed.lines[0].plain_text(),
            "● Read(/tmp/demo.rs) completed"
        );
        assert_eq!(
            completed.lines[1].plain_text(),
            "  ⎿ result detail (ctrl+o to expand)"
        );
        assert!(completed
            .copy_markdown
            .as_deref()
            .unwrap_or_default()
            .contains("Output:\n```text\nresult detail\n```"));
    }

    #[test]
    fn interactive_projection_keeps_live_text_order() {
        let mut reducer = LiveReducer::default();
        reducer.start_text("item-1", LiveTextKind::Thinking, "plan");
        reducer.start_text("item-2", LiveTextKind::FinalAnswer, "answer");

        let lines = reducer.interactive_projection_lines();

        assert_eq!(
            lines
                .iter()
                .map(crate::render_runtime::common::style::StyledLine::plain_text)
                .collect::<Vec<_>>(),
            vec!["plan".to_string(), String::new(), "answer".to_string()]
        );
        assert_eq!(reducer.active_projection_count(), 2);
    }

    #[test]
    fn interactive_projection_keeps_mixed_text_and_tool_order() {
        let mut reducer = LiveReducer::default();
        reducer.start_text("item-1", LiveTextKind::Thinking, "plan");
        reducer.start_tool_with_arguments(
            "tool-1",
            "call-1",
            "Read",
            r#"{"file_path":"/tmp/demo.rs"}"#,
        );
        reducer.start_text("item-2", LiveTextKind::FinalAnswer, "answer");

        let lines = reducer.interactive_projection_lines();

        assert_eq!(
            lines
                .iter()
                .map(crate::render_runtime::common::style::StyledLine::plain_text)
                .collect::<Vec<_>>(),
            vec![
                "plan".to_string(),
                String::new(),
                "● Read(/tmp/demo.rs) running".to_string(),
                String::new(),
                "answer".to_string(),
            ]
        );
    }

    #[test]
    fn interactive_projection_includes_active_tool_snapshot() {
        let mut reducer = LiveReducer::default();
        reducer.start_tool_with_arguments(
            "tool-1",
            "call-1",
            "Read",
            r#"{"file_path":"/tmp/demo.rs"}"#,
        );
        reducer.append_stable_tool_progress("tool-1", "Read 12 lines");

        let lines = reducer.interactive_projection_lines();

        assert_eq!(
            lines
                .iter()
                .map(crate::render_runtime::common::style::StyledLine::plain_text)
                .collect::<Vec<_>>(),
            vec![
                "● Read(/tmp/demo.rs) running".to_string(),
                "  ⎿ Read 12 lines".to_string(),
            ]
        );
        assert_eq!(reducer.active_projection_count(), 1);
    }

    #[test]
    fn tool_argument_deltas_update_live_tool_projection() {
        let mut reducer = LiveReducer::default();
        reducer.start_tool_with_arguments("tool-1", "call-1", "Glob", "");

        reducer.append_tool_arguments("tool-1", r#"{"pattern":"**/*.rs"}"#);

        let lines = reducer.interactive_projection_lines();
        assert_eq!(lines[0].plain_text(), "● Glob(**/*.rs) running");
    }
}
