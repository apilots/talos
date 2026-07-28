//! Server-owned projection from existing Turn, ActiveTurn, and history facts.
//!
//! This module formats bounded Subagent snapshots for Tool results. It does
//! not own lifecycle state, persist a second status, or expose client APIs.

use talos_error::{TalosError, TalosResult};
use talos_store::session::ActiveTurnStatus;
use talos_types::talos::{
    AgentEvent, AgentEventPayload, StopReason, TalosTimelineContent, TalosTimelineItem,
    TalosTimelinePhase,
};
use talos_types::{
    SubagentOutcome, SubagentRef, SubagentSnapshot, SubagentUpdate, SubagentUpdateKind,
};

use crate::history::timeline_items_from_events;

use super::service::SubagentService;

const MAX_TERMINAL_OUTPUT_BYTES: usize = 2 * 1024;
const MAX_SUBAGENT_TURN_EVENTS: usize = 256;

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
        let active = self
            .timeline
            .get_active_turn(&subagent.session_id)
            .await?
            .ok_or_else(|| {
                TalosError::Conflict(format!(
                    "subagent '{}' has no turn state",
                    subagent.session_id
                ))
            })?;
        let turn_events = self
            .complete_turn_events(&subagent.session_id, &active.turn_id)
            .await?;
        let turn_items = self
            .authoritative_turn_items(
                &subagent.session_id,
                active.status,
                active.message_id.as_deref(),
                &turn_events,
            )
            .await?;
        let outcome = Self::build_outcome(active.status, active.turn_id, &turn_events, &turn_items);
        Ok(Self::build_snapshot(
            subagent,
            agent_id,
            &turn_items,
            outcome,
        ))
    }

    pub(super) async fn snapshot_exact(
        &self,
        subagent: SubagentRef,
        turn_id: &str,
    ) -> TalosResult<SubagentSnapshot> {
        let active = self.timeline.get_active_turn(&subagent.session_id).await?;
        let active = active.ok_or_else(|| {
            TalosError::Conflict(format!(
                "subagent '{}' has no turn state",
                subagent.session_id
            ))
        })?;
        if active.turn_id != turn_id {
            return Err(TalosError::Conflict(format!(
                "turn '{turn_id}' is not the current turn for subagent '{}'",
                subagent.session_id
            )));
        }
        let turn_events = self
            .complete_turn_events(&subagent.session_id, turn_id)
            .await?;
        let turn_items = self
            .authoritative_turn_items(
                &subagent.session_id,
                active.status,
                active.message_id.as_deref(),
                &turn_events,
            )
            .await?;
        let outcome = Self::build_outcome(
            active.status,
            turn_id.to_string(),
            &turn_events,
            &turn_items,
        );
        let agent_id = self.subagent_agent_id(&subagent).await?;
        Ok(Self::build_snapshot(
            subagent,
            agent_id,
            &turn_items,
            outcome,
        ))
    }

    fn build_outcome(
        status: ActiveTurnStatus,
        turn_id: String,
        turn_events: &[AgentEvent],
        turn_items: &[TalosTimelineItem],
    ) -> SubagentOutcome {
        match status {
            ActiveTurnStatus::Running => SubagentOutcome::Running { turn_id },
            ActiveTurnStatus::WaitingUserAction => SubagentOutcome::WaitingUserAction { turn_id },
            ActiveTurnStatus::Cancelling => SubagentOutcome::Cancelling { turn_id },
            ActiveTurnStatus::Completed => {
                let stop_reason = turn_events
                    .iter()
                    .rev()
                    .find_map(|event| match &event.payload {
                        AgentEventPayload::TurnCompleted(payload) => Some(payload.stop_reason),
                        _ => None,
                    });
                if stop_reason == Some(StopReason::Cancelled) {
                    SubagentOutcome::Interrupted { turn_id }
                } else {
                    let (output, truncated) =
                        bounded_final_answer(turn_items, MAX_TERMINAL_OUTPUT_BYTES);
                    SubagentOutcome::Completed {
                        turn_id,
                        output,
                        truncated,
                    }
                }
            }
            ActiveTurnStatus::Failed => {
                let error = turn_events
                    .iter()
                    .rev()
                    .find_map(|event| match &event.payload {
                        AgentEventPayload::TurnError(payload) => Some(payload.error.clone()),
                        _ => None,
                    })
                    .unwrap_or_else(|| "subagent turn failed".to_string());
                let (error, truncated) = truncate_utf8(error, MAX_TERMINAL_OUTPUT_BYTES);
                SubagentOutcome::Failed {
                    error,
                    turn_id,
                    truncated,
                }
            }
        }
    }

    async fn subagent_agent_id(&self, subagent: &SubagentRef) -> TalosResult<String> {
        let record = self
            .sessions
            .get_session_record(&subagent.session_id)
            .await?
            .ok_or_else(|| {
                TalosError::ResourceNotFound(format!("subagent {}", subagent.session_id))
            })?;
        Ok(record.agent_id)
    }

    fn build_snapshot(
        subagent: SubagentRef,
        agent_id: String,
        turn_items: &[TalosTimelineItem],
        outcome: SubagentOutcome,
    ) -> SubagentSnapshot {
        let latest_update = turn_items.iter().rev().find_map(|item| {
            let kind = match item.phase {
                TalosTimelinePhase::Commentary => SubagentUpdateKind::Commentary,
                TalosTimelinePhase::FinalAnswer => SubagentUpdateKind::FinalAnswer,
                _ => return None,
            };
            let TalosTimelineContent::Text { text } = &item.content else {
                return None;
            };
            let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
            if normalized.is_empty() {
                return None;
            }
            let (text, truncated) = truncate_utf8(normalized, MAX_TERMINAL_OUTPUT_BYTES);
            Some(SubagentUpdate {
                kind,
                text,
                truncated,
            })
        });
        SubagentSnapshot {
            subagent,
            agent_id,
            latest_update,
            outcome,
        }
    }

    async fn complete_turn_events(
        &self,
        session_id: &str,
        turn_id: &str,
    ) -> TalosResult<Vec<AgentEvent>> {
        self.timeline
            .list_turn_events(session_id, turn_id, MAX_SUBAGENT_TURN_EVENTS)
            .await
    }

    async fn authoritative_turn_items(
        &self,
        session_id: &str,
        status: ActiveTurnStatus,
        message_id: Option<&str>,
        tail_events: &[AgentEvent],
    ) -> TalosResult<Vec<TalosTimelineItem>> {
        if status != ActiveTurnStatus::Completed {
            return Ok(timeline_items_from_events(tail_events));
        }
        let stop_reason = tail_events.iter().rev().find_map(|event| {
            let AgentEventPayload::TurnCompleted(completed) = &event.payload else {
                return None;
            };
            Some(completed.stop_reason)
        });
        if stop_reason == Some(StopReason::Cancelled) {
            return Ok(timeline_items_from_events(tail_events));
        }
        let session = self
            .sessions
            .get_session(session_id)
            .await?
            .ok_or_else(|| TalosError::ResourceNotFound(format!("subagent {session_id}")))?;
        let message_id = message_id.ok_or_else(|| {
            TalosError::Conflict(format!(
                "completed subagent turn for session '{session_id}' has no message identity"
            ))
        })?;
        self.history.latest_turn_items(&session, message_id).await
    }
}

fn bounded_final_answer(turn_items: &[TalosTimelineItem], max_bytes: usize) -> (String, bool) {
    let mut output = String::new();
    for text in turn_items
        .iter()
        .filter(|item| item.phase == TalosTimelinePhase::FinalAnswer)
        .filter_map(|item| match &item.content {
            TalosTimelineContent::Text { text } => Some(text.as_str()),
            _ => None,
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
