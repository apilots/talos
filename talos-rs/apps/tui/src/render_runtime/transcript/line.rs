//! Stable transcript line identity.

use crate::render_runtime::common::style::StyledLine;

/// Stable identifier for a rendered transcript line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct TranscriptLineId(pub(crate) u64);

/// A finalized transcript line with stable identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TranscriptLine {
    pub(crate) id: TranscriptLineId,
    pub(crate) content: StyledLine,
}

impl TranscriptLine {
    pub(crate) fn new(id: TranscriptLineId, content: StyledLine) -> Self {
        Self { id, content }
    }
}
