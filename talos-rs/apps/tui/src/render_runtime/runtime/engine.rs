//! Composes the render runtime, live reducer, transcript store, and terminal writer.

use std::collections::HashMap;
#[cfg(test)]
use std::io::{self, Write};
use std::marker::PhantomData;

use crate::render_runtime::common::style::StyledLine;
use crate::render_runtime::render::history::append_history_blocks;
#[cfg(test)]
use crate::render_runtime::render::live::LiveTextKind;
use crate::render_runtime::render::live::{LiveProjectionBlock, LiveReducer};
use crate::render_runtime::render::timeline::{
    apply_assistant_text_committed, apply_item_completed, apply_item_delta, apply_item_started,
};
use crate::render_runtime::runtime::controller::RuntimeController;
#[cfg(test)]
use crate::render_runtime::runtime::controller::RuntimeCycle;
use crate::render_runtime::runtime::event_loop::TuiRuntimeEvent;
use crate::render_runtime::transcript::block::TranscriptBlockId;
use crate::render_runtime::transcript::block::{FinalizedTranscriptBlock, TranscriptBlockKind};
use crate::render_runtime::transcript::store::PendingTerminalBlock;
use crate::render_runtime::transcript::store::TranscriptStore;
#[cfg(test)]
use crate::render_runtime::transcript::sync::flush_pending_terminal_blocks;
#[cfg(test)]
use crate::render_runtime::transcript::writer::{InteractionLayer, TerminalTranscriptWriter};
use talos_sdk::{
    AgentEventPayload, TalosTimelineContent, TalosTimelineDelta, TalosTimelineItem,
    TalosTimelineItemKind, TalosTimelinePhase,
};

/// Runtime engine for the render runtime.
///
/// This owns the transcript data flow for interactive and terminal projections.
pub(crate) struct RenderRuntimeEngine<W, I> {
    runtime: RuntimeController,
    live: LiveReducer,
    store: TranscriptStore,
    #[cfg(test)]
    writer: TerminalTranscriptWriter<W>,
    #[cfg(test)]
    interaction_layer: I,
    timeline_items: HashMap<String, TalosTimelineItem>,
    _marker: PhantomData<(W, I)>,
}

/// In-memory interaction layer used by tests and buffered runtime checks.
#[derive(Debug, Default)]
pub(crate) struct BufferedInteractionLayer;

#[cfg(test)]
impl InteractionLayer for BufferedInteractionLayer {
    fn suspend(&mut self, _output: &mut dyn Write) -> io::Result<()> {
        Ok(())
    }

    fn repaint(&mut self, _output: &mut dyn Write) -> io::Result<()> {
        Ok(())
    }

    fn restore(&mut self, _output: &mut dyn Write) -> io::Result<()> {
        Ok(())
    }
}

pub(crate) type BufferedRenderRuntimeEngine =
    RenderRuntimeEngine<Vec<u8>, BufferedInteractionLayer>;

impl<W, I> RenderRuntimeEngine<W, I> {
    #[cfg(test)]
    pub(crate) fn new(output: W, interaction_layer: I) -> Self
    where
        W: Write,
        I: InteractionLayer,
    {
        Self {
            runtime: RuntimeController::default(),
            live: LiveReducer::default(),
            store: TranscriptStore::default(),
            writer: TerminalTranscriptWriter::new(output),
            interaction_layer,
            timeline_items: HashMap::new(),
            _marker: PhantomData,
        }
    }

    pub(crate) fn append_user_input(&mut self, text: impl Into<String>) {
        let text = text.into();
        if text.trim().is_empty() {
            return;
        }
        self.push_finalized(FinalizedTranscriptBlock::new(
            TranscriptBlockKind::UserInput,
            vec![StyledLine::plain(text)],
        ));
    }

    pub(crate) fn append_command_input(&mut self, text: impl Into<String>) {
        let text = text.into();
        if text.trim().is_empty() {
            return;
        }
        self.push_finalized(FinalizedTranscriptBlock::new(
            TranscriptBlockKind::CommandInput,
            vec![StyledLine::plain(text)],
        ));
    }

    pub(crate) fn set_session_header(&mut self, lines: Vec<StyledLine>) {
        if lines.is_empty() {
            return;
        }
        self.store.upsert_session_header(lines);
        self.runtime.handle_event(TuiRuntimeEvent::HistoryLoaded);
    }

    pub(crate) fn append_system_notice(&mut self, text: impl Into<String>) {
        let text = text.into();
        if text.trim().is_empty() {
            return;
        }
        let lines = text.lines().map(StyledLine::plain).collect::<Vec<_>>();
        self.push_finalized(FinalizedTranscriptBlock::new(
            TranscriptBlockKind::SystemNotice,
            lines,
        ));
    }

    pub(crate) fn append_history(
        &mut self,
        blocks: impl IntoIterator<Item = FinalizedTranscriptBlock>,
    ) -> usize {
        let count = append_history_blocks(&mut self.store, blocks);
        if count > 0 {
            self.runtime.handle_event(TuiRuntimeEvent::HistoryLoaded);
        }
        count
    }

    #[cfg(test)]
    pub(crate) fn start_live_text(
        &mut self,
        item_id: impl Into<String>,
        kind: LiveTextKind,
        text: impl AsRef<str>,
    ) {
        self.live.start_text(item_id, kind, text);
        self.runtime.handle_event(TuiRuntimeEvent::Stream);
    }

    #[cfg(test)]
    pub(crate) fn append_live_text(&mut self, item_id: &str, text: &str) {
        self.live.append_text(item_id, text);
        self.runtime.handle_event(TuiRuntimeEvent::Stream);
    }

    #[cfg(test)]
    pub(crate) fn finalize_live_text(&mut self, item_id: &str) {
        if let Some(block) = self.live.finalize_text(item_id) {
            self.push_finalized(block);
            self.runtime.handle_event(TuiRuntimeEvent::LiveCommitTick);
        }
    }

    #[cfg(test)]
    pub(crate) fn start_tool_with_arguments(
        &mut self,
        item_id: impl Into<String>,
        call_id: impl Into<String>,
        name: impl Into<String>,
        arguments: impl Into<String>,
    ) {
        self.live
            .start_tool_with_arguments(item_id, call_id, name, arguments);
        self.runtime.handle_event(TuiRuntimeEvent::Stream);
    }

    #[cfg(test)]
    pub(crate) fn append_stable_tool_progress(&mut self, item_id: &str, text: impl Into<String>) {
        self.live.append_stable_tool_progress(item_id, text);
        self.runtime.handle_event(TuiRuntimeEvent::Stream);
    }

    pub(crate) fn apply_timeline_item_started(&mut self, item: &TalosTimelineItem) {
        let blocks = apply_item_started(&mut self.live, item);
        self.push_many_finalized(blocks);
        self.runtime.handle_event(TuiRuntimeEvent::Stream);
    }

    pub(crate) fn apply_timeline_item_delta(
        &mut self,
        item: &TalosTimelineItem,
        delta: TalosTimelineDelta,
    ) {
        let blocks = apply_item_delta(&mut self.live, item, delta);
        self.push_many_finalized(blocks);
        self.runtime.handle_event(TuiRuntimeEvent::Stream);
    }

    pub(crate) fn apply_timeline_item_completed(
        &mut self,
        item: &TalosTimelineItem,
        final_content: Option<&TalosTimelineContent>,
    ) {
        let blocks = apply_item_completed(&mut self.live, item, final_content);
        self.push_many_finalized(blocks);
        self.runtime.handle_event(TuiRuntimeEvent::LiveCommitTick);
    }

    pub(crate) fn apply_assistant_text_committed(
        &mut self,
        item_id: &str,
        phase: TalosTimelinePhase,
    ) {
        apply_assistant_text_committed(&mut self.live, item_id, phase);
        if let Some(item) = self.timeline_items.get_mut(item_id) {
            item.phase = phase;
        }
        self.runtime.handle_event(TuiRuntimeEvent::Stream);
    }

    pub(crate) fn apply_agent_event_payload(&mut self, payload: AgentEventPayload) {
        match payload {
            AgentEventPayload::TurnStarted(_) => {
                self.timeline_items.clear();
                self.handle_runtime_event(TuiRuntimeEvent::Stream);
            }
            AgentEventPayload::ItemStarted(payload) => {
                self.timeline_items
                    .insert(payload.item.item_id.clone(), payload.item.clone());
                if !self.is_duplicate_pending_user_input(&payload.item) {
                    self.apply_timeline_item_started(&payload.item);
                }
            }
            AgentEventPayload::ItemDelta(payload) => {
                let Some(item) = self.timeline_items.get(&payload.item_id).cloned() else {
                    return;
                };
                self.apply_timeline_item_delta(&item, payload.delta.clone());
                self.update_stored_timeline_item_delta(&payload.item_id, payload.delta);
            }
            AgentEventPayload::ItemCompleted(payload) => {
                let Some(mut item) = self.timeline_items.get(&payload.item_id).cloned() else {
                    return;
                };
                item.status = payload.status;
                item.completed_at_ms = Some(payload.completed_at_ms);
                if let Some(content) = payload.final_content.clone() {
                    item.content = content;
                }
                self.apply_timeline_item_completed(&item, payload.final_content.as_ref());
                self.timeline_items.insert(payload.item_id, item);
            }
            AgentEventPayload::AssistantTextCommitted(payload) => {
                self.apply_assistant_text_committed(&payload.item_id, payload.phase);
            }
            AgentEventPayload::TurnMetrics(_) | AgentEventPayload::TurnCompleted(_) => {
                self.handle_runtime_event(TuiRuntimeEvent::LiveCommitTick);
            }
            AgentEventPayload::TurnError(payload) => {
                self.push_finalized(FinalizedTranscriptBlock::new(
                    TranscriptBlockKind::SystemNotice,
                    vec![StyledLine::plain(format!(
                        "Stream error: {}",
                        payload.error
                    ))],
                ));
                self.timeline_items.clear();
            }
        }
    }

    pub(crate) fn handle_runtime_event(&mut self, event: TuiRuntimeEvent) {
        self.runtime.handle_event(event);
    }

    #[cfg(test)]
    pub(crate) fn run_cycle(&mut self) -> io::Result<RuntimeCycle>
    where
        W: Write,
        I: InteractionLayer,
    {
        let cycle = self.runtime.take_cycle();
        if cycle.flush_terminal_transcript {
            flush_pending_terminal_blocks(
                &mut self.store,
                &mut self.writer,
                &mut self.interaction_layer,
            )?;
        }
        Ok(cycle)
    }

    pub(crate) fn should_draw_frame(&mut self, fingerprint: u64) -> bool {
        self.runtime.should_draw_frame(fingerprint)
    }

    #[cfg(test)]
    pub(crate) fn flush_pending_terminal_to<W2: Write>(
        &mut self,
        writer: &mut TerminalTranscriptWriter<W2>,
        interaction_layer: &mut impl InteractionLayer,
    ) -> io::Result<bool> {
        flush_pending_terminal_blocks(&mut self.store, writer, interaction_layer)
    }

    pub(crate) fn pending_terminal_projection(&self) -> Vec<PendingTerminalBlock> {
        self.store.pending_terminal_projection()
    }

    pub(crate) fn mark_terminal_blocks_synced(
        &mut self,
        block_ids: impl IntoIterator<Item = TranscriptBlockId>,
    ) {
        self.store.mark_many_terminal_synced(block_ids);
    }

    pub(crate) fn store(&self) -> &TranscriptStore {
        &self.store
    }

    pub(crate) fn interactive_projection_lines(&self) -> Vec<StyledLine> {
        let mut lines = self.store.interactive_projection_lines();
        let live_lines = self.live.interactive_projection_lines();
        if !lines.is_empty() && !live_lines.is_empty() {
            lines.push(StyledLine::plain(""));
        }
        lines.extend(live_lines);
        lines
    }

    pub(crate) fn live_projection_lines(&self) -> Vec<StyledLine> {
        self.live.interactive_projection_lines()
    }

    pub(crate) fn live_projection_blocks(&self) -> Vec<LiveProjectionBlock> {
        self.live.interactive_projection_blocks()
    }

    pub(crate) fn interactive_block_count(&self) -> usize {
        self.store.blocks().len() + self.live.active_projection_count()
    }

    pub(crate) fn reset_transcript(&mut self) {
        self.store.clear();
        self.live = LiveReducer::default();
        self.timeline_items.clear();
    }

    #[cfg(test)]
    pub(crate) fn into_parts(self) -> (TranscriptStore, TerminalTranscriptWriter<W>, I) {
        (self.store, self.writer, self.interaction_layer)
    }

    fn push_finalized(&mut self, block: FinalizedTranscriptBlock) {
        self.store.push_finalized(block);
        self.runtime.handle_event(TuiRuntimeEvent::LiveCommitTick);
    }

    fn push_many_finalized(&mut self, blocks: impl IntoIterator<Item = FinalizedTranscriptBlock>) {
        let mut pushed = false;
        for block in blocks {
            self.store.push_finalized(block);
            pushed = true;
        }
        if pushed {
            self.runtime.handle_event(TuiRuntimeEvent::LiveCommitTick);
        }
    }

    fn update_stored_timeline_item_delta(&mut self, item_id: &str, delta: TalosTimelineDelta) {
        let Some(item) = self.timeline_items.get_mut(item_id) else {
            return;
        };
        match delta {
            TalosTimelineDelta::Text { text } => {
                if let TalosTimelineContent::Text { text: existing } = &mut item.content {
                    existing.push_str(&text);
                }
            }
            TalosTimelineDelta::ToolResult { content, is_error } => {
                if let TalosTimelineContent::ToolCall { result, .. } = &mut item.content {
                    *result = Some(talos_sdk::TalosToolResult { content, is_error });
                }
            }
            TalosTimelineDelta::ToolArguments { arguments_delta } => {
                if let TalosTimelineContent::ToolCall { arguments, .. } = &mut item.content {
                    arguments.push_str(&arguments_delta);
                }
            }
            TalosTimelineDelta::ToolProgress { .. }
            | TalosTimelineDelta::ToolDynamicRequest { .. }
            | TalosTimelineDelta::ContextCompaction { .. }
            | TalosTimelineDelta::Status { .. } => {}
        }
    }

    fn is_duplicate_pending_user_input(&self, item: &TalosTimelineItem) -> bool {
        let TalosTimelineItem {
            kind: TalosTimelineItemKind::UserInput,
            phase: TalosTimelinePhase::Input,
            content: TalosTimelineContent::Text { text },
            ..
        } = item
        else {
            return false;
        };
        self.store.blocks().last().is_some_and(|block| {
            block.kind == TranscriptBlockKind::UserInput
                && block
                    .lines
                    .first()
                    .is_some_and(|line| line.content.plain_text() == *text)
        })
    }
}

impl BufferedRenderRuntimeEngine {
    pub(crate) fn new_buffered() -> Self {
        Self {
            runtime: RuntimeController::default(),
            live: LiveReducer::default(),
            store: TranscriptStore::default(),
            #[cfg(test)]
            writer: TerminalTranscriptWriter::new(Vec::new()),
            #[cfg(test)]
            interaction_layer: BufferedInteractionLayer,
            timeline_items: HashMap::new(),
            _marker: PhantomData,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{self, Write};

    use super::RenderRuntimeEngine;
    use crate::render_runtime::common::style::StyledLine;
    use crate::render_runtime::render::live::LiveTextKind;
    use crate::render_runtime::runtime::controller::RuntimeCycle;
    use crate::render_runtime::transcript::block::{FinalizedTranscriptBlock, TranscriptBlockKind};
    use crate::render_runtime::transcript::writer::InteractionLayer;
    use talos_sdk::{
        AgentEventPayload, TalosAssistantTextCommittedEvent, TalosItemCompletedEvent,
        TalosItemDeltaEvent, TalosItemStartedEvent, TalosTimelineContent, TalosTimelineDelta,
        TalosTimelineItem, TalosTimelineItemKind, TalosTimelineItemStatus, TalosTimelinePhase,
        TalosTurnErrorEvent,
    };

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
    fn finalized_user_input_flushes_to_terminal_projection() {
        let mut engine = RenderRuntimeEngine::new(Vec::new(), TestInteractionLayer);
        engine.append_user_input("hello");

        let cycle = engine.run_cycle().unwrap();
        let (_, writer, _) = engine.into_parts();

        assert_eq!(
            cycle,
            RuntimeCycle {
                flush_terminal_transcript: true,
                commit_live_stream: false,
                draw_frame: true,
            }
        );
        assert_eq!(
            String::from_utf8(writer.into_inner()).unwrap(),
            "[suspend]hello\u{1b}[0m\n[repaint][restore]"
        );
    }

    #[test]
    fn live_text_flushes_only_after_finalize() {
        let mut engine = RenderRuntimeEngine::new(Vec::new(), TestInteractionLayer);
        engine.start_live_text("item-1", LiveTextKind::Commentary, "hello");
        engine.append_live_text("item-1", " world");

        let live_cycle = engine.run_cycle().unwrap();
        assert_eq!(
            live_cycle,
            RuntimeCycle {
                flush_terminal_transcript: false,
                commit_live_stream: true,
                draw_frame: true,
            }
        );

        engine.finalize_live_text("item-1");
        let finalized_cycle = engine.run_cycle().unwrap();
        let (_, writer, _) = engine.into_parts();

        assert!(finalized_cycle.flush_terminal_transcript);
        assert_eq!(
            String::from_utf8(writer.into_inner()).unwrap(),
            "[suspend]hello world\u{1b}[0m\n[repaint][restore]"
        );
    }

    #[test]
    fn history_blocks_enter_store_and_terminal_pending_path() {
        let mut engine = RenderRuntimeEngine::new(Vec::new(), TestInteractionLayer);
        let count = engine.append_history([FinalizedTranscriptBlock::new(
            TranscriptBlockKind::FinalAnswer,
            vec![StyledLine::plain("done")],
        )]);

        assert_eq!(count, 1);
        assert_eq!(engine.store().blocks().len(), 1);

        engine.run_cycle().unwrap();
        let (_, writer, _) = engine.into_parts();

        assert_eq!(
            String::from_utf8(writer.into_inner()).unwrap(),
            "[suspend]done\u{1b}[0m\n[repaint][restore]"
        );
    }

    fn timeline_text_item(phase: TalosTimelinePhase) -> TalosTimelineItem {
        TalosTimelineItem {
            item_id: "item-1".to_string(),
            parent_item_id: None,
            kind: TalosTimelineItemKind::AssistantText,
            phase,
            status: TalosTimelineItemStatus::Running,
            title: None,
            started_at_ms: 1,
            completed_at_ms: None,
            content: TalosTimelineContent::Text {
                text: String::new(),
            },
        }
    }

    fn timeline_user_input_item(text: &str) -> TalosTimelineItem {
        TalosTimelineItem {
            item_id: "user-item-1".to_string(),
            parent_item_id: None,
            kind: TalosTimelineItemKind::UserInput,
            phase: TalosTimelinePhase::Input,
            status: TalosTimelineItemStatus::Completed,
            title: None,
            started_at_ms: 1,
            completed_at_ms: Some(1),
            content: TalosTimelineContent::Text {
                text: text.to_string(),
            },
        }
    }

    #[test]
    fn agent_event_user_input_started_does_not_duplicate_local_pending_user_block() {
        let mut engine = RenderRuntimeEngine::new(Vec::new(), TestInteractionLayer);
        engine.append_user_input("hello");
        let item = timeline_user_input_item("hello");

        engine.apply_agent_event_payload(AgentEventPayload::ItemStarted(TalosItemStartedEvent {
            request_id: "request-1".to_string(),
            turn_id: "turn-1".to_string(),
            item,
        }));

        let user_blocks = engine
            .store()
            .blocks()
            .iter()
            .filter(|block| block.kind == TranscriptBlockKind::UserInput)
            .count();
        assert_eq!(user_blocks, 1);
    }

    #[test]
    fn agent_event_user_input_started_does_not_duplicate_local_synced_user_block() {
        let mut engine = RenderRuntimeEngine::new(Vec::new(), TestInteractionLayer);
        engine.append_user_input("hello");
        let synced_ids = engine
            .pending_terminal_projection()
            .into_iter()
            .map(|block| block.block_id)
            .collect::<Vec<_>>();
        engine.mark_terminal_blocks_synced(synced_ids);
        let item = timeline_user_input_item("hello");

        engine.apply_agent_event_payload(AgentEventPayload::ItemStarted(TalosItemStartedEvent {
            request_id: "request-1".to_string(),
            turn_id: "turn-1".to_string(),
            item,
        }));

        let user_blocks = engine
            .store()
            .blocks()
            .iter()
            .filter(|block| block.kind == TranscriptBlockKind::UserInput)
            .count();
        assert_eq!(user_blocks, 1);
    }

    #[test]
    fn engine_consumes_timeline_text_lifecycle_without_early_flush() {
        let mut engine = RenderRuntimeEngine::new(Vec::new(), TestInteractionLayer);
        let item = timeline_text_item(TalosTimelinePhase::Unknown);

        engine.apply_timeline_item_started(&item);
        engine.apply_timeline_item_delta(
            &item,
            TalosTimelineDelta::Text {
                text: "hello".to_string(),
            },
        );
        engine.apply_assistant_text_committed(&item.item_id, TalosTimelinePhase::FinalAnswer);
        let live_cycle = engine.run_cycle().unwrap();
        assert!(!live_cycle.flush_terminal_transcript);

        engine.apply_timeline_item_completed(&item, None);
        let finalized_cycle = engine.run_cycle().unwrap();
        let (_, writer, _) = engine.into_parts();

        assert!(finalized_cycle.flush_terminal_transcript);
        assert_eq!(
            String::from_utf8(writer.into_inner()).unwrap(),
            "[suspend]hello\u{1b}[0m\n[repaint][restore]"
        );
    }

    #[test]
    fn engine_consumes_agent_event_payload_lifecycle() {
        let mut engine = RenderRuntimeEngine::new(Vec::new(), TestInteractionLayer);
        let item = timeline_text_item(TalosTimelinePhase::Unknown);

        engine.apply_agent_event_payload(AgentEventPayload::ItemStarted(TalosItemStartedEvent {
            request_id: "request-1".to_string(),
            turn_id: "turn-1".to_string(),
            item: item.clone(),
        }));
        engine.apply_agent_event_payload(AgentEventPayload::ItemDelta(TalosItemDeltaEvent {
            request_id: "request-1".to_string(),
            turn_id: "turn-1".to_string(),
            item_id: item.item_id.clone(),
            delta: TalosTimelineDelta::Text {
                text: "hello".to_string(),
            },
        }));
        engine.apply_agent_event_payload(AgentEventPayload::AssistantTextCommitted(
            TalosAssistantTextCommittedEvent {
                request_id: "request-1".to_string(),
                turn_id: "turn-1".to_string(),
                item_id: item.item_id.clone(),
                phase: TalosTimelinePhase::FinalAnswer,
            },
        ));
        engine.apply_agent_event_payload(AgentEventPayload::ItemCompleted(
            TalosItemCompletedEvent {
                request_id: "request-1".to_string(),
                turn_id: "turn-1".to_string(),
                item_id: item.item_id,
                status: TalosTimelineItemStatus::Completed,
                completed_at_ms: 2,
                final_content: None,
            },
        ));
        engine.run_cycle().unwrap();
        let (_, writer, _) = engine.into_parts();

        assert_eq!(
            String::from_utf8(writer.into_inner()).unwrap(),
            "[suspend]hello\u{1b}[0m\n[repaint][restore]"
        );
    }

    #[test]
    fn engine_converts_turn_error_to_system_notice_projection() {
        let mut engine = RenderRuntimeEngine::new(Vec::new(), TestInteractionLayer);

        engine.apply_agent_event_payload(AgentEventPayload::TurnError(TalosTurnErrorEvent {
            request_id: "request-1".to_string(),
            turn_id: "turn-1".to_string(),
            error: "boom".to_string(),
            error_info: None,
            failed_at_ms: 2,
        }));
        engine.run_cycle().unwrap();
        let (_, writer, _) = engine.into_parts();

        assert_eq!(
            String::from_utf8(writer.into_inner()).unwrap(),
            "[suspend]Stream error: boom\u{1b}[0m\n[repaint][restore]"
        );
    }

    #[test]
    fn engine_flushes_pending_transcript_to_external_writer() {
        let mut engine = super::BufferedRenderRuntimeEngine::new_buffered();
        let mut writer =
            crate::render_runtime::transcript::writer::TerminalTranscriptWriter::new(Vec::new());

        engine.append_user_input("external writer");
        let flushed = engine
            .flush_pending_terminal_to(&mut writer, &mut TestInteractionLayer)
            .unwrap();

        assert!(flushed);
        assert!(engine.store().pending_terminal_projection().is_empty());
        assert_eq!(
            String::from_utf8(writer.into_inner()).unwrap(),
            "[suspend]external writer\u{1b}[0m\n[repaint][restore]"
        );
    }

    #[test]
    fn reset_transcript_clears_store_and_live_state() {
        let mut engine = super::BufferedRenderRuntimeEngine::new_buffered();

        engine.append_user_input("hello");
        engine.start_live_text("item-1", LiveTextKind::FinalAnswer, "pending");
        engine.reset_transcript();

        assert!(engine.store().blocks().is_empty());
        engine.finalize_live_text("item-1");
        assert!(engine.store().blocks().is_empty());
    }

    #[test]
    fn interactive_projection_includes_finalized_and_live_text() {
        let mut engine = super::BufferedRenderRuntimeEngine::new_buffered();

        engine.append_user_input("hello");
        engine.start_live_text("item-1", LiveTextKind::FinalAnswer, "live answer");

        let lines = engine.interactive_projection_lines();

        assert_eq!(
            lines.iter().map(StyledLine::plain_text).collect::<Vec<_>>(),
            vec![
                "hello".to_string(),
                String::new(),
                "live answer".to_string()
            ]
        );
        assert_eq!(engine.interactive_block_count(), 2);
    }
}
