use talos_error::{TalosError, TalosResult};
use talos_store::session::TimelineEventId;
use talos_types::talos::{AgentEvent, TimelineEventEnvelope};

use super::mapping::map_active_turn_snapshot;
use super::subscription::TimelineSubscriptionService;
use super::types::{EventReplay, TimelineCursorStatus, TimelineRecoveryState};

impl TimelineSubscriptionService {
    pub async fn replay_after(
        &self,
        session_id: &str,
        after_event_id: Option<&TimelineEventId>,
        limit: usize,
    ) -> TalosResult<EventReplay> {
        let inner = self.inner_talos()?;
        let high_water_mark_event_id = inner.latest_event_id(session_id).await?;
        let events = inner
            .list_events_after(session_id, after_event_id, limit.max(1))
            .await?;
        let oldest_event_id = if after_event_id.is_some() {
            inner.oldest_event_id(session_id).await?
        } else {
            None
        };
        reject_expired_cursor(after_event_id, events.first(), oldest_event_id.as_ref())?;
        let last_replay_seq = events
            .last()
            .and_then(|event| event.event_seq)
            .unwrap_or_else(|| {
                after_event_id
                    .and_then(|event_id| event_id.event_seq().ok())
                    .unwrap_or_default()
            });
        let events = events
            .into_iter()
            .map(|event| {
                TimelineEventEnvelope::from_persisted_event(event).ok_or_else(|| {
                    TalosError::internal(
                        "persisted timeline event is missing envelope metadata".to_string(),
                    )
                })
            })
            .collect::<TalosResult<Vec<_>>>()?;

        Ok(EventReplay {
            events,
            high_water_mark_event_id,
            last_replay_seq,
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
    ) -> TalosResult<TimelineRecoveryState> {
        let replay = self.replay_after(session_id, after_event_id, limit).await?;
        let active_turn = self.get_active_turn(session_id).await?;
        Ok(TimelineRecoveryState {
            replay,
            active_turn: map_active_turn_snapshot(session_id, active_turn),
        })
    }
}

pub(crate) fn reject_expired_cursor(
    after_event_id: Option<&TimelineEventId>,
    first_replay_event: Option<&AgentEvent>,
    oldest_event_id: Option<&TimelineEventId>,
) -> TalosResult<()> {
    let Some(after_event_id) = after_event_id else {
        return Ok(());
    };
    let after_seq = after_event_id.event_seq().map_err(|err| {
        TalosError::public(talos_error::ErrorCode::InvalidRequest, err.to_string())
    })?;
    if let Some(oldest_event_id) = oldest_event_id {
        let oldest_seq = oldest_event_id.event_seq().map_err(|err| {
            TalosError::public(talos_error::ErrorCode::InvalidRequest, err.to_string())
        })?;
        if oldest_seq > after_seq + 1 {
            return Err(expired_cursor_error());
        }
    }
    if first_replay_event.is_some_and(|event| event.event_seq.unwrap_or_default() > after_seq + 1) {
        return Err(expired_cursor_error());
    }
    Ok(())
}

fn expired_cursor_error() -> TalosError {
    TalosError::public(
        talos_error::ErrorCode::ResourceExpired,
        "event cursor is older than retained timeline events".to_string(),
    )
}
