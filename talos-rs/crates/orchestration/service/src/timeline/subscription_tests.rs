use super::super::{
    recovery::reject_expired_cursor,
    types::{
        TimelineCursorStatus, TimelineSubscribeCommand, TimelineSubscriptionCloseReason,
        TimelineSubscriptionMessage,
    },
};
use super::TimelineSubscriptionService;
use crate::{TimelineEventRetentionConfig, TimelineEventService};
use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use talos_store::session::{
    ActiveTurnRecord, ActiveTurnStore, ActiveTurnUpdate, TimelineEventId, TimelineEventStore,
};
use talos_types::talos::{
    AgentEvent, AgentEventPayload, AgentEventType, StopReason, TalosTurnCompletedEvent,
    TalosTurnMetricsEvent, TalosTurnStartedEvent, TimelineEventEnvelope, TokenUsage,
    TurnExecutionMetrics,
};
use tokio::sync::mpsc;

#[test]
fn replay_cursor_rejects_retention_gap() {
    let first_replay_event = event_record(5);

    let err = reject_expired_cursor(
        Some(&TimelineEventId::from_seq(1)),
        Some(&first_replay_event),
        Some(&TimelineEventId::from_seq(5)),
    )
    .expect_err("cursor gap should be rejected");

    assert!(err.is_code(talos_error::ErrorCode::ResourceExpired));
}

#[test]
fn replay_cursor_rejects_empty_retention_gap() {
    let err = reject_expired_cursor(
        Some(&TimelineEventId::from_seq(1)),
        None,
        Some(&TimelineEventId::from_seq(5)),
    )
    .expect_err("cursor older than oldest retained event should be rejected");

    assert!(err.is_code(talos_error::ErrorCode::ResourceExpired));
}

fn event_record(event_seq: i64) -> AgentEvent {
    AgentEvent {
        event_seq: Some(event_seq),
        event_id: Some(TimelineEventId::from_seq(event_seq).to_string()),
        created_at_ms: Some(1),
        event_type: AgentEventType::TurnStarted,
        turn_id: "turn-1".to_string(),
        message_id: "msg-1".to_string(),
        session_id: "s1".to_string(),
        sequence: 0,
        payload: AgentEventPayload::TurnStarted(TalosTurnStartedEvent {
            request_id: "req-1".to_string(),
            turn_id: "turn-1".to_string(),
            started_at_ms: 1,
        }),
    }
}

fn unpersisted_event(sequence: u64) -> AgentEvent {
    unpersisted_event_for_turn(sequence, "turn-1")
}

fn unpersisted_event_for_turn(sequence: u64, turn_id: &str) -> AgentEvent {
    AgentEvent {
        event_seq: None,
        event_id: None,
        created_at_ms: Some(100 + sequence as i64),
        event_type: AgentEventType::TurnStarted,
        turn_id: turn_id.to_string(),
        message_id: format!("msg-{turn_id}"),
        session_id: "s1".to_string(),
        sequence,
        payload: AgentEventPayload::TurnStarted(TalosTurnStartedEvent {
            request_id: "req-1".to_string(),
            turn_id: turn_id.to_string(),
            started_at_ms: 1,
        }),
    }
}

fn terminal_event(sequence: u64) -> AgentEvent {
    AgentEvent {
        event_seq: None,
        event_id: None,
        created_at_ms: Some(100 + sequence as i64),
        event_type: AgentEventType::TurnCompleted,
        turn_id: "turn-1".to_string(),
        message_id: "msg-1".to_string(),
        session_id: "s1".to_string(),
        sequence,
        payload: AgentEventPayload::TurnCompleted(TalosTurnCompletedEvent {
            request_id: "req-1".to_string(),
            turn_id: "turn-1".to_string(),
            completed_at_ms: 1,
            stop_reason: StopReason::EndTurn,
            usage: TokenUsage::default(),
            metrics: None,
            artifact_seq: None,
        }),
    }
}

fn metrics_event(sequence: u64) -> AgentEvent {
    AgentEvent {
        event_seq: None,
        event_id: None,
        created_at_ms: Some(100 + sequence as i64),
        event_type: AgentEventType::TurnMetrics,
        turn_id: "turn-1".to_string(),
        message_id: "msg-1".to_string(),
        session_id: "s1".to_string(),
        sequence,
        payload: AgentEventPayload::TurnMetrics(TalosTurnMetricsEvent {
            request_id: "req-1".to_string(),
            turn_id: "turn-1".to_string(),
            metrics: TurnExecutionMetrics::default(),
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
    let events = TimelineSubscriptionService::new(Arc::new(timeline.clone()));
    let first = timeline
        .record_talos_event(unpersisted_event(1))
        .await
        .unwrap();
    let first_event_id = first.event_id.clone();

    let mut subscription = events
        .subscribe(TimelineSubscribeCommand {
            session_id: "s1".to_string(),
            turn_id: None,
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
    let replay = recv_event(&mut subscription.events).await;
    let live = recv_event(&mut subscription.events).await;
    assert_eq!(replay.event_id, first_event_id);
    assert_eq!(live.event_id, second.event_id);
    assert!(live.event_seq > subscription.replay.last_replay_seq);
}

#[tokio::test]
async fn live_metrics_events_are_not_replayed_or_used_as_cursor_progress() {
    let (timeline, _store) = service();
    let events = TimelineSubscriptionService::new(Arc::new(timeline.clone()));
    let first = timeline
        .record_talos_event(unpersisted_event(1))
        .await
        .unwrap();

    let mut subscription = events
        .subscribe(TimelineSubscribeCommand {
            session_id: "s1".to_string(),
            turn_id: None,
            after_event_id: None,
            replay_limit: 32,
            close_when_idle: false,
            close_on_terminal: false,
        })
        .await
        .unwrap();

    timeline.record_talos_event(metrics_event(2)).await.unwrap();
    let second = timeline
        .record_talos_event(unpersisted_event(3))
        .await
        .unwrap();

    let replay = recv_event(&mut subscription.events).await;
    let metrics = recv_event(&mut subscription.events).await;
    let live = recv_event(&mut subscription.events).await;

    assert_eq!(replay.event_id, first.event_id);
    assert!(matches!(
        metrics.event.payload,
        AgentEventPayload::TurnMetrics(_)
    ));
    assert_eq!(live.event_id, second.event_id);

    let recovered = events.recover_timeline("s1", None, 32).await.unwrap();
    assert_eq!(recovered.replay.events.len(), 2);
    assert!(recovered
        .replay
        .events
        .iter()
        .all(|event| !matches!(event.event.payload, AgentEventPayload::TurnMetrics(_))));
}

#[tokio::test]
async fn turn_scoped_subscription_filters_replay_and_live_events() {
    let (timeline, _store) = service();
    let events = TimelineSubscriptionService::new(Arc::new(timeline.clone()));
    timeline
        .record_talos_event(unpersisted_event_for_turn(1, "turn-old"))
        .await
        .unwrap();
    let replay_match = timeline
        .record_talos_event(unpersisted_event_for_turn(2, "turn-current"))
        .await
        .unwrap();

    let mut subscription = events
        .subscribe(TimelineSubscribeCommand {
            session_id: "s1".to_string(),
            turn_id: Some("turn-current".to_string()),
            after_event_id: None,
            replay_limit: 32,
            close_when_idle: false,
            close_on_terminal: false,
        })
        .await
        .unwrap();

    assert_eq!(subscription.replay.events.len(), 1);
    assert_eq!(
        subscription.replay.events[0].event_id,
        replay_match.event_id
    );

    timeline
        .record_talos_event(unpersisted_event_for_turn(3, "turn-other"))
        .await
        .unwrap();
    let live_match = timeline
        .record_talos_event(unpersisted_event_for_turn(4, "turn-current"))
        .await
        .unwrap();

    let replay = recv_event(&mut subscription.events).await;
    let live = recv_event(&mut subscription.events).await;
    assert_eq!(replay.event_id, replay_match.event_id);
    assert_eq!(live.event_id, live_match.event_id);
}

#[tokio::test]
async fn session_scoped_subscription_continues_after_terminal_event() {
    let (timeline, _store) = service();
    let events = TimelineSubscriptionService::new(Arc::new(timeline.clone()));

    let mut subscription = events
        .subscribe(TimelineSubscribeCommand {
            session_id: "s1".to_string(),
            turn_id: None,
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

    let first = recv_event(&mut subscription.events).await;
    let second = recv_event(&mut subscription.events).await;
    assert_eq!(first.event_id, terminal.event_id);
    assert_eq!(second.event_id, next_turn.event_id);
}

#[tokio::test]
async fn close_on_terminal_closes_after_replayed_terminal_event() {
    let (timeline, _store) = service();
    let events = TimelineSubscriptionService::new(Arc::new(timeline.clone()));
    let terminal = timeline
        .record_talos_event(terminal_event(1))
        .await
        .unwrap();

    let mut subscription = events
        .subscribe(TimelineSubscribeCommand {
            session_id: "s1".to_string(),
            turn_id: Some("turn-1".to_string()),
            after_event_id: None,
            replay_limit: 32,
            close_when_idle: false,
            close_on_terminal: true,
        })
        .await
        .unwrap();

    let replay = recv_event(&mut subscription.events).await;
    assert_eq!(replay.event_id, terminal.event_id);
    let message = subscription.events.recv().await.expect("close message");
    let TimelineSubscriptionMessage::Closed(closed) = message else {
        panic!("expected subscription close message");
    };
    assert_eq!(closed.reason, TimelineSubscriptionCloseReason::Completed);
}

#[tokio::test]
async fn lagged_subscription_closes_without_sending_later_events() {
    let (timeline, _store) = service();
    let events = TimelineSubscriptionService::new(Arc::new(timeline.clone()));

    let mut subscription = events
        .subscribe(TimelineSubscribeCommand {
            session_id: "s1".to_string(),
            turn_id: None,
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
            Ok(Some(TimelineSubscriptionMessage::Event(event))) => received.push(event.event_seq),
            Ok(Some(TimelineSubscriptionMessage::Closed(closed))) => {
                assert_eq!(closed.reason, TimelineSubscriptionCloseReason::Lagged);
                break;
            }
            Ok(None) => break,
            Err(_) => panic!("lagged subscription did not close"),
        }
    }

    if !received.is_empty() {
        assert_eq!(received, (1..=received.len() as i64).collect::<Vec<_>>());
    }
}

#[tokio::test]
async fn idle_scoped_subscription_reports_completed_close() {
    let (timeline, _store) = service();
    let events = TimelineSubscriptionService::new(Arc::new(timeline.clone()));

    let mut subscription = events
        .subscribe(TimelineSubscribeCommand {
            session_id: "s1".to_string(),
            turn_id: None,
            after_event_id: None,
            replay_limit: 32,
            close_when_idle: true,
            close_on_terminal: false,
        })
        .await
        .unwrap();

    let message = subscription.events.recv().await.expect("close message");
    let TimelineSubscriptionMessage::Closed(closed) = message else {
        panic!("expected subscription close message");
    };
    assert_eq!(closed.reason, TimelineSubscriptionCloseReason::Completed);
}

async fn recv_event(
    events: &mut mpsc::Receiver<TimelineSubscriptionMessage>,
) -> TimelineEventEnvelope {
    match events.recv().await.expect("subscription message") {
        TimelineSubscriptionMessage::Event(event) => event,
        TimelineSubscriptionMessage::Closed(closed) => {
            panic!("unexpected subscription close: {closed:?}")
        }
    }
}

#[tokio::test]
async fn recover_timeline_returns_active_turn_snapshot() {
    let (timeline, _store) = service();
    let events = TimelineSubscriptionService::new(Arc::new(timeline.clone()));
    timeline
        .record_talos_event(unpersisted_event(1))
        .await
        .unwrap();

    let snapshot = events.recover_timeline("s1", None, 32).await.unwrap();

    assert_eq!(snapshot.replay.events.len(), 1);
    assert_eq!(snapshot.active_turn.session_id, "s1");
    assert_eq!(snapshot.active_turn.turn_id.as_deref(), Some("turn-1"));
}

#[derive(Default)]
struct MockStore {
    events: Mutex<Vec<AgentEvent>>,
    active_turn: Mutex<Option<ActiveTurnRecord>>,
}

#[async_trait]
impl TimelineEventStore for MockStore {
    async fn append_event(&self, mut event: AgentEvent) -> talos_error::TalosResult<AgentEvent> {
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
    ) -> talos_error::TalosResult<Vec<AgentEvent>> {
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
                event.session_id == session_id && event.event_seq.unwrap_or_default() > after_seq
            })
            .take(limit)
            .cloned()
            .collect())
    }

    async fn list_turn_events(
        &self,
        session_id: &str,
        turn_id: &str,
        limit: usize,
    ) -> talos_error::TalosResult<Vec<AgentEvent>> {
        let mut events = self
            .events
            .lock()
            .expect("events lock")
            .iter()
            .rev()
            .filter(|event| event.session_id == session_id && event.turn_id == turn_id)
            .take(limit.max(1))
            .cloned()
            .collect::<Vec<_>>();
        events.reverse();
        Ok(events)
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
    async fn upsert_active_turn(&self, update: ActiveTurnUpdate) -> talos_error::TalosResult<()> {
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
