//! Terminal transcript writer contract for primary scrollback output.

use std::io::{self, Write};

use crate::render_runtime::common::style::StyledLine;
use crate::render_runtime::render::ansi::write_ansi_line;
use crate::render_runtime::transcript::block::TranscriptBlockId;
use crate::render_runtime::transcript::store::PendingTerminalBlock;

/// Interaction layer hooks used to keep transcript flushes serialized with UI repaint.
pub(crate) trait InteractionLayer {
    fn suspend(&mut self, output: &mut dyn Write) -> io::Result<()>;
    fn repaint(&mut self, output: &mut dyn Write) -> io::Result<()>;
    fn restore(&mut self, output: &mut dyn Write) -> io::Result<()>;
}

/// Writes finalized transcript projections to primary terminal scrollback.
pub(crate) struct TerminalTranscriptWriter<W> {
    output: W,
    pending_lines: Vec<StyledLine>,
    pending_block_ids: Vec<TranscriptBlockId>,
}

impl<W: Write> TerminalTranscriptWriter<W> {
    pub(crate) fn new(output: W) -> Self {
        Self {
            output,
            pending_lines: Vec::new(),
            pending_block_ids: Vec::new(),
        }
    }

    pub(crate) fn enqueue_lines(&mut self, lines: impl IntoIterator<Item = StyledLine>) {
        self.pending_lines.extend(lines);
    }

    pub(crate) fn enqueue_blocks(
        &mut self,
        blocks: impl IntoIterator<Item = PendingTerminalBlock>,
    ) {
        for block in blocks {
            self.pending_block_ids.push(block.block_id);
            self.enqueue_lines(block.lines);
        }
    }

    pub(crate) fn flush_with_interaction_layer(
        &mut self,
        interaction_layer: &mut impl InteractionLayer,
    ) -> io::Result<Option<Vec<TranscriptBlockId>>> {
        if self.pending_lines.is_empty() {
            return Ok(None);
        }

        interaction_layer.suspend(&mut self.output)?;
        for line in self.pending_lines.drain(..) {
            write_ansi_line(&mut self.output, &line)?;
        }
        interaction_layer.repaint(&mut self.output)?;
        interaction_layer.restore(&mut self.output)?;
        self.output.flush()?;
        Ok(Some(std::mem::take(&mut self.pending_block_ids)))
    }

    #[cfg(test)]
    pub(crate) fn into_inner(self) -> W {
        self.output
    }
}

#[cfg(test)]
mod tests {
    use std::io::{self, Write};

    use super::{InteractionLayer, TerminalTranscriptWriter};
    use crate::render_runtime::common::style::StyledLine;

    struct TestInteractionLayer;

    impl InteractionLayer for TestInteractionLayer {
        fn suspend(&mut self, output: &mut dyn Write) -> io::Result<()> {
            output.write_all(b"[suspend]")
        }

        fn repaint(&mut self, output: &mut dyn Write) -> io::Result<()> {
            output.write_all(b"[repaint]")
        }

        fn restore(&mut self, output: &mut dyn Write) -> io::Result<()> {
            output.write_all(b"[restore]")
        }
    }

    #[test]
    fn flush_order_serializes_transcript_before_repaint_and_restore() {
        let mut writer = TerminalTranscriptWriter::new(Vec::new());
        writer.enqueue_lines([StyledLine::plain("hello")]);

        let flushed_block_ids = writer
            .flush_with_interaction_layer(&mut TestInteractionLayer)
            .unwrap();

        assert_eq!(flushed_block_ids, Some(Vec::new()));
        assert_eq!(
            String::from_utf8(writer.into_inner()).unwrap(),
            "[suspend]hello\u{1b}[0m\n[repaint][restore]"
        );
    }

    #[test]
    fn no_pending_lines_do_not_repaint_or_flush() {
        let mut writer = TerminalTranscriptWriter::new(Vec::new());

        let flushed_block_ids = writer
            .flush_with_interaction_layer(&mut TestInteractionLayer)
            .unwrap();

        assert_eq!(flushed_block_ids, None);
        assert!(writer.into_inner().is_empty());
    }

    #[test]
    fn writer_does_not_emit_alternate_screen_sequences() {
        let mut writer = TerminalTranscriptWriter::new(Vec::new());
        writer.enqueue_lines([StyledLine::plain("persisted")]);
        writer
            .flush_with_interaction_layer(&mut TestInteractionLayer)
            .unwrap();

        let output = String::from_utf8(writer.into_inner()).unwrap();

        assert!(!output.contains("\u{1b}[?1049h"));
        assert!(!output.contains("\u{1b}[?1049l"));
    }

    #[test]
    fn flushed_blocks_return_synced_block_ids() {
        use crate::render_runtime::common::style::StyledLine;
        use crate::render_runtime::transcript::block::{TranscriptBlockId, TranscriptBlockKind};
        use crate::render_runtime::transcript::store::PendingTerminalBlock;

        let block_id = TranscriptBlockId(7);
        let mut writer = TerminalTranscriptWriter::new(Vec::new());
        writer.enqueue_blocks([PendingTerminalBlock {
            block_id,
            kind: TranscriptBlockKind::FinalAnswer,
            lines: vec![StyledLine::plain("done")],
        }]);

        let flushed_block_ids = writer
            .flush_with_interaction_layer(&mut TestInteractionLayer)
            .unwrap();

        assert_eq!(flushed_block_ids, Some(vec![block_id]));
    }
}
