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

/// Returns visible interactive lines for the provided viewport.
#[cfg(test)]
pub(crate) fn visible_range_projection(
    lines: &[StyledLine],
    scroll_offset: usize,
    viewport_height: usize,
    overscan: usize,
) -> &[StyledLine] {
    let start = scroll_offset.saturating_sub(overscan).min(lines.len());
    let end = scroll_offset
        .saturating_add(viewport_height)
        .saturating_add(overscan)
        .min(lines.len());
    &lines[start..end]
}

#[cfg(test)]
mod tests {
    use super::visible_range_projection;
    use crate::render_runtime::common::style::StyledLine;

    #[test]
    fn visible_range_is_capped_by_viewport_and_overscan() {
        let lines = (0..20)
            .map(|index| StyledLine::plain(index.to_string()))
            .collect::<Vec<_>>();

        let visible = visible_range_projection(&lines, 10, 4, 2);

        assert_eq!(visible.len(), 8);
        assert_eq!(visible[0].plain_text(), "8");
        assert_eq!(visible[7].plain_text(), "15");
    }
}
