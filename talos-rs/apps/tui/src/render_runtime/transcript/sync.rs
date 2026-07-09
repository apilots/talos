//! Synchronizes finalized transcript blocks to terminal scrollback.

use std::io::{self, Write};

use crate::render_runtime::transcript::store::TranscriptStore;
use crate::render_runtime::transcript::writer::{InteractionLayer, TerminalTranscriptWriter};

/// Flushes all pending terminal projections and marks flushed blocks as synced.
pub(crate) fn flush_pending_terminal_blocks<W: Write>(
    store: &mut TranscriptStore,
    writer: &mut TerminalTranscriptWriter<W>,
    interaction_layer: &mut impl InteractionLayer,
) -> io::Result<bool> {
    let pending = store.pending_terminal_projection();
    if pending.is_empty() {
        return Ok(false);
    }

    writer.enqueue_blocks(pending);
    let Some(flushed_block_ids) = writer.flush_with_interaction_layer(interaction_layer)? else {
        return Ok(false);
    };
    store.mark_many_terminal_synced(flushed_block_ids);
    Ok(true)
}

#[cfg(test)]
mod tests {
    use std::io::{self, Write};

    use super::flush_pending_terminal_blocks;
    use crate::render_runtime::common::style::StyledLine;
    use crate::render_runtime::transcript::block::TranscriptBlockKind;
    use crate::render_runtime::transcript::store::TranscriptStore;
    use crate::render_runtime::transcript::writer::{InteractionLayer, TerminalTranscriptWriter};

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
    fn flushes_pending_blocks_and_marks_them_synced() {
        let mut store = TranscriptStore::default();
        store.push_block(
            TranscriptBlockKind::UserInput,
            vec![StyledLine::plain("hello")],
        );
        let mut writer = TerminalTranscriptWriter::new(Vec::new());

        let flushed =
            flush_pending_terminal_blocks(&mut store, &mut writer, &mut TestInteractionLayer)
                .unwrap();

        assert!(flushed);
        assert!(store.pending_terminal_projection().is_empty());
        assert_eq!(
            String::from_utf8(writer.into_inner()).unwrap(),
            "[suspend]hello\u{1b}[0m\n[repaint][restore]"
        );
    }

    #[test]
    fn does_not_repaint_when_no_blocks_are_pending() {
        let mut store = TranscriptStore::default();
        let mut writer = TerminalTranscriptWriter::new(Vec::new());

        let flushed =
            flush_pending_terminal_blocks(&mut store, &mut writer, &mut TestInteractionLayer)
                .unwrap();

        assert!(!flushed);
        assert!(writer.into_inner().is_empty());
    }
}
