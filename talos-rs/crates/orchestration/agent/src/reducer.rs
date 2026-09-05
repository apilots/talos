use talos_agent_api::{
    AgentAssistantTextPhase, AgentContextCompaction, AgentContextCompactionDelta, AgentEvent,
    AgentEventPayload, AgentMessage, AgentMessageBlock, AgentMessageBlockCompleted,
    AgentMessageBlockContent, AgentMessageBlockDelta, AgentMessageBlockDeltaContent,
    AgentMessageBlockStatus, AgentMessageStatus, AgentRunMetrics, AgentToolCallArguments,
    AgentTurnRef,
};

use crate::{AgentError, AgentResult};

/// Durable reducer state reconstructed from canonical Agent events.
#[derive(Debug, Clone, PartialEq)]
pub struct AgentRunCheckpoint {
    pub last_sequence: u64,
    pub messages: Vec<AgentMessage>,
    pub metrics: AgentRunMetrics,
}

/// Deterministic authority for canonical Agent message and metrics state.
///
/// Harness owns one reducer per live invocation and restores it from a
/// checkpoint for continuation. The reducer never reads transport framing,
/// Store Records, Provider-native state, or free-form JSON payloads.
#[derive(Debug, Clone)]
pub struct AgentEventReducer {
    identity: AgentTurnRef,
    turn_started: bool,
    terminal: bool,
    checkpoint: AgentRunCheckpoint,
}

impl AgentEventReducer {
    /// Create reducer state for a fresh logical Turn.
    pub fn new(identity: AgentTurnRef) -> Self {
        Self {
            identity,
            turn_started: false,
            terminal: false,
            checkpoint: AgentRunCheckpoint {
                last_sequence: 0,
                messages: Vec::new(),
                metrics: AgentRunMetrics::default(),
            },
        }
    }

    /// Restore a non-terminal logical Turn from its canonical checkpoint.
    pub fn from_checkpoint(
        identity: AgentTurnRef,
        checkpoint: AgentRunCheckpoint,
    ) -> AgentResult<Self> {
        if checkpoint.last_sequence == 0 {
            return Err(AgentError::invalid_execution_context(
                "continued Agent checkpoint must follow TurnStarted",
            ));
        }
        if checkpoint
            .messages
            .iter()
            .any(|message| message.turn_id != identity.turn_id)
        {
            return Err(AgentError::invalid_execution_context(
                "Agent checkpoint contains a message from another Turn",
            ));
        }
        Ok(Self {
            identity,
            turn_started: true,
            terminal: false,
            checkpoint,
        })
    }

    /// Return the next canonical sequence accepted by this reducer.
    pub fn next_sequence(&self) -> u64 {
        self.checkpoint.last_sequence.saturating_add(1)
    }

    pub(crate) fn identity(&self) -> &AgentTurnRef {
        &self.identity
    }

    /// Apply one canonical event after validating identity, order, and lifecycle.
    pub fn apply(&mut self, event: AgentEvent) -> AgentResult<()> {
        if event.identity != self.identity {
            return Err(AgentError::invalid_execution_context(
                "Agent event identity does not match reducer identity",
            ));
        }
        if event.sequence != self.next_sequence() {
            return Err(AgentError::invalid_execution_context(format!(
                "Agent event sequence {} does not match expected {}",
                event.sequence,
                self.next_sequence()
            )));
        }
        if self.terminal {
            return Err(AgentError::run_conflict(
                "closed Agent Turn rejects late events",
            ));
        }
        let is_turn_started = matches!(&event.payload, AgentEventPayload::TurnStarted(_));
        if !self.turn_started && !is_turn_started {
            return Err(AgentError::run_conflict(
                "Agent Turn must start before other events",
            ));
        }

        self.apply_payload(event.payload)?;
        self.checkpoint.last_sequence = event.sequence;
        Ok(())
    }

    /// Snapshot reducer state for persistence and continuation.
    pub fn checkpoint(&self) -> AgentRunCheckpoint {
        self.checkpoint.clone()
    }

    /// Close every open Provider-owned message lifecycle before abnormal settlement.
    ///
    /// Harness is the only caller allowed to synthesize these status-only
    /// snapshots, after Provider cleanup is confirmed or while rebuilding a
    /// durable Turn from its last checkpoint.
    pub fn close_open_lifecycle(
        &mut self,
        message_status: AgentMessageStatus,
        block_status: AgentMessageBlockStatus,
        occurred_at: i64,
    ) -> AgentResult<Vec<AgentEvent>> {
        if !is_terminal_message_status(message_status) || !is_terminal_block_status(block_status) {
            return Err(AgentError::run_conflict(
                "Agent lifecycle closure requires terminal statuses",
            ));
        }
        let message_ids = self
            .checkpoint
            .messages
            .iter()
            .map(|message| message.message_id.clone())
            .collect::<Vec<_>>();
        let mut events = Vec::new();
        for message_id in message_ids {
            let open_block_ids = self
                .checkpoint
                .messages
                .iter()
                .find(|message| message.message_id == message_id)
                .expect("checkpoint message identity remains stable")
                .blocks
                .iter()
                .filter(|block| !is_terminal_block_status(block.status))
                .map(|block| block.block_id.clone())
                .collect::<Vec<_>>();
            for block_id in open_block_ids {
                let block = self.completed_block_snapshot(
                    &message_id,
                    &block_id,
                    block_status,
                    occurred_at,
                )?;
                let event = AgentEvent {
                    identity: self.identity.clone(),
                    sequence: self.next_sequence(),
                    occurred_at,
                    payload: AgentEventPayload::MessageBlockCompleted(AgentMessageBlockCompleted {
                        message_id: message_id.clone(),
                        block,
                    }),
                };
                self.apply(event.clone())?;
                events.push(event);
            }
            let message_is_open = self
                .checkpoint
                .messages
                .iter()
                .find(|message| message.message_id == message_id)
                .is_some_and(|message| !is_terminal_message_status(message.status));
            if message_is_open {
                let message =
                    self.completed_message_snapshot(&message_id, message_status, occurred_at)?;
                let event = AgentEvent {
                    identity: self.identity.clone(),
                    sequence: self.next_sequence(),
                    occurred_at,
                    payload: AgentEventPayload::MessageCompleted(message),
                };
                self.apply(event.clone())?;
                events.push(event);
            }
        }
        Ok(events)
    }

    pub(crate) fn completed_block_snapshot(
        &self,
        message_id: &str,
        block_id: &str,
        status: AgentMessageBlockStatus,
        completed_at: i64,
    ) -> AgentResult<AgentMessageBlock> {
        let message = self
            .checkpoint
            .messages
            .iter()
            .find(|message| message.message_id == message_id)
            .ok_or_else(|| AgentError::run_conflict("Agent message does not exist"))?;
        let block = message
            .blocks
            .iter()
            .find(|block| block.block_id == block_id)
            .ok_or_else(|| AgentError::run_conflict("Agent message block does not exist"))?;
        let mut completed = block.clone();
        completed.content = finalized_content(&block.content, status)?;
        completed.status = status;
        completed.completed_at = Some(completed_at);
        Ok(completed)
    }

    pub(crate) fn completed_message_snapshot(
        &self,
        message_id: &str,
        status: AgentMessageStatus,
        completed_at: i64,
    ) -> AgentResult<AgentMessage> {
        let mut completed = self
            .checkpoint
            .messages
            .iter()
            .find(|message| message.message_id == message_id)
            .cloned()
            .ok_or_else(|| AgentError::run_conflict("Agent message does not exist"))?;
        completed.status = status;
        completed.completed_at = Some(completed_at);
        Ok(completed)
    }

    fn apply_payload(&mut self, payload: AgentEventPayload) -> AgentResult<()> {
        match payload {
            AgentEventPayload::TurnStarted(_) => {
                if self.turn_started {
                    return Err(AgentError::run_conflict(
                        "Agent TurnStarted may occur only once",
                    ));
                }
                self.turn_started = true;
            }
            AgentEventPayload::MessageStarted(started) => {
                validate_started_message(&self.identity, &started.message)?;
                if self
                    .checkpoint
                    .messages
                    .iter()
                    .any(|message| message.message_id == started.message.message_id)
                {
                    return Err(AgentError::run_conflict("Agent message already exists"));
                }
                self.checkpoint.messages.push(started.message);
            }
            AgentEventPayload::MessageBlockStarted(started) => {
                let message = find_message_mut(&mut self.checkpoint.messages, &started.message_id)?;
                validate_started_block(message, &started.block)?;
                message.blocks.push(started.block);
            }
            AgentEventPayload::MessageBlockDelta(delta) => {
                apply_block_delta(&mut self.checkpoint.messages, delta)?;
            }
            AgentEventPayload::MessageBlockCompleted(completed) => {
                let message =
                    find_message_mut(&mut self.checkpoint.messages, &completed.message_id)?;
                let block = find_block_mut(message, &completed.block.block_id)?;
                let mut expected = block.clone();
                expected.content = finalized_content(&block.content, completed.block.status)?;
                expected.status = completed.block.status;
                expected.completed_at = completed.block.completed_at;
                if !is_terminal_block_status(completed.block.status)
                    || completed.block.completed_at.is_none()
                    || completed.block != expected
                {
                    return Err(AgentError::run_conflict(
                        "completed Agent block snapshot does not match reducer state",
                    ));
                }
                *block = completed.block;
            }
            AgentEventPayload::MessageCompleted(completed) => {
                let message =
                    find_message_mut(&mut self.checkpoint.messages, completed.message_id.as_str())?;
                let mut expected = message.clone();
                expected.status = completed.status;
                expected.completed_at = completed.completed_at;
                if !is_terminal_message_status(completed.status)
                    || completed.completed_at.is_none()
                    || completed.blocks.iter().any(|block| {
                        !is_terminal_block_status(block.status) || block.completed_at.is_none()
                    })
                    || completed != expected
                {
                    return Err(AgentError::run_conflict(
                        "completed Agent message snapshot does not match reducer state",
                    ));
                }
                *message = completed;
            }
            AgentEventPayload::WaitingForInput(pending) => {
                let waiting_block_id = match &pending {
                    talos_agent_api::AgentPendingInput::Message { .. } => None,
                    talos_agent_api::AgentPendingInput::UserAction { action } => {
                        action.item_id.as_deref()
                    }
                };
                if let Some(block_id) = waiting_block_id {
                    let block = find_block_by_id_mut(&mut self.checkpoint.messages, block_id)?;
                    if block.status != AgentMessageBlockStatus::Running {
                        return Err(AgentError::run_conflict(
                            "Agent pending input requires a running message block",
                        ));
                    }
                    block.status = AgentMessageBlockStatus::WaitingForInput;
                }
            }
            AgentEventPayload::MetricsUpdated(metrics) => {
                validate_monotonic_metrics(&self.checkpoint.metrics, &metrics)?;
                self.checkpoint.metrics = metrics;
            }
            AgentEventPayload::TurnCompleted(_) | AgentEventPayload::TurnFailed(_) => {
                if self.checkpoint.messages.iter().any(|message| {
                    !is_terminal_message_status(message.status)
                        || message.completed_at.is_none()
                        || message.blocks.iter().any(|block| {
                            !is_terminal_block_status(block.status) || block.completed_at.is_none()
                        })
                }) {
                    return Err(AgentError::run_conflict(
                        "Agent Turn cannot finish with open message lifecycle",
                    ));
                }
                self.terminal = true;
            }
        }
        Ok(())
    }
}

fn validate_started_message(identity: &AgentTurnRef, message: &AgentMessage) -> AgentResult<()> {
    if message.turn_id != identity.turn_id
        || message.status != AgentMessageStatus::InProgress
        || message.completed_at.is_some()
        || !message.blocks.is_empty()
    {
        return Err(AgentError::run_conflict(
            "started Agent message has invalid identity or lifecycle state",
        ));
    }
    Ok(())
}

fn validate_started_block(message: &AgentMessage, block: &AgentMessageBlock) -> AgentResult<()> {
    if message.status != AgentMessageStatus::InProgress
        || block.status != AgentMessageBlockStatus::Running
        || block.completed_at.is_some()
        || message
            .blocks
            .iter()
            .any(|existing| existing.block_id == block.block_id)
    {
        return Err(AgentError::run_conflict(
            "started Agent block has invalid lifecycle state",
        ));
    }
    Ok(())
}

fn apply_block_delta(
    messages: &mut [AgentMessage],
    delta: AgentMessageBlockDelta,
) -> AgentResult<()> {
    let message = find_message_mut(messages, &delta.message_id)?;
    let block = find_block_mut(message, &delta.block_id)?;
    if block.status != AgentMessageBlockStatus::Running {
        return Err(AgentError::run_conflict(
            "Agent block delta requires a running block",
        ));
    }
    match (&mut block.content, delta.delta) {
        (
            AgentMessageBlockContent::Reasoning(content),
            AgentMessageBlockDeltaContent::Text(delta),
        ) => content.text.push_str(&delta),
        (
            AgentMessageBlockContent::AssistantText(content),
            AgentMessageBlockDeltaContent::Text(delta),
        ) => content.text.push_str(&delta),
        (
            AgentMessageBlockContent::AssistantText(content),
            AgentMessageBlockDeltaContent::AssistantTextPhase(phase),
        ) => {
            if content.phase != AgentAssistantTextPhase::Pending
                || phase == AgentAssistantTextPhase::Pending
            {
                return Err(AgentError::run_conflict(
                    "Agent assistant text phase can be committed only once",
                ));
            }
            content.phase = phase;
        }
        (
            AgentMessageBlockContent::ExecutionSummary(content),
            AgentMessageBlockDeltaContent::Text(delta),
        ) => content.text.push_str(&delta),
        (
            AgentMessageBlockContent::ToolCall(tool_call),
            AgentMessageBlockDeltaContent::ToolCallArguments(delta),
        ) => match &mut tool_call.arguments {
            AgentToolCallArguments::Streaming { json_fragment } => {
                json_fragment.push_str(&delta.json_fragment);
            }
            AgentToolCallArguments::Complete { .. } => {
                return Err(AgentError::run_conflict(
                    "completed Tool arguments reject deltas",
                ));
            }
        },
        (
            AgentMessageBlockContent::ToolCall(tool_call),
            AgentMessageBlockDeltaContent::ToolProgress(progress),
        ) => tool_call.progress.push(progress),
        (
            AgentMessageBlockContent::ContextCompaction(compaction),
            AgentMessageBlockDeltaContent::ContextCompaction(delta),
        ) => apply_compaction_delta(compaction, delta),
        _ => {
            return Err(AgentError::run_conflict(
                "Agent block delta kind does not match block content",
            ));
        }
    }
    Ok(())
}

fn apply_compaction_delta(
    compaction: &mut AgentContextCompaction,
    delta: AgentContextCompactionDelta,
) {
    if let Some(stage) = delta.stage {
        compaction.stage = stage;
    }
    if delta.outcome.is_some() {
        compaction.outcome = delta.outcome;
    }
    if delta.metrics.is_some() {
        compaction.metrics = delta.metrics;
    }
    if delta.message.is_some() {
        compaction.message = delta.message;
    }
}

fn finalized_content(
    content: &AgentMessageBlockContent,
    status: AgentMessageBlockStatus,
) -> AgentResult<AgentMessageBlockContent> {
    let mut content = content.clone();
    if status != AgentMessageBlockStatus::Completed {
        return Ok(content);
    }
    let AgentMessageBlockContent::ToolCall(tool_call) = &mut content else {
        return Ok(content);
    };
    let AgentToolCallArguments::Streaming { json_fragment } = &tool_call.arguments else {
        return Ok(content);
    };
    let value: serde_json::Value = serde_json::from_str(json_fragment).map_err(|error| {
        AgentError::invalid_execution_context(format!(
            "completed Tool arguments are invalid JSON: {error}"
        ))
    })?;
    tool_call.arguments = AgentToolCallArguments::Complete {
        value: talos_types::ToolArguments::from(value),
    };
    Ok(content)
}

fn validate_monotonic_metrics(
    previous: &AgentRunMetrics,
    next: &AgentRunMetrics,
) -> AgentResult<()> {
    let previous_usage = &previous.usage;
    let next_usage = &next.usage;
    if next.model_rounds < previous.model_rounds
        || next.tool_calls < previous.tool_calls
        || next.tool_successes < previous.tool_successes
        || next.tool_errors < previous.tool_errors
        || next_usage.input_tokens < previous_usage.input_tokens
        || next_usage.output_tokens < previous_usage.output_tokens
        || next_usage.total_tokens < previous_usage.total_tokens
        || optional_counter_decreased(
            previous_usage.cache_creation_input_tokens,
            next_usage.cache_creation_input_tokens,
        )
        || optional_counter_decreased(
            previous_usage.cache_read_input_tokens,
            next_usage.cache_read_input_tokens,
        )
    {
        return Err(AgentError::run_conflict(
            "Agent metrics update must be cumulative",
        ));
    }
    Ok(())
}

fn optional_counter_decreased(previous: Option<u32>, next: Option<u32>) -> bool {
    match (previous, next) {
        (Some(_), None) => true,
        (Some(previous), Some(next)) => next < previous,
        (None, None | Some(_)) => false,
    }
}

fn find_message_mut<'a>(
    messages: &'a mut [AgentMessage],
    message_id: &str,
) -> AgentResult<&'a mut AgentMessage> {
    messages
        .iter_mut()
        .find(|message| message.message_id == message_id)
        .ok_or_else(|| AgentError::run_conflict("Agent message does not exist"))
}

fn find_block_mut<'a>(
    message: &'a mut AgentMessage,
    block_id: &str,
) -> AgentResult<&'a mut AgentMessageBlock> {
    message
        .blocks
        .iter_mut()
        .find(|block| block.block_id == block_id)
        .ok_or_else(|| AgentError::run_conflict("Agent message block does not exist"))
}

fn find_block_by_id_mut<'a>(
    messages: &'a mut [AgentMessage],
    block_id: &str,
) -> AgentResult<&'a mut AgentMessageBlock> {
    messages
        .iter_mut()
        .flat_map(|message| message.blocks.iter_mut())
        .find(|block| block.block_id == block_id)
        .ok_or_else(|| AgentError::run_conflict("Agent pending input block does not exist"))
}

fn is_terminal_message_status(status: AgentMessageStatus) -> bool {
    matches!(
        status,
        AgentMessageStatus::Completed | AgentMessageStatus::Failed | AgentMessageStatus::Cancelled
    )
}

fn is_terminal_block_status(status: AgentMessageBlockStatus) -> bool {
    matches!(
        status,
        AgentMessageBlockStatus::Completed
            | AgentMessageBlockStatus::Failed
            | AgentMessageBlockStatus::Cancelled
    )
}

#[cfg(test)]
#[path = "reducer_tests.rs"]
mod tests;
