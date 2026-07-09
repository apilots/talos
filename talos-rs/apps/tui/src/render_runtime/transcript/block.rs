//! Finalized transcript block model.

use crate::render_runtime::transcript::line::TranscriptLine;

/// Stable identifier for a transcript block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct TranscriptBlockId(pub(crate) u64);

/// High-level kind of finalized transcript block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TranscriptBlockKind {
    SessionHeader,
    CommandInput,
    UserInput,
    Commentary,
    Thinking,
    FinalAnswer,
    Tool,
    SystemNotice,
}

/// Finalized transcript block used by both interactive and terminal projections.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TranscriptBlock {
    pub(crate) id: TranscriptBlockId,
    pub(crate) kind: TranscriptBlockKind,
    pub(crate) lines: Vec<TranscriptLine>,
    pub(crate) copy_markdown: Option<String>,
    pub(crate) terminal_synced: bool,
}

/// Finalized transcript content ready to enter the store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FinalizedTranscriptBlock {
    pub(crate) kind: TranscriptBlockKind,
    pub(crate) lines: Vec<crate::render_runtime::common::style::StyledLine>,
    pub(crate) copy_markdown: Option<String>,
}

impl FinalizedTranscriptBlock {
    pub(crate) fn new(
        kind: TranscriptBlockKind,
        lines: Vec<crate::render_runtime::common::style::StyledLine>,
    ) -> Self {
        Self {
            kind,
            lines,
            copy_markdown: None,
        }
    }

    pub(crate) fn with_copy_markdown(mut self, copy_markdown: impl Into<String>) -> Self {
        let copy_markdown = copy_markdown.into();
        if !copy_markdown.trim().is_empty() {
            self.copy_markdown = Some(copy_markdown);
        }
        self
    }
}

impl TranscriptBlock {
    pub(crate) fn new(
        id: TranscriptBlockId,
        kind: TranscriptBlockKind,
        lines: Vec<TranscriptLine>,
    ) -> Self {
        Self {
            id,
            kind,
            lines,
            copy_markdown: None,
            terminal_synced: false,
        }
    }

    pub(crate) fn with_copy_markdown(mut self, copy_markdown: Option<String>) -> Self {
        self.copy_markdown = copy_markdown;
        self
    }
}
