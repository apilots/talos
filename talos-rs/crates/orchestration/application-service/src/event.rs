use std::sync::Arc;

use talos_error::{TalosError, TalosResult};
use talos_session_store::{
    ActiveTurnRecord, ActiveTurnStatus, TimelineEventId, TimelineEventService,
};
use talos_types::talos::{TalosTimelineEvent, TimelineEventEnvelope};
use tokio::sync::{broadcast, mpsc};
use tokio_util::sync::CancellationToken;

pub type ApplicationEvent = TimelineEventEnvelope;
pub type ApplicationEventStream = broadcast::Receiver<TimelineEventEnvelope>;

#[derive(Debug, Clone)]
pub struct TimelineSubscribeParams {
    pub session_id: String,
    pub after_event_id: Option<TimelineEventId>,
    pub replay_limit: usize,
    pub close_when_idle: bool,
    pub close_on_terminal: bool,
}

pub struct TimelineSubscription {
    pub session_id: String,
    pub high_water_mark_event_id: Option<TimelineEventId>,
    pub replay: EventReplay,
    pub events: mpsc::Receiver<TimelineEventEnvelope>,
    pub has_live_turn: bool,
}

#[derive(Debug)]
pub struct TimelineRecoverySnapshot {
    pub replay: EventReplay,
    pub active_turn: Option<ActiveTurnRecord>,
}

#[derive(Clone, Default)]
pub struct EventService {
    inner: Option<Arc<TimelineEventService>>,
}

impl EventService {
    pub fn new(inner: Arc<TimelineEventService>) -> Self {
        Self { inner: Some(inner) }
    }

    pub async fn replay_after(
        &self,
        session_id: &str,
        after_event_id: Option<&TimelineEventId>,
        limit: usize,
    ) -> TalosResult<EventReplay> {
        let inner = self.inner()?;
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
                    TalosError::SerializationError(
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

    pub async fn recover_session(
        &self,
        session_id: &str,
        after_event_id: Option<&TimelineEventId>,
        limit: usize,
    ) -> TalosResult<TimelineRecoverySnapshot> {
        let replay = self.replay_after(session_id, after_event_id, limit).await?;
        let active_turn = self.get_active_turn(session_id).await?;
        Ok(TimelineRecoverySnapshot {
            replay,
            active_turn,
        })
    }

    pub async fn subscribe(
        &self,
        params: TimelineSubscribeParams,
    ) -> TalosResult<TimelineSubscription> {
        let live_events = self.subscribe_live_events()?;
        let high_water_mark_event_id = self.latest_event_id(&params.session_id).await?;
        let replay = self
            .replay_after(
                &params.session_id,
                params.after_event_id.as_ref(),
                params.replay_limit,
            )
            .await?;
        let has_live_turn = self.has_live_turn(&params.session_id).await?;
        let events = spawn_subscription_stream(
            params.session_id.clone(),
            replay.events.clone(),
            replay.last_replay_seq,
            live_events,
            has_live_turn,
            params.close_when_idle,
            params.close_on_terminal,
        );
        Ok(TimelineSubscription {
            session_id: params.session_id,
            high_water_mark_event_id,
            replay,
            events,
            has_live_turn,
        })
    }

    pub fn subscribe_live_events(&self) -> TalosResult<ApplicationEventStream> {
        Ok(self.inner_talos()?.subscribe_live_events())
    }

    pub async fn get_active_turn(&self, session_id: &str) -> TalosResult<Option<ActiveTurnRecord>> {
        self.inner_talos()?.get_active_turn(session_id).await
    }

    pub async fn latest_event_id(&self, session_id: &str) -> TalosResult<Option<TimelineEventId>> {
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

    fn inner(&self) -> TalosResult<&Arc<TimelineEventService>> {
        self.inner
            .as_ref()
            .ok_or_else(|| TalosError::InternalError("event service is not configured".to_string()))
    }

    fn inner_talos(&self) -> TalosResult<&Arc<TimelineEventService>> {
        self.inner()
    }
}

#[derive(Debug)]
pub struct EventReplay {
    pub events: Vec<TimelineEventEnvelope>,
    pub high_water_mark_event_id: Option<TimelineEventId>,
    pub last_replay_seq: i64,
    pub cursor_status: TimelineCursorStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimelineCursorStatus {
    Current,
    FromBeginning,
}

fn spawn_subscription_stream(
    session_id: String,
    replay_events: Vec<TimelineEventEnvelope>,
    last_replay_seq: i64,
    mut live_events: ApplicationEventStream,
    has_live_turn: bool,
    close_when_idle: bool,
    close_on_terminal: bool,
) -> mpsc::Receiver<TimelineEventEnvelope> {
    let (tx, rx) = mpsc::channel(256);
    tokio::spawn(async move {
        let mut last_sent_seq = last_replay_seq;
        for record in replay_events {
            last_sent_seq = record.event_seq;
            if tx.send(record).await.is_err() {
                return;
            }
        }

        if close_when_idle && !has_live_turn {
            return;
        }

        loop {
            match live_events.recv().await {
                Ok(record) => {
                    let event_seq = record.event_seq;
                    if record.session_id != session_id || event_seq <= last_sent_seq {
                        continue;
                    }
                    last_sent_seq = event_seq;
                    let is_terminal = is_terminal_event(&record);
                    if tx.send(record).await.is_err() {
                        return;
                    }
                    if close_on_terminal && is_terminal {
                        return;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    tracing::warn!(
                        skipped,
                        "timeline subscription lagged; closing stream for explicit recovery"
                    );
                    return;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            }
        }
    });
    rx
}

fn is_terminal_event(record: &TimelineEventEnvelope) -> bool {
    matches!(
        record.event.payload,
        talos_types::talos::TalosTimelineEventPayload::TurnCompleted(_)
            | talos_types::talos::TalosTimelineEventPayload::TurnError(_)
    )
}

fn reject_expired_cursor(
    after_event_id: Option<&TimelineEventId>,
    first_replay_event: Option<&TalosTimelineEvent>,
    oldest_event_id: Option<&TimelineEventId>,
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
    if first_replay_event.is_some_and(|event| event.event_seq.unwrap_or_default() > after_seq + 1) {
        return Err(expired_cursor_error());
    }
    Ok(())
}

fn expired_cursor_error() -> TalosError {
    TalosError::ResourceExpired("event cursor is older than retained timeline events".to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        reject_expired_cursor, EventService, TimelineCursorStatus, TimelineSubscribeParams,
    };
    use async_trait::async_trait;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use talos_session_store::{
        ActiveTurnRecord, ActiveTurnStore, ActiveTurnUpdate, TimelineEventId,
        TimelineEventRetentionConfig, TimelineEventService, TimelineEventStore,
    };
    use talos_types::talos::{
        StopReason, TalosTimelineEvent, TalosTimelineEventPayload, TalosTimelineEventType,
        TalosTurnCompletedEvent, TalosTurnStartedEvent, TalosUsage,
    };

    #[test]
    fn replay_cursor_rejects_retention_gap() {
        let first_replay_event = event_record(5);

        let err = reject_expired_cursor(
            Some(&TimelineEventId::from_seq(1)),
            Some(&first_replay_event),
            Some(&TimelineEventId::from_seq(5)),
        )
        .expect_err("cursor gap should be rejected");

        assert!(matches!(err, talos_error::TalosError::ResourceExpired(_)));
    }

    #[test]
    fn replay_cursor_rejects_empty_retention_gap() {
        let err = reject_expired_cursor(
            Some(&TimelineEventId::from_seq(1)),
            None,
            Some(&TimelineEventId::from_seq(5)),
        )
        .expect_err("cursor older than oldest retained event should be rejected");

        assert!(matches!(err, talos_error::TalosError::ResourceExpired(_)));
    }

    fn event_record(event_seq: i64) -> TalosTimelineEvent {
        TalosTimelineEvent {
            event_seq: Some(event_seq),
            event_id: Some(TimelineEventId::from_seq(event_seq).to_string()),
            created_at_ms: Some(1),
            event_type: TalosTimelineEventType::TurnStarted,
            turn_id: "turn-1".to_string(),
            message_id: "msg-1".to_string(),
            session_id: "s1".to_string(),
            sequence: 0,
            payload: TalosTimelineEventPayload::TurnStarted(TalosTurnStartedEvent {
                request_id: "req-1".to_string(),
                turn_id: "turn-1".to_string(),
                started_at_ms: 1,
            }),
        }
    }

    fn unpersisted_event(sequence: u64) -> TalosTimelineEvent {
        TalosTimelineEvent {
            event_seq: None,
            event_id: None,
            created_at_ms: Some(100 + sequence as i64),
            event_type: TalosTimelineEventType::TurnStarted,
            turn_id: "turn-1".to_string(),
            message_id: "msg-1".to_string(),
            session_id: "s1".to_string(),
            sequence,
            payload: TalosTimelineEventPayload::TurnStarted(TalosTurnStartedEvent {
                request_id: "req-1".to_string(),
                turn_id: "turn-1".to_string(),
                started_at_ms: 1,
            }),
        }
    }

    fn terminal_event(sequence: u64) -> TalosTimelineEvent {
        TalosTimelineEvent {
            event_seq: None,
            event_id: None,
            created_at_ms: Some(100 + sequence as i64),
            event_type: TalosTimelineEventType::TurnCompleted,
            turn_id: "turn-1".to_string(),
            message_id: "msg-1".to_string(),
            session_id: "s1".to_string(),
            sequence,
            payload: TalosTimelineEventPayload::TurnCompleted(TalosTurnCompletedEvent {
                request_id: "req-1".to_string(),
                turn_id: "turn-1".to_string(),
                completed_at_ms: 1,
                stop_reason: StopReason::EndTurn,
                usage: TalosUsage::default(),
            }),
        }
    }

    fn service() -> (TimelineEventService, Arc<MockStore>) {
        let store = Arc::new(MockStore::default());
        let event_store: Arc<dyn TimelineEventStore> = store.clone();
        let active_turn_store: Arc<dyn ActiveTurnStore> = store.clone();
        (
            TimelineEventService::new(
                event_store,
                active_turn_store,
                TimelineEventRetentionConfig::default(),
            ),
            store,
        )
    }

    #[tokio::test]
    async fn subscribe_returns_replay_and_live_envelopes() {
        let (timeline, _store) = service();
        let events = EventService::new(Arc::new(timeline.clone()));
        let first = timeline
            .record_talos_event(unpersisted_event(1))
            .await
            .unwrap();
        let first_event_id = first.event_id.clone();

        let mut subscription = events
            .subscribe(TimelineSubscribeParams {
                session_id: "s1".to_string(),
                after_event_id: None,
                replay_limit: 32,
                close_when_idle: false,
                close_on_terminal: false,
            })
            .await
            .unwrap();

        assert_eq!(subscription.replay.events.len(), 1);
        assert_eq!(
            subscription.replay.cursor_status,
            TimelineCursorStatus::FromBeginning
        );
        assert_eq!(
            subscription.replay.high_water_mark_event_id,
            Some(TimelineEventId::from_seq(1))
        );
        assert_eq!(subscription.replay.events[0].event_id, first_event_id);

        let second = timeline
            .record_talos_event(unpersisted_event(2))
            .await
            .unwrap();
        let replay = subscription.events.recv().await.unwrap();
        let live = subscription.events.recv().await.unwrap();
        assert_eq!(replay.event_id, first_event_id);
        assert_eq!(live.event_id, second.event_id);
        assert!(live.event_seq > subscription.replay.last_replay_seq);
    }

    #[tokio::test]
    async fn session_scoped_subscription_continues_after_terminal_event() {
        let (timeline, _store) = service();
        let events = EventService::new(Arc::new(timeline.clone()));

        let mut subscription = events
            .subscribe(TimelineSubscribeParams {
                session_id: "s1".to_string(),
                after_event_id: None,
                replay_limit: 32,
                close_when_idle: false,
                close_on_terminal: false,
            })
            .await
            .unwrap();

        let terminal = timeline
            .record_talos_event(terminal_event(1))
            .await
            .unwrap();
        let next_turn = timeline
            .record_talos_event(unpersisted_event(2))
            .await
            .unwrap();

        let first = subscription.events.recv().await.unwrap();
        let second = subscription.events.recv().await.unwrap();
        assert_eq!(first.event_id, terminal.event_id);
        assert_eq!(second.event_id, next_turn.event_id);
    }

    #[tokio::test]
    async fn lagged_subscription_closes_without_sending_later_events() {
        let (timeline, _store) = service();
        let events = EventService::new(Arc::new(timeline.clone()));

        let mut subscription = events
            .subscribe(TimelineSubscribeParams {
                session_id: "s1".to_string(),
                after_event_id: None,
                replay_limit: 32,
                close_when_idle: false,
                close_on_terminal: false,
            })
            .await
            .unwrap();

        for sequence in 1..=1300 {
            timeline
                .record_talos_event(unpersisted_event(sequence))
                .await
                .unwrap();
        }

        let mut received = Vec::new();
        loop {
            match tokio::time::timeout(Duration::from_secs(1), subscription.events.recv()).await {
                Ok(Some(event)) => received.push(event.event_seq),
                Ok(None) => break,
                Err(_) => panic!("lagged subscription did not close"),
            }
        }

        if !received.is_empty() {
            assert_eq!(received, (1..=received.len() as i64).collect::<Vec<_>>());
        }
    }

    #[tokio::test]
    async fn recover_session_returns_active_turn_snapshot() {
        let (timeline, _store) = service();
        let events = EventService::new(Arc::new(timeline.clone()));
        timeline
            .record_talos_event(unpersisted_event(1))
            .await
            .unwrap();

        let snapshot = events.recover_session("s1", None, 32).await.unwrap();

        assert_eq!(snapshot.replay.events.len(), 1);
        let active = snapshot.active_turn.expect("active turn should exist");
        assert_eq!(active.session_id, "s1");
        assert_eq!(active.turn_id, "turn-1");
    }

    #[derive(Default)]
    struct MockStore {
        events: Mutex<Vec<TalosTimelineEvent>>,
        active_turn: Mutex<Option<ActiveTurnRecord>>,
    }

    #[async_trait]
    impl TimelineEventStore for MockStore {
        async fn append_event(
            &self,
            mut event: TalosTimelineEvent,
        ) -> talos_error::TalosResult<TalosTimelineEvent> {
            let mut events = self.events.lock().expect("events lock");
            let event_seq = events.len() as i64 + 1;
            event.event_seq = Some(event_seq);
            event.event_id = Some(TimelineEventId::from_seq(event_seq).to_string());
            event.created_at_ms.get_or_insert(1000 + event_seq);
            events.push(event.clone());
            Ok(event)
        }

        async fn list_events_after(
            &self,
            session_id: &str,
            after_event_id: Option<&TimelineEventId>,
            limit: usize,
        ) -> talos_error::TalosResult<Vec<TalosTimelineEvent>> {
            let after_seq = after_event_id
                .map(TimelineEventId::event_seq)
                .transpose()?
                .unwrap_or_default();
            Ok(self
                .events
                .lock()
                .expect("events lock")
                .iter()
                .filter(|event| {
                    event.session_id == session_id
                        && event.event_seq.unwrap_or_default() > after_seq
                })
                .take(limit)
                .cloned()
                .collect())
        }

        async fn latest_event_id(
            &self,
            session_id: &str,
        ) -> talos_error::TalosResult<Option<TimelineEventId>> {
            Ok(self
                .events
                .lock()
                .expect("events lock")
                .iter()
                .rev()
                .find(|event| event.session_id == session_id)
                .and_then(|event| event.event_seq)
                .map(TimelineEventId::from_seq))
        }

        async fn oldest_event_id(
            &self,
            session_id: &str,
        ) -> talos_error::TalosResult<Option<TimelineEventId>> {
            Ok(self
                .events
                .lock()
                .expect("events lock")
                .iter()
                .find(|event| event.session_id == session_id)
                .and_then(|event| event.event_seq)
                .map(TimelineEventId::from_seq))
        }

        async fn delete_completed_events_before(
            &self,
            _before_timestamp_ms: i64,
            _limit: usize,
        ) -> talos_error::TalosResult<u64> {
            Ok(0)
        }

        async fn delete_overflow_events_per_session(
            &self,
            _max_events_per_session: usize,
            _limit: usize,
        ) -> talos_error::TalosResult<u64> {
            Ok(0)
        }
    }

    #[async_trait]
    impl ActiveTurnStore for MockStore {
        async fn upsert_active_turn(
            &self,
            update: ActiveTurnUpdate,
        ) -> talos_error::TalosResult<()> {
            *self.active_turn.lock().expect("active turn lock") = Some(ActiveTurnRecord {
                session_id: update.session_id,
                turn_id: update.turn_id,
                message_id: update.message_id,
                status: update.status,
                last_event_id: update.last_event_id,
                pending_user_action: update.pending_user_action,
                updated_at: update.updated_at,
            });
            Ok(())
        }

        async fn get_active_turn(
            &self,
            session_id: &str,
        ) -> talos_error::TalosResult<Option<ActiveTurnRecord>> {
            Ok(self
                .active_turn
                .lock()
                .expect("active turn lock")
                .clone()
                .filter(|record| record.session_id == session_id))
        }

        async fn clear_active_turn(&self, session_id: &str) -> talos_error::TalosResult<()> {
            let mut active = self.active_turn.lock().expect("active turn lock");
            if active
                .as_ref()
                .is_some_and(|record| record.session_id == session_id)
            {
                *active = None;
            }
            Ok(())
        }
    }
}
