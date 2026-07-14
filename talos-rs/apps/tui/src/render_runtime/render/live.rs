//! Live stream reducer for render runtime.

use std::collections::HashMap;

use crate::render_runtime::common::style::StyledLine;
use crate::render_runtime::render::tool_projection;
use crate::render_runtime::transcript::block::{FinalizedTranscriptBlock, TranscriptBlockKind};
use talos_sdk::{TalosContentItem, TalosToolProgressKind};

const TOOL_PROGRESS_BUFFER_LIMIT: usize = 4096;

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
    completed: bool,
    phase_resolved: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LiveTextPhaseResolution {
    Pending,
    Resolved,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LiveToolBlock {
    call_id: String,
    name: String,
    arguments: String,
    progress: LiveToolProgress,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct LiveToolProgress {
    status: String,
    stdout: String,
    stderr: String,
    latest_kind: Option<TalosToolProgressKind>,
}

impl LiveToolProgress {
    fn append(&mut self, kind: TalosToolProgressKind, text_delta: &str) {
        if text_delta.is_empty() {
            return;
        }
        self.latest_kind = Some(kind);
        let buffer = match kind {
            TalosToolProgressKind::Status => {
                self.status.clear();
                &mut self.status
            }
            TalosToolProgressKind::Stdout => &mut self.stdout,
            TalosToolProgressKind::Stderr => &mut self.stderr,
        };
        buffer.push_str(text_delta);
        if buffer.len() > TOOL_PROGRESS_BUFFER_LIMIT {
            let mut start = buffer.len().saturating_sub(TOOL_PROGRESS_BUFFER_LIMIT);
            while !buffer.is_char_boundary(start) {
                start += 1;
            }
            buffer.drain(..start);
        }
    }

    fn latest_preview(&self) -> Option<String> {
        let buffer = match self.latest_kind? {
            TalosToolProgressKind::Status => &self.status,
            TalosToolProgressKind::Stdout => &self.stdout,
            TalosToolProgressKind::Stderr => &self.stderr,
        };
        buffer
            .lines()
            .rev()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .map(tool_projection::compact_tool_preview)
    }
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
        self.start_text_with_phase(item_id, kind, text, LiveTextPhaseResolution::Resolved);
    }

    pub(crate) fn start_unresolved_text(
        &mut self,
        item_id: impl Into<String>,
        kind: LiveTextKind,
        text: impl AsRef<str>,
    ) {
        self.start_text_with_phase(item_id, kind, text, LiveTextPhaseResolution::Pending);
    }

    fn start_text_with_phase(
        &mut self,
        item_id: impl Into<String>,
        kind: LiveTextKind,
        text: impl AsRef<str>,
        phase_resolution: LiveTextPhaseResolution,
    ) {
        let item_id = item_id.into();
        self.track_item_order(&item_id);
        let phase_resolved = phase_resolution == LiveTextPhaseResolution::Resolved;
        let block = self.text_blocks.entry(item_id).or_insert(LiveTextBlock {
            kind,
            text: String::new(),
            completed: false,
            phase_resolved,
        });
        if phase_resolved || !block.phase_resolved {
            block.kind = kind;
        }
        block.phase_resolved |= phase_resolved;
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

    pub(crate) fn replace_text(&mut self, item_id: &str, text: impl Into<String>) {
        if let Some(block) = self.text_blocks.get_mut(item_id) {
            block.text = text.into();
        }
    }

    pub(crate) fn commit_text(
        &mut self,
        item_id: &str,
        kind: LiveTextKind,
    ) -> Option<FinalizedTranscriptBlock> {
        if let Some(block) = self.text_blocks.get_mut(item_id) {
            block.kind = kind;
            block.phase_resolved = true;
        }
        self.take_text_if_finalized(item_id)
    }

    pub(crate) fn complete_text(&mut self, item_id: &str) -> Option<FinalizedTranscriptBlock> {
        if let Some(block) = self.text_blocks.get_mut(item_id) {
            block.completed = true;
        }
        self.take_text_if_finalized(item_id)
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

    fn take_text_if_finalized(&mut self, item_id: &str) -> Option<FinalizedTranscriptBlock> {
        let ready = self
            .text_blocks
            .get(item_id)
            .is_some_and(|block| block.completed && block.phase_resolved);
        ready.then(|| self.finalize_text(item_id)).flatten()
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
                progress: LiveToolProgress::default(),
            },
        );
    }

    pub(crate) fn append_tool_progress_delta(
        &mut self,
        item_id: &str,
        kind: TalosToolProgressKind,
        text_delta: &str,
    ) {
        if let Some(block) = self.tool_blocks.get_mut(item_id) {
            block.progress.append(kind, text_delta);
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
                if let Some(progress) = block.progress.latest_preview() {
                    tool_lines.push(StyledLine::plain(format!("  ⎿ {progress}")));
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
    use talos_sdk::{TalosContentItem, TalosToolProgressKind};

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
    fn finalizes_completed_unknown_text_after_phase_commit() {
        let mut reducer = LiveReducer::default();
        reducer.start_unresolved_text("item-1", LiveTextKind::Commentary, "answer");

        assert!(reducer.complete_text("item-1").is_none());
        let finalized = reducer
            .commit_text("item-1", LiveTextKind::FinalAnswer)
            .unwrap();

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
        reducer.append_tool_progress_delta(
            "tool-1",
            TalosToolProgressKind::Status,
            "Read 12 lines",
        );
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
    fn interactive_projection_keeps_latest_active_tool_snapshot() {
        let mut reducer = LiveReducer::default();
        reducer.start_tool_with_arguments(
            "tool-1",
            "call-1",
            "Read",
            r#"{"file_path":"/tmp/demo.rs"}"#,
        );
        reducer.append_tool_progress_delta("tool-1", TalosToolProgressKind::Status, "Read 4 lines");
        reducer.append_tool_progress_delta(
            "tool-1",
            TalosToolProgressKind::Status,
            "Read 12 lines",
        );

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
