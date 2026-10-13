use super::subscription::TimelineSubscriptionService;
use super::TimelineEventId;
use talos_agent_api::{RecoverTimelineResponse, TimelineCursorStatus, TimelineReplay};
use talos_error::TalosResult;

impl TimelineSubscriptionService {
    pub async fn replay_after(
        &self,
        session_id: &str,
        after_event_id: Option<&TimelineEventId>,
        limit: usize,
    ) -> TalosResult<TimelineReplay> {
        let high_water_mark_event_id = self.latest_event_id(session_id).await?;
        self.replay_after_through(
            session_id,
            after_event_id,
            limit,
            high_water_mark_event_id.as_ref(),
        )
        .await
    }

    pub(super) async fn replay_after_through(
        &self,
        session_id: &str,
        after_event_id: Option<&TimelineEventId>,
        limit: usize,
        high_water_mark_event_id: Option<&TimelineEventId>,
    ) -> TalosResult<TimelineReplay> {
        let high_water_mark_event_seq = match high_water_mark_event_id {
            Some(event_id) => Some(event_id.event_seq()?),
            None => None,
        };
        let mut events = self
            .event_service()
            .list_events_after(session_id, after_event_id, limit.max(1))
            .await?;
        if let Some(high_water_mark_event_seq) = high_water_mark_event_seq {
            events.retain(|event| event.event_seq <= high_water_mark_event_seq);
        } else {
            events.clear();
        }
        let last_replay_seq = events
            .last()
            .map(|event| event.event_seq)
            .unwrap_or_else(|| {
                after_event_id
                    .and_then(|event_id| event_id.event_seq().ok())
                    .unwrap_or_default()
            });
        let next_cursor = events
            .last()
            .filter(|event| {
                high_water_mark_event_seq.is_some_and(|high_water| event.event_seq < high_water)
            })
            .map(|event| event.event_id.clone());
        Ok(TimelineReplay {
            events,
            high_water_mark_event_id: high_water_mark_event_id.map(ToString::to_string),
            next_cursor,
            last_event_seq: last_replay_seq,
            cursor_status: if after_event_id.is_some() {
                TimelineCursorStatus::Current
            } else {
                TimelineCursorStatus::FromBeginning
            },
        })
    }

    pub async fn recover_timeline(
        &self,
        session_id: &str,
        after_event_id: Option<&TimelineEventId>,
        limit: usize,
    ) -> TalosResult<RecoverTimelineResponse> {
        let replay = self.replay_after(session_id, after_event_id, limit).await?;
        let active_turn = self.event_service().display_active_turn(session_id).await?;
        Ok(RecoverTimelineResponse {
            replay,
            active_turn,
        })
    }
}
