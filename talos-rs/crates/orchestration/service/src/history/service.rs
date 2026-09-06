use std::collections::HashMap;
use std::sync::Arc;

use crate::timeline::TimelineEventId;
use crate::TimelineEventService;
use talos_agent::{
    AgentEventReducer, AgentFuture, AgentResult, SessionHistoryReadParams, SessionHistoryReader,
    SessionHistoryWindow,
};
use talos_agent_api::{
    AgentEventPayload, AgentRunFailure, AgentRunOutput, AgentRunResult, SessionHistoryResponse,
    SessionHistoryTurn,
};
use talos_error::{ErrorCode, TalosError, TalosResult};

const HISTORY_SCAN_BATCH_SIZE: usize = 2048;
const DEFAULT_HISTORY_PAGE_SIZE: usize = 100;
const MAX_HISTORY_PAGE_SIZE: usize = 1000;
const HISTORY_CURSOR_PREFIX: &str = "history_";

/// Service-owned canonical Session history reader.
///
/// History is reduced only from durable Agent events. It does not merge Kernel
/// artifacts, legacy Timeline items, or transport DTOs into a second history
/// model.
#[derive(Clone)]
pub struct SessionHistoryService {
    event_service: Arc<TimelineEventService>,
}

impl SessionHistoryService {
    pub fn new(event_service: Arc<TimelineEventService>) -> Self {
        Self { event_service }
    }

    pub async fn read(
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
        self.attach_notices(&mut data, high_water_event_seq).await?;
        Ok(SessionHistoryResponse { data, next_cursor })
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

    async fn read_window(
        &self,
        params: SessionHistoryReadParams,
    ) -> TalosResult<Vec<SessionHistoryTurn>> {
        if params.limit == 0 {
            return Ok(Vec::new());
        }
        let limit = params.limit.clamp(1, MAX_HISTORY_PAGE_SIZE);
        let mut turns = match params.window {
            SessionHistoryWindow::Latest => {
                let terminals = self
                    .event_service
                    .list_latest_terminal_events(&params.session_id, None, limit)
                    .await?;
                let mut turns = Vec::with_capacity(terminals.len());
                for terminal in terminals {
                    turns.push(self.reduce_terminal(terminal).await?);
                }
                turns
            }
            SessionHistoryWindow::OldestAfter {
                completed_event_seq,
            } => self
                .read_completed_after(&params.session_id, completed_event_seq, None, limit)
                .await?
                .into_iter()
                .map(|(_, turn)| turn)
                .collect(),
        };
        let high_water = self
            .event_service
            .latest_event_id(&params.session_id)
            .await?
            .map(|id| id.event_seq())
            .transpose()?
            .unwrap_or_default();
        self.attach_notices(&mut turns, high_water).await?;
        Ok(turns)
    }

    async fn read_completed_after(
        &self,
        session_id: &str,
        after_event_seq: i64,
        through_event_seq: Option<i64>,
        limit: usize,
    ) -> TalosResult<Vec<(i64, SessionHistoryTurn)>> {
        let mut timeline_cursor =
            (after_event_seq > 0).then(|| TimelineEventId::from_seq(after_event_seq));
        let mut reducers = HashMap::new();
        let mut completed = Vec::with_capacity(limit);
        loop {
            let events = self
                .event_service
                .list_events_after(
                    session_id,
                    timeline_cursor.as_ref(),
                    HISTORY_SCAN_BATCH_SIZE,
                )
                .await?;
            if events.is_empty() {
                break;
            }
            for timeline_event in &events {
                if through_event_seq.is_some_and(|through| timeline_event.event_seq > through) {
                    break;
                }
                let Some(event) = timeline_event.event.agent_event().cloned() else {
                    continue;
                };
                let identity = event.identity.clone();
                let terminal = matches!(
                    event.payload,
                    AgentEventPayload::TurnCompleted(_) | AgentEventPayload::TurnFailed(_)
                );
                reducers
                    .entry(identity.turn_id.clone())
                    .or_insert_with(|| AgentEventReducer::new(identity))
                    .apply(event)
                    .map_err(|error| {
                        TalosError::internal_with_source("invalid canonical Session history", error)
                    })?;
                if !terminal {
                    continue;
                }
                let reducer = reducers
                    .remove(&timeline_event.event.identity().turn_id)
                    .expect("terminal Turn reducer exists");
                let turn = self.reduce_checkpoint(timeline_event, reducer.checkpoint())?;
                completed.push((timeline_event.event_seq, turn));
                if completed.len() == limit {
                    return Ok(completed);
                }
            }
            if events.len() < HISTORY_SCAN_BATCH_SIZE
                || through_event_seq.is_some_and(|through| {
                    events
                        .last()
                        .is_some_and(|event| event.event_seq >= through)
                })
            {
                break;
            }
            timeline_cursor = events
                .last()
                .map(|event| TimelineEventId::from(event.event_id.clone()));
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
        for turn in turns {
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
            identity: terminal.event.identity().clone(),
            result,
            notices: Vec::new(),
            completed_event_seq: terminal.event_seq,
            completed_at: event.occurred_at,
        })
    }
}

impl SessionHistoryReader for SessionHistoryService {
    fn read(
        &self,
        params: SessionHistoryReadParams,
    ) -> AgentFuture<'_, AgentResult<Vec<SessionHistoryTurn>>> {
        Box::pin(async move { self.read_window(params).await })
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

fn invalid_history_cursor() -> TalosError {
    TalosError::new(ErrorCode::InvalidRequest, "invalid Session history cursor")
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
