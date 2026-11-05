//! Composes the render runtime, live reducer, and transcript store.

use std::collections::{HashMap, HashSet};

use crate::render_runtime::common::style::StyledLine;
use crate::render_runtime::render::live::{LiveProjectionBlock, LiveReducer};
use crate::render_runtime::render::timeline::{
    apply_block_completed, apply_block_delta, apply_block_started,
};
use crate::render_runtime::runtime::controller::RuntimeController;
use crate::render_runtime::runtime::event_loop::TuiRuntimeEvent;
use crate::render_runtime::transcript::block::TranscriptBlockId;
use crate::render_runtime::transcript::block::{FinalizedTranscriptBlock, TranscriptBlockKind};
use crate::render_runtime::transcript::store::PendingTerminalBlock;
use crate::render_runtime::transcript::store::TranscriptStore;
use talos_sdk::{
    AgentAssistantTextPhase, AgentEventPayload, AgentMessageBlock, AgentMessageBlockContent,
    AgentMessageBlockDeltaContent,
};

/// Runtime engine for the render runtime.
///
/// This owns the transcript data flow for interactive and terminal projections.
#[derive(Default)]
pub(crate) struct RenderRuntimeEngine {
    runtime: RuntimeController,
    live: LiveReducer,
    store: TranscriptStore,
    message_blocks: HashMap<String, AgentMessageBlock>,
    tool_call_blocks: HashMap<String, String>,
    local_user_input_block: Option<TranscriptBlockId>,
    automation_inputs: HashMap<(String, String), talos_sdk::AutomationInputOrigin>,
    input_blocks: HashMap<(String, String), TranscriptBlockId>,
    input_created_at: HashMap<(String, String), i64>,
    annotated_inputs: HashSet<(String, String)>,
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

    pub(crate) fn set_automation_input(
        &mut self,
        turn_id: &str,
        input: talos_sdk::AutomationInputOrigin,
    ) {
        let key = (turn_id.to_string(), input.message_id.clone());
        self.automation_inputs.insert(key.clone(), input);
        self.annotate_input(&key);
    }
    pub(crate) fn loop_input_created_at(&self, turn_id: &str, loop_id: &str) -> Option<i64> {
        self.automation_inputs.iter().find_map(|(key, input)| {
            (key.0 == turn_id && matches!(&input.origin, talos_sdk::AutomationTurnOrigin::Loop { loop_id: id, .. } if id == loop_id))
                .then(|| self.input_created_at.get(key).copied()).flatten()
        })
    }
    fn annotate_input(&mut self, key: &(String, String)) {
        if self.annotated_inputs.contains(key) {
            return;
        }
        let (Some(input), Some(block)) =
            (self.automation_inputs.get(key), self.input_blocks.get(key))
        else {
            return;
        };
        let label = match input.origin {
            talos_sdk::AutomationTurnOrigin::Goal { .. } => "  ◎ Goal",
            talos_sdk::AutomationTurnOrigin::Loop { .. } => "  ↻ Loop",
        };
        self.store.append_input_metadata(*block, label);
        self.annotated_inputs.insert(key.clone());
        self.runtime.handle_event(TuiRuntimeEvent::LiveCommitTick);
    }
    pub(crate) fn apply_turn_event_payload(&mut self, turn_id: &str, payload: AgentEventPayload) {
        let input = match &payload {
            AgentEventPayload::MessageStarted(started) => Some(&started.message),
            AgentEventPayload::MessageCompleted(message) => Some(message),
            _ => None,
        };
        if let Some(message) = input.filter(|m| m.role == talos_sdk::AgentMessageRole::User) {
            self.input_created_at.insert(
                (turn_id.into(), message.message_id.clone()),
                message.created_at,
            );
        }
        let key = match &payload {
            AgentEventPayload::MessageBlockStarted(started)
                if matches!(
                    started.block.content,
                    AgentMessageBlockContent::UserContent(_)
                ) =>
            {
                Some((turn_id.to_string(), started.message_id.clone()))
            }
            AgentEventPayload::MessageCompleted(message)
                if message.role == talos_sdk::AgentMessageRole::User =>
            {
                Some((turn_id.to_string(), message.message_id.clone()))
            }
            _ => None,
        };
        if let AgentEventPayload::MessageCompleted(message) = &payload {
            if message.role == talos_sdk::AgentMessageRole::User
                && key
                    .as_ref()
                    .is_some_and(|key| !self.input_blocks.contains_key(key))
            {
                self.push_many_finalized(
                    crate::render_runtime::render::history::agent_message_to_blocks(message),
                );
            }
        }
        let complete = matches!(payload, AgentEventPayload::MessageCompleted(_));
        self.apply_agent_event_payload(payload);
        if let Some(key) = key {
            if let Some(block) = self
                .store
                .blocks()
                .last()
                .filter(|block| block.kind == TranscriptBlockKind::UserInput)
            {
                self.input_blocks.insert(key.clone(), block.id);
            }
            if complete {
                self.annotate_input(&key);
            }
        }
    }

    pub(crate) fn apply_agent_event_payload(&mut self, payload: AgentEventPayload) {
        match payload {
            AgentEventPayload::TurnStarted(_) => {
                self.message_blocks.clear();
                self.tool_call_blocks.clear();
                self.handle_runtime_event(TuiRuntimeEvent::Stream);
            }
            AgentEventPayload::MessageStarted(_) => {}
            AgentEventPayload::MessageBlockStarted(started) => {
                if let AgentMessageBlockContent::ToolCall(tool) = &started.block.content {
                    self.tool_call_blocks
                        .insert(tool.call_id.clone(), started.block.block_id.clone());
                }
                let finalized = apply_block_started(&mut self.live, &started.block);
                self.message_blocks
                    .insert(started.block.block_id.clone(), started.block);
                self.push_many_finalized(finalized);
                self.runtime.handle_event(TuiRuntimeEvent::Stream);
            }
            AgentEventPayload::MessageBlockDelta(delta) => {
                let Some(block) = self.message_blocks.get(&delta.block_id).cloned() else {
                    return;
                };
                let finalized = apply_block_delta(&mut self.live, &block, delta.delta.clone());
                if let Some(block) = self.message_blocks.get_mut(&delta.block_id) {
                    match delta.delta {
                        AgentMessageBlockDeltaContent::Text(text) => match &mut block.content {
                            AgentMessageBlockContent::AssistantText(assistant) => {
                                assistant.text.push_str(&text);
                            }
                            AgentMessageBlockContent::Reasoning(reasoning) => {
                                reasoning.text.push_str(&text);
                            }
                            AgentMessageBlockContent::UserContent(_)
                            | AgentMessageBlockContent::ToolCall(_)
                            | AgentMessageBlockContent::ToolResult(_)
                            | AgentMessageBlockContent::ExecutionSummary(_)
                            | AgentMessageBlockContent::ContextCompaction(_)
                            | AgentMessageBlockContent::SystemNotice(_) => {}
                        },
                        AgentMessageBlockDeltaContent::AssistantTextPhase(phase) => {
                            if let AgentMessageBlockContent::AssistantText(assistant) =
                                &mut block.content
                            {
                                assistant.phase = phase;
                            }
                        }
                        AgentMessageBlockDeltaContent::ToolCallArguments(_)
                        | AgentMessageBlockDeltaContent::ToolProgress(_)
                        | AgentMessageBlockDeltaContent::ContextCompaction(_) => {}
                    }
                }
                self.push_many_finalized(finalized);
                self.runtime.handle_event(TuiRuntimeEvent::Stream);
            }
            AgentEventPayload::MessageBlockCompleted(completed) => {
                let tool_call_block_id = match &completed.block.content {
                    AgentMessageBlockContent::ToolResult(result) => {
                        self.tool_call_blocks.get(&result.call_id).cloned()
                    }
                    _ => None,
                };
                let finalized = apply_block_completed(
                    &mut self.live,
                    &completed.block,
                    tool_call_block_id.as_deref(),
                );
                self.message_blocks
                    .insert(completed.block.block_id.clone(), completed.block);
                self.push_many_finalized(finalized);
                self.runtime.handle_event(TuiRuntimeEvent::LiveCommitTick);
            }
            AgentEventPayload::MessageCompleted(_)
            | AgentEventPayload::WaitingForInput(_)
            | AgentEventPayload::MetricsUpdated(_)
            | AgentEventPayload::TurnCompleted(_) => {
                self.handle_runtime_event(TuiRuntimeEvent::LiveCommitTick);
            }
            AgentEventPayload::TurnFailed(payload) => {
                self.push_finalized(FinalizedTranscriptBlock::new(
                    TranscriptBlockKind::SystemNotice,
                    vec![StyledLine::plain(format!(
                        "Stream error: {}",
                        payload.error.message
                    ))],
                ));
                self.message_blocks.clear();
                self.tool_call_blocks.clear();
            }
        }
    }

    pub(crate) fn assistant_text_phase(&self, block_id: &str) -> Option<AgentAssistantTextPhase> {
        match &self.message_blocks.get(block_id)?.content {
            AgentMessageBlockContent::AssistantText(assistant) => Some(assistant.phase),
            _ => None,
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
        self.message_blocks.clear();
        self.tool_call_blocks.clear();
        self.local_user_input_block = None;
        self.automation_inputs.clear();
        self.input_blocks.clear();
        self.input_created_at.clear();
        self.annotated_inputs.clear();
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
}

#[cfg(test)]
#[path = "engine_tests.rs"]
mod tests;
