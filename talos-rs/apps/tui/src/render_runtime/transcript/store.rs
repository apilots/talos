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
}

/// Owns finalized transcript blocks and stable line identities.
#[derive(Debug, Default)]
pub(crate) struct TranscriptStore {
    blocks: Vec<TranscriptBlock>,
    next_block_id: u64,
    next_line_id: u64,
}

impl TranscriptStore {
    #[cfg(test)]
    pub(crate) fn push_block(
        &mut self,
        kind: TranscriptBlockKind,
        lines: Vec<StyledLine>,
    ) -> TranscriptBlockId {
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
        self.blocks
            .push(TranscriptBlock::new(id, kind, transcript_lines));
        id
    }

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
        let insert_index = self.insert_index_for(block.kind);
        self.blocks.insert(
            insert_index,
            TranscriptBlock::new(id, block.kind, transcript_lines)
                .with_copy_markdown(block.copy_markdown),
        );
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

    pub(crate) fn pending_terminal_blocks(&self) -> impl Iterator<Item = &TranscriptBlock> {
        self.blocks.iter().filter(|block| !block.terminal_synced)
    }

    pub(crate) fn pending_terminal_projection(&self) -> Vec<PendingTerminalBlock> {
        self.pending_terminal_blocks()
            .map(|block| PendingTerminalBlock {
                block_id: block.id,
                kind: block.kind,
                lines: terminal_projection(block),
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
        }
    }

    pub(crate) fn mark_many_terminal_synced(
        &mut self,
        block_ids: impl IntoIterator<Item = TranscriptBlockId>,
    ) {
        for block_id in block_ids {
            self.mark_terminal_synced(block_id);
        }
    }

    fn insert_index_for(&self, kind: TranscriptBlockKind) -> usize {
        if kind != TranscriptBlockKind::Thinking {
            return self.blocks.len();
        }

        let round_start = self
            .blocks
            .iter()
            .rposition(|block| {
                matches!(
                    block.kind,
                    TranscriptBlockKind::UserInput | TranscriptBlockKind::CommandInput
                )
            })
            .map_or(0, |index| index + 1);

        self.blocks[round_start..]
            .iter()
            .position(|block| {
                matches!(
                    block.kind,
                    TranscriptBlockKind::Commentary | TranscriptBlockKind::FinalAnswer
                )
            })
            .map_or(self.blocks.len(), |offset| round_start + offset)
    }
}

#[cfg(test)]
mod tests {
    use super::TranscriptStore;
    use crate::render_runtime::common::style::StyledLine;
    use crate::render_runtime::transcript::block::TranscriptBlockKind;

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
}
