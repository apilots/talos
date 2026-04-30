use std::sync::Arc;

use talos_error::{TalosError, TalosResult};
use talos_session_store::{
    ActiveTurnRecord, ActiveTurnStatus, SessionEventId, SessionEventRecord, SessionEventService,
};
use talos_types::talos::TalosEventPayload;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

pub type ApplicationEvent = TalosEventPayload;
pub type ApplicationEventStream = broadcast::Receiver<SessionEventRecord>;

#[derive(Clone, Default)]
pub struct EventService {
    inner: Option<Arc<SessionEventService>>,
}

impl EventService {
    pub fn new(inner: Arc<SessionEventService>) -> Self {
        Self { inner: Some(inner) }
    }

    pub async fn replay_after(
        &self,
        session_id: &str,
        after_event_id: Option<&SessionEventId>,
        limit: usize,
    ) -> TalosResult<EventReplay> {
        let inner = self.inner()?;
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
            .map(|event| event.event_seq)
            .unwrap_or_else(|| {
                after_event_id
                    .and_then(|event_id| event_id.event_seq().ok())
                    .unwrap_or_default()
            });

        Ok(EventReplay {
            events,
            last_replay_seq,
        })
    }

    pub fn subscribe_live_events(&self) -> TalosResult<ApplicationEventStream> {
        Ok(self.inner_talos()?.subscribe_live_events())
    }

    pub async fn get_active_turn(&self, session_id: &str) -> TalosResult<Option<ActiveTurnRecord>> {
        self.inner_talos()?.get_active_turn(session_id).await
    }

    pub async fn latest_event_id(&self, session_id: &str) -> TalosResult<Option<SessionEventId>> {
        self.inner_talos()?.latest_event_id(session_id).await
    }

    pub async fn has_live_turn(&self, session_id: &str) -> TalosResult<bool> {
        Ok(self
            .get_active_turn(session_id)
            .await?
            .is_some_and(|record| {
                matches!(
                    record.status,
                    ActiveTurnStatus::Running
                        | ActiveTurnStatus::WaitingUserAction
                        | ActiveTurnStatus::Cancelling
                )
            }))
    }

    pub async fn run_event_log_gc_loop(&self, shutdown: CancellationToken) {
        match self.inner_talos() {
            Ok(inner) => inner.run_event_log_gc_loop(shutdown).await,
            Err(err) => tracing::error!(error = %err, "event log gc loop could not start"),
        }
    }

    fn inner(&self) -> TalosResult<&Arc<SessionEventService>> {
        self.inner
            .as_ref()
            .ok_or_else(|| TalosError::InternalError("event service is not configured".to_string()))
    }

    fn inner_talos(&self) -> TalosResult<&Arc<SessionEventService>> {
        self.inner()
    }
}

#[derive(Debug)]
pub struct EventReplay {
    pub events: Vec<SessionEventRecord>,
    pub last_replay_seq: i64,
}

fn reject_expired_cursor(
    after_event_id: Option<&SessionEventId>,
    first_replay_event: Option<&SessionEventRecord>,
    oldest_event_id: Option<&SessionEventId>,
) -> TalosResult<()> {
    let Some(after_event_id) = after_event_id else {
        return Ok(());
    };
    let after_seq = after_event_id
        .event_seq()
        .map_err(|err| TalosError::ValidationError(err.to_string()))?;
    if let Some(oldest_event_id) = oldest_event_id {
        let oldest_seq = oldest_event_id
            .event_seq()
            .map_err(|err| TalosError::ValidationError(err.to_string()))?;
        if oldest_seq > after_seq + 1 {
            return Err(expired_cursor_error());
        }
    }
    if first_replay_event.is_some_and(|event| event.event_seq > after_seq + 1) {
        return Err(expired_cursor_error());
    }
    Ok(())
}

fn expired_cursor_error() -> TalosError {
    TalosError::ResourceExpired("event cursor is older than retained session events".to_string())
}

#[cfg(test)]
mod tests {
    use super::reject_expired_cursor;
    use talos_session_store::{SessionEventId, SessionEventRecord};
    use talos_types::talos::{
        TalosEventPayload, TalosEventType, TalosMessageStartedEvent, TalosUsage,
    };

    #[test]
    fn replay_cursor_rejects_retention_gap() {
        let first_replay_event = event_record(5);

        let err = reject_expired_cursor(
            Some(&SessionEventId::from_seq(1)),
            Some(&first_replay_event),
            Some(&SessionEventId::from_seq(5)),
        )
        .expect_err("cursor gap should be rejected");

        assert!(matches!(err, talos_error::TalosError::ResourceExpired(_)));
    }

    #[test]
    fn replay_cursor_rejects_empty_retention_gap() {
        let err = reject_expired_cursor(
            Some(&SessionEventId::from_seq(1)),
            None,
            Some(&SessionEventId::from_seq(5)),
        )
        .expect_err("cursor older than oldest retained event should be rejected");

        assert!(matches!(err, talos_error::TalosError::ResourceExpired(_)));
    }

    fn event_record(event_seq: i64) -> SessionEventRecord {
        SessionEventRecord {
            event_seq,
            event_id: SessionEventId::from_seq(event_seq),
            session_id: "s1".to_string(),
            turn_id: Some("turn-1".to_string()),
            message_id: "msg-1".to_string(),
            sequence: 0,
            event_type: TalosEventType::MessageStarted,
            payload: TalosEventPayload::MessageStarted(TalosMessageStartedEvent {
                request_id: "req-1".to_string(),
                model: "qwen-plus".to_string(),
                usage: TalosUsage::default(),
                iteration: 0,
                step_id: "start".to_string(),
            }),
            created_at: 1,
        }
    }
}
