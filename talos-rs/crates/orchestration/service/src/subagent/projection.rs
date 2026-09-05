//! Server-owned projection from canonical Agent Turn facts.
//!
//! This module formats bounded Subagent snapshots for Tool results. It owns no
//! lifecycle state and reuses the Agent reducer instead of maintaining an item
//! delta reducer or reading Provider artifacts.

use talos_agent::AgentEventReducer;
use talos_agent_api::{
    ActiveTurnState, AgentAssistantTextPhase, AgentEventPayload, AgentMessage,
    AgentMessageBlockContent,
};
use talos_error::{TalosError, TalosResult};
use talos_types::{
    SubagentOutcome, SubagentRef, SubagentSnapshot, SubagentUpdate, SubagentUpdateKind,
};

use super::service::SubagentService;

const MAX_TERMINAL_OUTPUT_BYTES: usize = 2 * 1024;
impl SubagentService {
    pub(super) async fn snapshot(&self, subagent: SubagentRef) -> TalosResult<SubagentSnapshot> {
        let agent_id = self.subagent_agent_id(&subagent).await?;
        self.snapshot_with_agent_id(subagent, agent_id).await
    }

    pub(super) async fn snapshot_with_agent_id(
        &self,
        subagent: SubagentRef,
        agent_id: String,
    ) -> TalosResult<SubagentSnapshot> {
        if let Some(active) = self.timeline.get_active_turn(&subagent.session_id).await? {
            return self
                .build_turn_snapshot(
                    subagent,
                    agent_id,
                    active.identity.turn_id,
                    Some(active.state),
                )
                .await;
        }
        let terminal = self
            .timeline
            .get_latest_terminal_event(&subagent.session_id)
            .await?
            .ok_or_else(|| missing_turn_state(&subagent.session_id))?;
        self.build_turn_snapshot(subagent, agent_id, terminal.event.identity.turn_id, None)
            .await
    }

    pub(super) async fn snapshot_exact(
        &self,
        subagent: SubagentRef,
        turn_id: &str,
    ) -> TalosResult<SubagentSnapshot> {
        let active_state = self
            .timeline
            .get_active_turn(&subagent.session_id)
            .await?
            .filter(|active| active.identity.turn_id == turn_id)
            .map(|active| active.state);
        let agent_id = self.subagent_agent_id(&subagent).await?;
        self.build_turn_snapshot(subagent, agent_id, turn_id.to_string(), active_state)
            .await
    }

    async fn build_turn_snapshot(
        &self,
        subagent: SubagentRef,
        agent_id: String,
        turn_id: String,
        active_state: Option<ActiveTurnState>,
    ) -> TalosResult<SubagentSnapshot> {
        let identity = talos_agent_api::AgentTurnRef::new(&subagent.session_id, &turn_id);
        let mut reducer = AgentEventReducer::new(identity.clone());
        let mut last_event = None;
        self.timeline
            .scan_turn_events(&identity, |record| {
                let event = record.event;
                reducer
                    .apply(event.clone())
                    .map_err(talos_agent::AgentError::into_talos_error)?;
                last_event = Some(event);
                Ok(())
            })
            .await?;
        let last_event = last_event.ok_or_else(|| missing_turn_state(&subagent.session_id))?;
        let messages = reducer.checkpoint().messages;
        let outcome = build_outcome(active_state, turn_id, &last_event.payload, &messages)?;
        Ok(SubagentSnapshot {
            subagent,
            agent_id,
            latest_update: latest_update(&messages),
            outcome,
        })
    }

    async fn subagent_agent_id(&self, subagent: &SubagentRef) -> TalosResult<String> {
        self.sessions
            .load_record(&subagent.session_id)
            .await?
            .map(|record| record.agent_id)
            .ok_or_else(|| {
                TalosError::public(
                    talos_error::ErrorCode::ResourceNotFound,
                    format!("subagent {}", subagent.session_id),
                )
            })
    }
}

fn build_outcome(
    active_state: Option<ActiveTurnState>,
    turn_id: String,
    last_event: &AgentEventPayload,
    messages: &[AgentMessage],
) -> TalosResult<SubagentOutcome> {
    match last_event {
        AgentEventPayload::TurnCompleted(_) => {
            let (output, truncated) = bounded_final_answer(messages, MAX_TERMINAL_OUTPUT_BYTES);
            Ok(SubagentOutcome::Completed {
                turn_id,
                output,
                truncated,
            })
        }
        AgentEventPayload::TurnFailed(failed) => {
            if failed.error.code == talos_error::ErrorCode::Cancelled {
                return Ok(SubagentOutcome::Interrupted { turn_id });
            }
            let (error, truncated) =
                truncate_utf8(failed.error.message.clone(), MAX_TERMINAL_OUTPUT_BYTES);
            Ok(SubagentOutcome::Failed {
                error,
                turn_id,
                truncated,
            })
        }
        _ => match active_state {
            Some(ActiveTurnState::Running) => Ok(SubagentOutcome::Running { turn_id }),
            Some(ActiveTurnState::WaitingForInput) => {
                Ok(SubagentOutcome::WaitingUserAction { turn_id })
            }
            Some(ActiveTurnState::Cancelling) => Ok(SubagentOutcome::Cancelling { turn_id }),
            None => Err(TalosError::internal(
                "nonterminal Subagent history has no canonical active Turn",
            )),
        },
    }
}

fn latest_update(messages: &[AgentMessage]) -> Option<SubagentUpdate> {
    messages.iter().rev().find_map(|message| {
        message.blocks.iter().rev().find_map(|block| {
            let AgentMessageBlockContent::AssistantText(text) = &block.content else {
                return None;
            };
            let kind = match text.phase {
                AgentAssistantTextPhase::Commentary => SubagentUpdateKind::Commentary,
                AgentAssistantTextPhase::FinalAnswer => SubagentUpdateKind::FinalAnswer,
                AgentAssistantTextPhase::Pending => return None,
            };
            let normalized = text.text.split_whitespace().collect::<Vec<_>>().join(" ");
            if normalized.is_empty() {
                return None;
            }
            let (text, truncated) = truncate_utf8(normalized, MAX_TERMINAL_OUTPUT_BYTES);
            Some(SubagentUpdate {
                kind,
                text,
                truncated,
            })
        })
    })
}

fn bounded_final_answer(messages: &[AgentMessage], max_bytes: usize) -> (String, bool) {
    let mut output = String::new();
    for text in messages
        .iter()
        .flat_map(|message| &message.blocks)
        .filter_map(|block| {
            let AgentMessageBlockContent::AssistantText(text) = &block.content else {
                return None;
            };
            (text.phase == AgentAssistantTextPhase::FinalAnswer).then_some(text.text.as_str())
        })
    {
        let separator = usize::from(!output.is_empty());
        if output.len() + separator + text.len() <= max_bytes {
            if separator == 1 {
                output.push('\n');
            }
            output.push_str(text);
            continue;
        }
        let remaining = max_bytes.saturating_sub(output.len());
        if remaining > separator {
            if separator == 1 {
                output.push('\n');
            }
            let available = remaining - separator;
            let mut boundary = available.min(text.len());
            while !text.is_char_boundary(boundary) {
                boundary -= 1;
            }
            output.push_str(&text[..boundary]);
        }
        return (output, true);
    }
    (output, false)
}

fn missing_turn_state(session_id: &str) -> TalosError {
    TalosError::public(
        talos_error::ErrorCode::Conflict,
        format!("subagent '{session_id}' has no turn state"),
    )
}

fn truncate_utf8(value: String, max_bytes: usize) -> (String, bool) {
    if value.len() <= max_bytes {
        return (value, false);
    }
    let mut boundary = max_bytes;
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    (value[..boundary].to_string(), true)
}
