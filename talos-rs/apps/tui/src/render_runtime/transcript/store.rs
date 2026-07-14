//! Finalized transcript store for render runtime projections.

use crate::render_runtime::common::style::StyledLine;
use crate::render_runtime::transcript::block::{
    FinalizedTranscriptBlock, TranscriptBlock, TranscriptBlockId, TranscriptBlockKind,
};
use crate::render_runtime::transcript::line::{TranscriptLine, TranscriptLineId};
use crate::render_runtime::transcript::projection::terminal_projection;

/// Terminal projection for a block that has not been flushed yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingTerminalBlock {
    pub(crate) block_id: TranscriptBlockId,
    pub(crate) kind: TranscriptBlockKind,
    pub(crate) lines: Vec<StyledLine>,
    pub(crate) round_duration_ms: Option<u64>,
}

/// Owns finalized transcript blocks and stable line identities.
#[derive(Debug, Default)]
pub(crate) struct TranscriptStore {
    blocks: Vec<TranscriptBlock>,
    next_block_id: u64,
    next_line_id: u64,
    terminal_history_present: bool,
}

impl TranscriptStore {
    pub(crate) fn push_finalized(&mut self, block: FinalizedTranscriptBlock) -> TranscriptBlockId {
        let id = TranscriptBlockId(self.next_block_id);
        self.next_block_id += 1;
        let transcript_lines = block
            .lines
            .into_iter()
            .map(|line| {
                let line_id = TranscriptLineId(self.next_line_id);
                self.next_line_id += 1;
                TranscriptLine::new(line_id, line)
            })
            .collect();
        let mut transcript_block = TranscriptBlock::new(id, block.kind, transcript_lines)
            .with_copy_markdown(block.copy_markdown);
        if let Some(round_duration_ms) = block.round_duration_ms {
            transcript_block = transcript_block.with_round_duration_ms(round_duration_ms);
        }
        self.blocks.push(transcript_block);
        id
    }

    pub(crate) fn upsert_session_header(&mut self, lines: Vec<StyledLine>) -> TranscriptBlockId {
        if self
            .blocks
            .first()
            .is_some_and(|block| block.kind == TranscriptBlockKind::SessionHeader)
        {
            let block = self
                .blocks
                .first_mut()
                .expect("first block should exist after header check");
            if block.terminal_synced {
                return block.id;
            }
            block.lines = lines
                .into_iter()
                .map(|line| {
                    let line_id = TranscriptLineId(self.next_line_id);
                    self.next_line_id += 1;
                    TranscriptLine::new(line_id, line)
                })
                .collect();
            block.terminal_synced = false;
            return block.id;
        }

        let id = TranscriptBlockId(self.next_block_id);
        self.next_block_id += 1;
        let transcript_lines = lines
            .into_iter()
            .map(|line| {
                let line_id = TranscriptLineId(self.next_line_id);
                self.next_line_id += 1;
                TranscriptLine::new(line_id, line)
            })
            .collect();
        self.blocks.insert(
            0,
            TranscriptBlock::new(id, TranscriptBlockKind::SessionHeader, transcript_lines),
        );
        id
    }

    pub(crate) fn blocks(&self) -> &[TranscriptBlock] {
        &self.blocks
    }

    pub(crate) fn clear(&mut self) {
        self.blocks.clear();
        self.next_block_id = 0;
        self.next_line_id = 0;
    }

    pub(crate) fn pending_terminal_projection(&self) -> Vec<PendingTerminalBlock> {
        self.blocks
            .iter()
            .filter(|block| !block.terminal_synced)
            .map(|block| PendingTerminalBlock {
                block_id: block.id,
                kind: block.kind,
                lines: terminal_projection(block),
                round_duration_ms: block.round_duration_ms,
            })
            .collect()
    }

    pub(crate) fn interactive_projection_lines(&self) -> Vec<StyledLine> {
        let mut lines = Vec::new();
        for (index, block) in self.blocks.iter().enumerate() {
            if index > 0 {
                lines.push(StyledLine::plain(""));
            }
            lines.extend(terminal_projection(block));
        }
        lines
    }

    pub(crate) fn mark_terminal_synced(&mut self, block_id: TranscriptBlockId) {
        if let Some(block) = self.blocks.iter_mut().find(|block| block.id == block_id) {
            block.terminal_synced = true;
            self.terminal_history_present = true;
        }
    }

    pub(crate) fn has_terminal_history(&self) -> bool {
        self.terminal_history_present
    }

    pub(crate) fn mark_many_terminal_synced(
        &mut self,
        block_ids: impl IntoIterator<Item = TranscriptBlockId>,
    ) {
        for block_id in block_ids {
            self.mark_terminal_synced(block_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TranscriptStore;
    use crate::render_runtime::common::style::StyledLine;
    use crate::render_runtime::transcript::block::{
        FinalizedTranscriptBlock, TranscriptBlockId, TranscriptBlockKind,
    };

    trait TestTranscriptStoreExt {
        fn push_block(
            &mut self,
            kind: TranscriptBlockKind,
            lines: Vec<StyledLine>,
        ) -> TranscriptBlockId;
    }

    impl TestTranscriptStoreExt for TranscriptStore {
        fn push_block(
            &mut self,
            kind: TranscriptBlockKind,
            lines: Vec<StyledLine>,
        ) -> TranscriptBlockId {
            self.push_finalized(FinalizedTranscriptBlock::new(kind, lines))
        }
    }

    #[test]
    fn assigns_stable_block_and_line_ids() {
        let mut store = TranscriptStore::default();

        let first = store.push_block(
            TranscriptBlockKind::UserInput,
            vec![StyledLine::plain("hello")],
        );
        let second = store.push_block(
            TranscriptBlockKind::FinalAnswer,
            vec![StyledLine::plain("world")],
        );

        assert_ne!(first, second);
        assert_eq!(store.blocks()[0].lines[0].id.0, 0);
        assert_eq!(store.blocks()[1].lines[0].id.0, 1);
    }

    #[test]
    fn pending_terminal_projection_tracks_unsynced_blocks() {
        let mut store = TranscriptStore::default();
        let first = store.push_block(
            TranscriptBlockKind::UserInput,
            vec![StyledLine::plain("hello")],
        );
        let second = store.push_block(
            TranscriptBlockKind::FinalAnswer,
            vec![StyledLine::plain("world")],
        );
        store.mark_terminal_synced(first);

        let pending = store.pending_terminal_projection();

        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].block_id, second);
        assert_eq!(pending[0].lines[0].plain_text(), "world");
    }

    #[test]
    fn interactive_projection_flattens_blocks_with_spacing() {
        let mut store = TranscriptStore::default();
        store.push_block(
            TranscriptBlockKind::UserInput,
            vec![StyledLine::plain("hello")],
        );
        store.push_block(
            TranscriptBlockKind::FinalAnswer,
            vec![StyledLine::plain("world")],
        );

        let lines = store.interactive_projection_lines();

        assert_eq!(
            lines.iter().map(StyledLine::plain_text).collect::<Vec<_>>(),
            vec!["hello".to_string(), String::new(), "world".to_string()]
        );
    }

    #[test]
    fn preserves_finalized_block_insertion_order() {
        let mut store = TranscriptStore::default();
        store.push_block(
            TranscriptBlockKind::UserInput,
            vec![StyledLine::plain("review")],
        );
        store.push_block(
            TranscriptBlockKind::Commentary,
            vec![StyledLine::plain("checking files")],
        );
        store.push_block(
            TranscriptBlockKind::Tool,
            vec![StyledLine::plain("bash completed")],
        );
        store.push_block(
            TranscriptBlockKind::Thinking,
            vec![StyledLine::plain("inspect result")],
        );

        assert_eq!(
            store
                .blocks()
                .iter()
                .map(|block| block.kind)
                .collect::<Vec<_>>(),
            vec![
                TranscriptBlockKind::UserInput,
                TranscriptBlockKind::Commentary,
                TranscriptBlockKind::Tool,
                TranscriptBlockKind::Thinking,
            ]
        );
    }

    #[test]
    fn clear_resets_blocks_and_stable_ids() {
        let mut store = TranscriptStore::default();
        store.push_block(
            TranscriptBlockKind::UserInput,
            vec![StyledLine::plain("hello")],
        );

        store.clear();
        let id = store.push_block(
            TranscriptBlockKind::FinalAnswer,
            vec![StyledLine::plain("world")],
        );

        assert_eq!(id.0, 0);
        assert_eq!(store.blocks()[0].lines[0].id.0, 0);
    }

    #[test]
    fn clear_preserves_the_existing_terminal_history_boundary() {
        let mut store = TranscriptStore::default();
        let id = store.push_block(
            TranscriptBlockKind::SystemNotice,
            vec![StyledLine::plain("previous session")],
        );
        store.mark_terminal_synced(id);

        store.clear();

        assert!(store.has_terminal_history());
    }
}
