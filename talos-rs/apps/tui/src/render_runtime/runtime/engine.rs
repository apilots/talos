//! Composes the render runtime, live reducer, and transcript store.

use std::collections::HashMap;

use crate::render_runtime::common::style::StyledLine;
use crate::render_runtime::render::live::{LiveProjectionBlock, LiveReducer};
use crate::render_runtime::render::timeline::{
    apply_assistant_text_committed, apply_item_completed, apply_item_delta, apply_item_started,
};
use crate::render_runtime::runtime::controller::RuntimeController;
use crate::render_runtime::runtime::event_loop::TuiRuntimeEvent;
use crate::render_runtime::transcript::block::TranscriptBlockId;
use crate::render_runtime::transcript::block::{FinalizedTranscriptBlock, TranscriptBlockKind};
use crate::render_runtime::transcript::store::PendingTerminalBlock;
use crate::render_runtime::transcript::store::TranscriptStore;
use talos_sdk::{
    AgentEventPayload, TalosTimelineContent, TalosTimelineDelta, TalosTimelineItem,
    TalosTimelinePhase,
};

/// Runtime engine for the render runtime.
///
/// This owns the transcript data flow for interactive and terminal projections.
#[derive(Default)]
pub(crate) struct RenderRuntimeEngine {
    runtime: RuntimeController,
    live: LiveReducer,
    store: TranscriptStore,
    timeline_items: HashMap<String, TalosTimelineItem>,
    local_user_input_block: Option<TranscriptBlockId>,
}

impl RenderRuntimeEngine {
    pub(crate) fn append_user_input(&mut self, text: impl Into<String>) {
        let text = text.into();
        if text.trim().is_empty() {
            return;
        }
        let block_id = self.store.push_finalized(FinalizedTranscriptBlock::new(
            TranscriptBlockKind::UserInput,
            vec![StyledLine::plain(text)],
        ));
        self.local_user_input_block = Some(block_id);
        self.runtime.handle_event(TuiRuntimeEvent::LiveCommitTick);
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
        let mut count = 0usize;
        for block in blocks {
            self.store.push_finalized(block);
            count += 1;
        }
        if count > 0 {
            self.runtime.handle_event(TuiRuntimeEvent::HistoryLoaded);
        }
        count
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
        let finalized = apply_assistant_text_committed(&mut self.live, item_id, phase);
        if let Some(item) = self.timeline_items.get_mut(item_id) {
            item.phase = phase;
        }
        if let Some(block) = finalized {
            self.push_finalized(block);
        } else {
            self.runtime.handle_event(TuiRuntimeEvent::Stream);
        }
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
                self.apply_timeline_item_started(&payload.item);
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
                        payload.error.message
                    ))],
                ));
                self.timeline_items.clear();
            }
        }
    }

    pub(crate) fn handle_runtime_event(&mut self, event: TuiRuntimeEvent) {
        self.runtime.handle_event(event);
    }

    pub(crate) fn should_draw_frame(&mut self, fingerprint: u64) -> bool {
        self.runtime.should_draw_frame(fingerprint)
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
        self.local_user_input_block = None;
    }

    fn push_finalized(&mut self, block: FinalizedTranscriptBlock) {
        self.store.push_finalized(block);
        self.runtime.handle_event(TuiRuntimeEvent::LiveCommitTick);
    }

    fn push_many_finalized(&mut self, blocks: impl IntoIterator<Item = FinalizedTranscriptBlock>) {
        let mut pushed = false;
        for block in blocks {
            if block.kind == TranscriptBlockKind::UserInput {
                if let Some(block_id) = self.local_user_input_block.take() {
                    if self.store.replace_user_input(block_id, block.clone()) {
                        pushed = true;
                        continue;
                    }
                }
            }
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
            | TalosTimelineDelta::UserAction { .. }
            | TalosTimelineDelta::ContextCompaction { .. }
            | TalosTimelineDelta::Status { .. } => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RenderRuntimeEngine;
    use crate::render_runtime::common::style::StyledLine;
    use crate::render_runtime::render::live::LiveTextKind;
    use crate::render_runtime::transcript::block::{FinalizedTranscriptBlock, TranscriptBlockKind};
    use crate::test_support::{append_live_text, finalize_live_text, start_live_text};
    use talos_sdk::{
        AgentEventPayload, TalosAssistantTextCommittedEvent, TalosItemCompletedEvent,
        TalosItemDeltaEvent, TalosItemStartedEvent, TalosTimelineContent, TalosTimelineDelta,
        TalosTimelineItem, TalosTimelineItemKind, TalosTimelineItemStatus, TalosTimelinePhase,
        TalosTurnErrorEvent,
    };

    fn pending_plain_text(engine: &RenderRuntimeEngine) -> Vec<String> {
        engine
            .pending_terminal_projection()
            .into_iter()
            .flat_map(|block| block.lines)
            .map(|line| line.plain_text())
            .collect()
    }

    #[test]
    fn finalized_user_input_enters_terminal_projection() {
        let mut engine = RenderRuntimeEngine::default();
        engine.append_user_input("hello");

        assert_eq!(pending_plain_text(&engine), vec!["hello"]);
    }

    #[test]
    fn live_text_flushes_only_after_finalize() {
        let mut engine = RenderRuntimeEngine::default();
        start_live_text(&mut engine, "item-1", LiveTextKind::Commentary, "hello");
        append_live_text(&mut engine, "item-1", LiveTextKind::Commentary, " world");

        assert!(engine.pending_terminal_projection().is_empty());

        finalize_live_text(&mut engine, "item-1", LiveTextKind::Commentary);

        assert_eq!(pending_plain_text(&engine), vec!["hello world"]);
    }

    #[test]
    fn history_blocks_enter_store_and_terminal_pending_path() {
        let mut engine = RenderRuntimeEngine::default();
        let count = engine.append_history([FinalizedTranscriptBlock::new(
            TranscriptBlockKind::FinalAnswer,
            vec![StyledLine::plain("done")],
        )]);

        assert_eq!(count, 1);
        assert_eq!(engine.store().blocks().len(), 1);
        assert_eq!(pending_plain_text(&engine), vec!["done"]);
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
    fn agent_event_user_input_started_renders_server_user_block() {
        let mut engine = RenderRuntimeEngine::default();
        let item = timeline_user_input_item("hello");

        engine.apply_agent_event_payload(AgentEventPayload::ItemStarted(TalosItemStartedEvent {
            request_id: "request-1".to_string(),
            turn_id: "turn-1".to_string(),
            item,
        }));

        assert_eq!(pending_plain_text(&engine), vec!["hello"]);
    }

    #[test]
    fn agent_event_user_input_started_replaces_local_pending_user_block() {
        let mut engine = RenderRuntimeEngine::default();
        engine.append_user_input("local input");
        let item = timeline_user_input_item("server input");

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
            .collect::<Vec<_>>();
        assert_eq!(user_blocks.len(), 1);
        assert_eq!(user_blocks[0].lines[0].content.plain_text(), "server input");
    }

    #[test]
    fn agent_event_user_input_started_replaces_local_synced_user_block() {
        let mut engine = RenderRuntimeEngine::default();
        engine.append_user_input("local input");
        let synced_ids = engine
            .pending_terminal_projection()
            .into_iter()
            .map(|block| block.block_id)
            .collect::<Vec<_>>();
        engine.mark_terminal_blocks_synced(synced_ids);
        let item = timeline_user_input_item("server input");

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
            .collect::<Vec<_>>();
        assert_eq!(user_blocks.len(), 1);
        assert_eq!(user_blocks[0].lines[0].content.plain_text(), "server input");
    }

    #[test]
    fn agent_event_user_input_started_preserves_history_user_block() {
        let mut engine = RenderRuntimeEngine::default();
        engine.append_history([FinalizedTranscriptBlock::new(
            TranscriptBlockKind::UserInput,
            vec![StyledLine::plain("history input")],
        )]);
        let item = timeline_user_input_item("server input");

        engine.apply_agent_event_payload(AgentEventPayload::ItemStarted(TalosItemStartedEvent {
            request_id: "request-1".to_string(),
            turn_id: "turn-1".to_string(),
            item,
        }));

        let user_text = engine
            .store()
            .blocks()
            .iter()
            .filter(|block| block.kind == TranscriptBlockKind::UserInput)
            .map(|block| block.lines[0].content.plain_text())
            .collect::<Vec<_>>();
        assert_eq!(user_text, vec!["history input", "server input"]);
    }

    #[test]
    fn agent_events_interleave_same_turn_user_inputs_with_model_work() {
        let mut engine = RenderRuntimeEngine::default();
        let initial = timeline_user_input_item("initial");
        let mut steer = timeline_user_input_item("steer");
        steer.item_id = "user-item-2".to_string();
        let mut before_steer = timeline_text_item(TalosTimelinePhase::Commentary);
        before_steer.item_id = "assistant-item-1".to_string();
        let mut after_steer = timeline_text_item(TalosTimelinePhase::FinalAnswer);
        after_steer.item_id = "assistant-item-2".to_string();

        engine.apply_timeline_item_started(&initial);
        engine.apply_timeline_item_started(&before_steer);
        engine.apply_timeline_item_delta(
            &before_steer,
            TalosTimelineDelta::Text {
                text: "before steer".to_string(),
            },
        );
        engine.apply_timeline_item_completed(&before_steer, None);
        engine.apply_timeline_item_started(&steer);
        engine.apply_timeline_item_started(&after_steer);
        engine.apply_timeline_item_delta(
            &after_steer,
            TalosTimelineDelta::Text {
                text: "after steer".to_string(),
            },
        );
        engine.apply_timeline_item_completed(&after_steer, None);

        let transcript = engine
            .store()
            .blocks()
            .iter()
            .map(|block| (block.kind, block.lines[0].content.plain_text()))
            .collect::<Vec<_>>();
        assert_eq!(
            transcript,
            vec![
                (TranscriptBlockKind::UserInput, "initial".to_string()),
                (TranscriptBlockKind::Commentary, "before steer".to_string(),),
                (TranscriptBlockKind::UserInput, "steer".to_string()),
                (TranscriptBlockKind::FinalAnswer, "after steer".to_string(),),
            ]
        );
    }

    #[test]
    fn engine_consumes_timeline_text_lifecycle_without_early_flush() {
        let mut engine = RenderRuntimeEngine::default();
        let item = timeline_text_item(TalosTimelinePhase::Unknown);

        engine.apply_timeline_item_started(&item);
        engine.apply_timeline_item_delta(
            &item,
            TalosTimelineDelta::Text {
                text: "hello".to_string(),
            },
        );
        engine.apply_assistant_text_committed(&item.item_id, TalosTimelinePhase::FinalAnswer);
        assert!(engine.pending_terminal_projection().is_empty());

        engine.apply_timeline_item_completed(&item, None);

        assert_eq!(pending_plain_text(&engine), vec!["hello"]);
    }

    #[test]
    fn completed_unknown_text_waits_for_authoritative_phase_commit() {
        let mut engine = RenderRuntimeEngine::default();
        let item = timeline_text_item(TalosTimelinePhase::Unknown);

        engine.apply_timeline_item_started(&item);
        engine.apply_timeline_item_delta(
            &item,
            TalosTimelineDelta::Text {
                text: "working update".to_string(),
            },
        );
        engine.apply_timeline_item_completed(&item, None);

        assert!(engine.pending_terminal_projection().is_empty());
        assert_eq!(
            engine
                .live_projection_lines()
                .iter()
                .map(StyledLine::plain_text)
                .collect::<Vec<_>>(),
            vec!["working update".to_string()]
        );

        engine.apply_assistant_text_committed(&item.item_id, TalosTimelinePhase::Commentary);

        let pending = engine.pending_terminal_projection();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].kind, TranscriptBlockKind::Commentary);
        assert_eq!(pending[0].lines[0].plain_text(), "working update");
        assert!(engine.live_projection_lines().is_empty());
    }

    #[test]
    fn engine_consumes_agent_event_payload_lifecycle() {
        let mut engine = RenderRuntimeEngine::default();
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

        assert_eq!(pending_plain_text(&engine), vec!["hello"]);
    }

    #[test]
    fn engine_converts_turn_error_to_system_notice_projection() {
        let mut engine = RenderRuntimeEngine::default();

        engine.apply_agent_event_payload(AgentEventPayload::TurnError(TalosTurnErrorEvent {
            request_id: "request-1".to_string(),
            turn_id: "turn-1".to_string(),
            error: talos_error::ApiError {
                code: talos_error::ErrorCode::InternalError,
                message: "boom".to_string(),
            },
            failed_at_ms: 2,
        }));

        assert_eq!(pending_plain_text(&engine), vec!["Stream error: boom"]);
    }

    #[test]
    fn reset_transcript_clears_store_and_live_state() {
        let mut engine = RenderRuntimeEngine::default();

        engine.append_user_input("hello");
        start_live_text(&mut engine, "item-1", LiveTextKind::FinalAnswer, "pending");
        engine.reset_transcript();

        assert!(engine.store().blocks().is_empty());
        finalize_live_text(&mut engine, "item-1", LiveTextKind::FinalAnswer);
        assert!(engine.store().blocks().is_empty());
    }

    #[test]
    fn interactive_projection_includes_finalized_and_live_text() {
        let mut engine = RenderRuntimeEngine::default();

        engine.append_user_input("hello");
        start_live_text(
            &mut engine,
            "item-1",
            LiveTextKind::FinalAnswer,
            "live answer",
        );

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
