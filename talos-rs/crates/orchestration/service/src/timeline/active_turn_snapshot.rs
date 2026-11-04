//! Running Turn reads paired with the exact durable event offset they cover.
use talos_agent_api::{
    ActiveTurnSnapshot, ActiveTurnState, SessionActiveTurnResponse, TimelineActiveTurn,
};
use talos_error::TalosResult;

use super::{TimelineEventId, TimelineEventService, TimelineSubscriptionService};

impl TimelineEventService {
    pub(crate) async fn read_active_turn(
        &self,
        session_id: &str,
    ) -> TalosResult<SessionActiveTurnResponse> {
        // Capture the record first: execution can advance while reduction runs,
        // but content must never claim an offset newer than the captured record.
        let active = self.display_active_turn(session_id).await?;
        let latest = self.latest_event_id(session_id).await?;
        let through = active
            .as_ref()
            .map(|turn| TimelineEventId::from(turn.last_event_id.clone()).event_seq())
            .or_else(|| latest.as_ref().map(TimelineEventId::event_seq))
            .transpose()?
            .unwrap_or_default();
        let last_completed_event_seq = self
            .list_latest_terminal_events(session_id, Some(through.saturating_add(1)), 1)
            .await?
            .last()
            .map_or(0, |event| event.event_seq);
        let latest_event_id = latest.map(|id| id.to_string());
        let Some(active_turn) = active else {
            return Ok(SessionActiveTurnResponse {
                active_turn: None,
                last_completed_event_seq,
                latest_event_id,
            });
        };
        Ok(SessionActiveTurnResponse {
            active_turn: Some(self.read_turn_snapshot(active_turn).await?),
            last_completed_event_seq,
            latest_event_id,
        })
    }

    async fn read_turn_snapshot(
        &self,
        active_turn: TimelineActiveTurn,
    ) -> TalosResult<ActiveTurnSnapshot> {
        let through = TimelineEventId::from(active_turn.last_event_id.clone()).event_seq()?;
        let (checkpoint, mut pending_input) = self
            .turn_checkpoint_with_pending(&active_turn.identity, through)
            .await?;
        if active_turn.state != ActiveTurnState::WaitingForInput {
            pending_input = None;
        }
        let notices = self
            .turn_notices(std::slice::from_ref(&active_turn.identity), through)
            .await?;
        Ok(ActiveTurnSnapshot {
            active_turn,
            last_sequence: checkpoint.last_sequence,
            last_event_seq: through,
            messages: checkpoint.messages,
            metrics: checkpoint.metrics,
            pending_input,
            notices,
        })
    }
}

impl TimelineSubscriptionService {
    pub(crate) async fn read_active_turn(
        &self,
        session_id: &str,
    ) -> TalosResult<SessionActiveTurnResponse> {
        self.event_service().read_active_turn(session_id).await
    }
}

#[cfg(test)]
#[path = "active_turn_snapshot_tests.rs"]
mod tests;
