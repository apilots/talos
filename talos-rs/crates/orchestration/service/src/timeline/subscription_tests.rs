use std::sync::Arc;
use std::time::Duration;

use talos_agent_api::{
    AgentEvent, AgentEventPayload, AgentMessage, AgentMessageRole, AgentMessageStatus,
    AgentStopReason, AgentTurnCompleted, AgentTurnRef, TimelineCursorStatus,
};
use talos_storage::SqliteStorage;
use talos_store::session::{
    AgentExecutionStore, CommitTerminalSettlementParams, DatabaseSessionStore,
};

use super::TimelineSubscriptionService;
use crate::test_support::{commit_test_running_turn, run_test_migrations};
use crate::timeline::TimelineEventId;
use crate::{
    TimelineEventService, TimelineSubscribeCommand, TimelineSubscriptionCloseReason,
    TimelineSubscriptionMessage,
};

async fn fixture() -> (
    Arc<DatabaseSessionStore>,
    Arc<TimelineEventService>,
    TimelineSubscriptionService,
) {
    let storage = Arc::new(
        SqliteStorage::new_with_max_connections(":memory:", 1)
            .await
            .expect("test storage should open"),
    );
    run_test_migrations(storage.pool()).await;
    let store = Arc::new(
        DatabaseSessionStore::new(storage)
            .await
            .expect("Session Store should initialize"),
    );
    let event_service = Arc::new(TimelineEventService::new(store.clone()));
    let subscription_service = TimelineSubscriptionService::new(event_service.clone());
    (store, event_service, subscription_service)
}

fn user_message(identity: &AgentTurnRef) -> AgentMessage {
    AgentMessage {
        message_id: "message-1".to_string(),
        turn_id: identity.turn_id.clone(),
        role: AgentMessageRole::User,
        status: AgentMessageStatus::Completed,
        blocks: Vec::new(),
        created_at: 10,
        completed_at: Some(10),
    }
}

#[tokio::test]
async fn live_subscription_recovers_a_notice_published_after_a_newer_agent_fact() {
    let (store, events, subscriptions) = fixture().await;
    let identity = AgentTurnRef::new("session-1", "turn-1");
    let commit =
        commit_test_running_turn(store.as_ref(), identity.clone(), user_message(&identity)).await;
    let mut subscription = subscriptions
        .subscribe(TimelineSubscribeCommand {
            session_id: identity.session_id.clone(),
            turn_id: None,
            after_event_id: Some(TimelineEventId::from_seq(commit.active_turn.last_event_seq)),
            replay_limit: 1,
            close_when_idle: false,
            close_on_terminal: false,
        })
        .await
        .unwrap();
    let notice = store
        .append_timeline_notice(talos_agent_api::TimelineSystemNotice {
            identity: identity.clone(),
            occurred_at: 11,
            notice: talos_agent_api::AgentSystemNotice {
                level: talos_types::TalosTimelineNoticeLevel::Warning,
                content: talos_agent_api::SystemNoticeContent::Text(
                    "Usage is delayed.".to_string(),
                ),
            },
        })
        .await
        .unwrap();
    let progress = store
        .commit_progress_event(
            AgentEvent {
                identity,
                sequence: commit.events.last().unwrap().sequence + 1,
                occurred_at: 12,
                payload: AgentEventPayload::MetricsUpdated(Default::default()),
            },
            commit.active_turn.last_event_seq,
        )
        .await
        .unwrap();
    let expected = vec![notice.event_seq, progress.active_turn.last_event_seq];
    events.publish_committed(progress.events);
    let mut received = Vec::new();
    for _ in 0..2 {
        let message = tokio::time::timeout(Duration::from_secs(2), subscription.events.recv())
            .await
            .unwrap()
            .unwrap();
        let TimelineSubscriptionMessage::Event(event) = message else {
            panic!("expected durable fact");
        };
        received.push(event.event_seq);
    }
    assert_eq!(received, expected);
}

#[tokio::test]
async fn replay_returns_canonical_events_and_an_active_turn_projection() {
    let (store, _events, subscriptions) = fixture().await;
    let identity = AgentTurnRef::new("session-1", "turn-1");
    let admission =
        commit_test_running_turn(store.as_ref(), identity.clone(), user_message(&identity)).await;

    let recovered = subscriptions
        .recover_timeline("session-1", None, 10)
        .await
        .expect("Timeline should recover");

    assert_eq!(recovered.replay.events.len(), admission.events.len());
    assert_eq!(
        recovered.replay.cursor_status,
        TimelineCursorStatus::FromBeginning
    );
    let active = recovered
        .active_turn
        .expect("admitted Turn should remain active");
    assert_eq!(active.identity, identity);
    assert_eq!(
        active.last_event_id,
        TimelineEventId::from_seq(admission.active_turn.last_event_seq).to_string()
    );
}

#[tokio::test]
async fn recovery_exposes_the_next_cursor_until_the_captured_high_water_mark() {
    let (store, _events, subscriptions) = fixture().await;
    let identity = AgentTurnRef::new("session-1", "turn-1");
    let admission =
        commit_test_running_turn(store.as_ref(), identity.clone(), user_message(&identity)).await;

    let first = subscriptions
        .recover_timeline("session-1", None, 1)
        .await
        .expect("first Timeline page should recover");
    let first_cursor = first
        .replay
        .next_cursor
        .clone()
        .expect("truncated recovery should expose its next cursor");
    assert_eq!(first.replay.events.len(), 1);
    assert_eq!(
        first.replay.high_water_mark_event_id,
        admission
            .events
            .last()
            .map(|event| TimelineEventId::from_seq(event.event_seq).to_string())
    );
    assert_ne!(
        first.replay.high_water_mark_event_id.as_deref(),
        Some(first_cursor.as_str())
    );

    let second = subscriptions
        .recover_timeline("session-1", Some(&TimelineEventId::from(first_cursor)), 10)
        .await
        .expect("remaining Timeline page should recover");
    assert_eq!(second.replay.events.len(), admission.events.len() - 1);
    assert_eq!(second.replay.next_cursor, None);
    assert_eq!(
        second.replay.events.last().map(|event| &event.event_id),
        second.replay.high_water_mark_event_id.as_ref()
    );
}

#[tokio::test]
async fn subscription_drains_sparse_turn_pages_before_live_handoff() {
    let (store, events, subscriptions) = fixture().await;
    let first_identity = AgentTurnRef::new("session-1", "turn-1");
    let first_admission = commit_test_running_turn(
        store.as_ref(),
        first_identity.clone(),
        user_message(&first_identity),
    )
    .await;
    store
        .commit_terminal_settlement(CommitTerminalSettlementParams {
            event: AgentEvent {
                identity: first_identity,
                sequence: first_admission
                    .events
                    .last()
                    .expect("first admission should persist events")
                    .sequence
                    + 1,
                occurred_at: 11,
                payload: AgentEventPayload::TurnCompleted(AgentTurnCompleted {
                    stop_reason: AgentStopReason::EndTurn,
                }),
            },
            expected_event_seq: first_admission.active_turn.last_event_seq,
            completed_turn_metrics: None,
        })
        .await
        .expect("first Turn should complete");

    let second_identity = AgentTurnRef::new("session-1", "turn-2");
    let second_admission = commit_test_running_turn(
        store.as_ref(),
        second_identity.clone(),
        user_message(&second_identity),
    )
    .await;
    let mut subscription = subscriptions
        .subscribe(TimelineSubscribeCommand {
            session_id: "session-1".to_string(),
            turn_id: Some("turn-2".to_string()),
            after_event_id: None,
            replay_limit: 1,
            close_when_idle: false,
            close_on_terminal: false,
        })
        .await
        .expect("filtered Timeline subscription should open");

    for expected in &second_admission.events {
        let message = tokio::time::timeout(Duration::from_secs(1), subscription.events.recv())
            .await
            .expect("filtered replay should not stall")
            .expect("filtered replay stream should remain open");
        let TimelineSubscriptionMessage::Event(actual) = message else {
            panic!("filtered replay should contain only Timeline events");
        };
        assert_eq!(
            actual.event_id,
            TimelineEventId::from_seq(expected.event_seq).to_string()
        );
        assert_eq!(
            actual.event.agent_event().unwrap().identity,
            second_identity
        );
    }

    let terminal = store
        .commit_terminal_settlement(CommitTerminalSettlementParams {
            event: AgentEvent {
                identity: second_identity.clone(),
                sequence: second_admission
                    .events
                    .last()
                    .expect("second admission should persist events")
                    .sequence
                    + 1,
                occurred_at: 12,
                payload: AgentEventPayload::TurnCompleted(AgentTurnCompleted {
                    stop_reason: AgentStopReason::EndTurn,
                }),
            },
            expected_event_seq: second_admission.active_turn.last_event_seq,
            completed_turn_metrics: None,
        })
        .await
        .expect("second Turn should complete");
    let terminal_id = terminal.event_seq;
    events.publish_committed([terminal]);

    let message = tokio::time::timeout(Duration::from_secs(1), subscription.events.recv())
        .await
        .expect("live handoff should not stall")
        .expect("live Timeline stream should remain open");
    assert!(matches!(
        message,
        TimelineSubscriptionMessage::Event(event)
            if event.event_seq == terminal_id
                && event.event.agent_event().unwrap().identity == second_identity
                && matches!(event.event.agent_event().unwrap().payload, AgentEventPayload::TurnCompleted(_))
    ));
}

#[tokio::test]
async fn subscription_after_high_water_receives_one_terminal_then_closes() {
    let (store, events, subscriptions) = fixture().await;
    let identity = AgentTurnRef::new("session-1", "turn-1");
    let admission =
        commit_test_running_turn(store.as_ref(), identity.clone(), user_message(&identity)).await;
    let cursor = subscriptions
        .latest_event_id("session-1")
        .await
        .expect("latest event should load");
    let mut subscription = subscriptions
        .subscribe(TimelineSubscribeCommand {
            session_id: "session-1".to_string(),
            turn_id: Some("turn-1".to_string()),
            after_event_id: cursor,
            replay_limit: 10,
            close_when_idle: false,
            close_on_terminal: true,
        })
        .await
        .expect("subscription should open");
    let terminal = store
        .commit_terminal_settlement(CommitTerminalSettlementParams {
            event: AgentEvent {
                identity,
                sequence: admission
                    .events
                    .last()
                    .expect("admission should persist events")
                    .sequence
                    + 1,
                occurred_at: 11,
                payload: AgentEventPayload::TurnCompleted(AgentTurnCompleted {
                    stop_reason: AgentStopReason::EndTurn,
                }),
            },
            expected_event_seq: admission.active_turn.last_event_seq,
            completed_turn_metrics: None,
        })
        .await
        .expect("terminal event should commit");
    events.publish_committed([terminal]);

    let event = subscription
        .events
        .recv()
        .await
        .expect("terminal event should arrive");
    assert!(matches!(
        event,
        TimelineSubscriptionMessage::Event(event)
            if matches!(event.event.agent_event().unwrap().payload, AgentEventPayload::TurnCompleted(_))
    ));
    let closed = subscription
        .events
        .recv()
        .await
        .expect("terminal event should close the subscription");
    assert!(matches!(
        closed,
        TimelineSubscriptionMessage::Closed(closed)
            if closed.reason == TimelineSubscriptionCloseReason::Completed
    ));
}

#[tokio::test]
async fn idle_subscription_closes_without_fabricating_a_terminal_event() {
    let (_store, _events, subscriptions) = fixture().await;
    let mut subscription = subscriptions
        .subscribe(TimelineSubscribeCommand {
            session_id: "session-1".to_string(),
            turn_id: None,
            after_event_id: None,
            replay_limit: 10,
            close_when_idle: true,
            close_on_terminal: true,
        })
        .await
        .expect("idle subscription should open");

    let message = subscription
        .events
        .recv()
        .await
        .expect("idle subscription should close explicitly");
    assert!(matches!(
        message,
        TimelineSubscriptionMessage::Closed(closed)
            if closed.reason == TimelineSubscriptionCloseReason::Completed
    ));
}
