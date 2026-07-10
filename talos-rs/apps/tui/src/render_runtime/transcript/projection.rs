//! Transcript projection helpers.

use crate::render_runtime::common::style::StyledLine;
use crate::render_runtime::transcript::block::TranscriptBlock;

/// Returns terminal projection lines for a finalized block.
pub(crate) fn terminal_projection(block: &TranscriptBlock) -> Vec<StyledLine> {
    block
        .lines
        .iter()
        .map(|line| line.content.clone())
        .collect()
}
