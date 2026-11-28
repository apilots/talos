use std::collections::HashMap;
use std::sync::Arc;

use crate::timeline::TimelineEventId;
use crate::TimelineEventService;
use talos_agent_api::{
    AgentEventPayload, AgentRunFailure, AgentRunOutput, AgentRunResult, SessionHistoryResponse,
    SessionHistoryTurn,
};
use talos_error::{ErrorCode, TalosError, TalosResult};

const MAX_HISTORY_READ_ATTEMPTS: usize = 3;

const DEFAULT_HISTORY_PAGE_SIZE: usize = 100;
const MAX_HISTORY_PAGE_SIZE: usize = 1000;
const HISTORY_CURSOR_PREFIX: &str = "history_";

/// Product-owned Session history view for display and local historical reads.
///
/// History is reduced only from durable Agent events. It does not merge Kernel
/// artifacts, legacy Timeline items, or transport DTOs into a second history
/// model. Provider context is read exclusively from the Worker's execution Store.
#[derive(Clone)]
pub struct SessionHistoryService {
    event_service: Arc<TimelineEventService>,
}

impl SessionHistoryService {
    pub(crate) fn supports_history_supplements(&self) -> bool {
        self.event_service.supports_history_supplements()
    }
    pub(crate) async fn read_terminal_history_supplement(
        &self,
        identity: &talos_agent_api::AgentTurnRef,
    ) -> TalosResult<Option<talos_agent_api::trajectory::SessionHistorySupplement>> {
        self.event_service
            .read_terminal_history_supplement(identity)
            .await
    }
    pub(crate) async fn read_execution_payload(
        &self,
        identity: &talos_agent_api::AgentTurnRef,
        payload_id: &str,
    ) -> TalosResult<Option<String>> {
        self.event_service
            .read_execution_payload(identity, payload_id)
            .await
    }
    pub fn new(event_service: Arc<TimelineEventService>) -> Self {
        Self { event_service }
    }

    pub async fn read(
        &self,
        session_id: &str,
        cursor: Option<&str>,
        limit: Option<usize>,
    ) -> TalosResult<SessionHistoryResponse> {
        for _ in 0..MAX_HISTORY_READ_ATTEMPTS {
            let generation = self.event_service.history_generation(session_id).await?;
            let result = self.read_page(session_id, cursor, limit).await;
            if generation == self.event_service.history_generation(session_id).await? {
                return result;
            }
            // A cursor belongs to its original generation and cannot restart at a new cut.
            if cursor.is_some() {
                return Err(invalid_history_cursor());
            }
        }
        Err(history_changed())
    }

    async fn read_page(
        &self,
        session_id: &str,
        cursor: Option<&str>,
        limit: Option<usize>,
    ) -> TalosResult<SessionHistoryResponse> {
        let page_size = limit
            .unwrap_or(DEFAULT_HISTORY_PAGE_SIZE)
            .clamp(1, MAX_HISTORY_PAGE_SIZE);
        let (high_water_event_seq, after_event_seq) = match cursor {
            Some(cursor) => {
                let parsed = parse_history_cursor(cursor)?;
                self.validate_cursor_event(session_id, parsed.0, false)
                    .await?;
                parsed
            }
            None => (
                self.event_service
                    .latest_event_id(session_id)
                    .await?
                    .map(|event_id| event_id.event_seq())
                    .transpose()?
                    .unwrap_or_default(),
                0,
            ),
        };
        if after_event_seq > high_water_event_seq {
            return Err(invalid_history_cursor());
        }
        if high_water_event_seq == 0 {
            return Ok(SessionHistoryResponse {
                data: Vec::new(),
                next_cursor: None,
                last_stable_event_id: None,
            });
        }
        if after_event_seq > 0 {
            self.validate_cursor_event(session_id, after_event_seq, true)
                .await?;
        }

        let mut completed = self
            .read_completed_after(
                session_id,
                after_event_seq,
                Some(high_water_event_seq),
                page_size + 1,
            )
            .await?;

        let has_next_page = completed.len() > page_size;
        completed.truncate(page_size);
        let next_cursor = has_next_page
            .then(|| completed.last().map(|(event_seq, _)| *event_seq))
            .flatten()
            .map(|event_seq| format_history_cursor(high_water_event_seq, event_seq));
        let mut data: Vec<_> = completed.into_iter().map(|(_, turn)| turn).collect();
        let last_stable_event_id = data
            .last()
            .map(|turn| TimelineEventId::from_seq(turn.completed_event_seq).to_string());
        self.attach_notices(&mut data, high_water_event_seq).await?;
        Ok(SessionHistoryResponse {
            data,
            next_cursor,
            last_stable_event_id,
        })
    }

    async fn validate_cursor_event(
        &self,
        session_id: &str,
        event_seq: i64,
        terminal_required: bool,
    ) -> TalosResult<()> {
        let event = self
            .event_service
            .get_event(event_seq)
            .await?
            .filter(|event| event.event.identity().session_id == session_id)
            .ok_or_else(invalid_history_cursor)?;
        if terminal_required
            && !matches!(
                event.event.agent_event().map(|event| &event.payload),
                Some(AgentEventPayload::TurnCompleted(_) | AgentEventPayload::TurnFailed(_))
            )
        {
            return Err(invalid_history_cursor());
        }
        Ok(())
    }

    async fn read_completed_after(
        &self,
        session_id: &str,
        after_event_seq: i64,
        through_event_seq: Option<i64>,
        limit: usize,
    ) -> TalosResult<Vec<(i64, SessionHistoryTurn)>> {
        let terminals = self
            .event_service
            .list_terminal_events_after(session_id, after_event_seq, through_event_seq, limit)
            .await?;
        let mut completed = Vec::with_capacity(terminals.len());
        for terminal in terminals {
            let event_seq = terminal.event_seq;
            completed.push((event_seq, self.reduce_terminal(terminal).await?));
        }
        Ok(completed)
    }

    async fn reduce_terminal(
        &self,
        terminal: talos_agent_api::TimelineEvent,
    ) -> TalosResult<SessionHistoryTurn> {
        let checkpoint = self
            .event_service
            .turn_checkpoint(terminal.event.identity(), terminal.event_seq)
            .await?;
        self.reduce_checkpoint(&terminal, checkpoint)
    }

    /// Enrich only returned Turns, never the pagination lookahead, at one shared high water.
    async fn attach_notices(
        &self,
        turns: &mut [SessionHistoryTurn],
        high_water: i64,
    ) -> TalosResult<()> {
        if turns.is_empty() {
            return Ok(());
        }
        let identities: Vec<_> = turns.iter().map(|turn| turn.identity.clone()).collect();
        let mut notices = HashMap::<_, Vec<_>>::new();
        for event in self
            .event_service
            .turn_notices(&identities, high_water)
            .await?
        {
            let identity = event.event.identity();
            notices
                .entry((identity.session_id.clone(), identity.turn_id.clone()))
                .or_default()
                .push(event);
        }
        let mut origins = self.event_service.read_input_origins(&identities).await?;
        for turn in turns {
            turn.automation_input = origins.remove(&(
                turn.identity.session_id.clone(),
                turn.identity.turn_id.clone(),
            ));
            turn.notices = notices
                .remove(&(
                    turn.identity.session_id.clone(),
                    turn.identity.turn_id.clone(),
                ))
                .unwrap_or_default();
        }
        Ok(())
    }

    fn reduce_checkpoint(
        &self,
        terminal: &talos_agent_api::TimelineEvent,
        checkpoint: talos_agent::AgentRunCheckpoint,
    ) -> TalosResult<SessionHistoryTurn> {
        let event = terminal
            .event
            .agent_event()
            .ok_or_else(|| TalosError::internal("History completion must be an Agent event."))?;
        let result = match &event.payload {
            AgentEventPayload::TurnCompleted(completion) => {
                AgentRunResult::Completed(AgentRunOutput {
                    messages: checkpoint.messages,
                    metrics: checkpoint.metrics,
                    completion: completion.clone(),
                })
            }
            AgentEventPayload::TurnFailed(failure) => AgentRunResult::Failed(AgentRunFailure {
                messages: checkpoint.messages,
                metrics: checkpoint.metrics,
                failure: failure.clone(),
            }),
            _ => {
                return Err(TalosError::internal(
                    "history terminal event is not terminal",
                ))
            }
        };
        Ok(SessionHistoryTurn {
            automation_input: None,
            identity: terminal.event.identity().clone(),
            result,
            notices: Vec::new(),
            completed_event_seq: terminal.event_seq,
            completed_at: event.occurred_at,
        })
    }
}

fn format_history_cursor(high_water_event_seq: i64, after_event_seq: i64) -> String {
    format!("{HISTORY_CURSOR_PREFIX}{high_water_event_seq}_{after_event_seq}")
}

fn parse_history_cursor(cursor: &str) -> TalosResult<(i64, i64)> {
    let raw = cursor
        .strip_prefix(HISTORY_CURSOR_PREFIX)
        .ok_or_else(invalid_history_cursor)?;
    let (high_water, after) = raw.split_once('_').ok_or_else(invalid_history_cursor)?;
    let high_water = high_water
        .parse::<i64>()
        .map_err(|_| invalid_history_cursor())?;
    let after = after.parse::<i64>().map_err(|_| invalid_history_cursor())?;
    if high_water <= 0 || after < 0 {
        return Err(invalid_history_cursor());
    }
    Ok((high_water, after))
}

fn history_changed() -> TalosError {
    TalosError::new(
        ErrorCode::Conflict,
        "Session history changed repeatedly while reading.",
    )
}

fn invalid_history_cursor() -> TalosError {
    TalosError::new(ErrorCode::InvalidRequest, "invalid Session history cursor")
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
