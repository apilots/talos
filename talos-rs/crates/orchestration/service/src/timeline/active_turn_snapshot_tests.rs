use std::sync::Arc;

use talos_agent_api::{
    AgentAssistantText, AgentAssistantTextPhase, AgentEvent, AgentEventPayload, AgentMessage,
    AgentMessageBlock, AgentMessageBlockContent, AgentMessageBlockDelta,
    AgentMessageBlockDeltaContent, AgentMessageBlockStarted, AgentMessageBlockStatus,
    AgentMessageRole, AgentMessageStarted, AgentMessageStatus, AgentStopReason, AgentTurnCompleted,
    AgentTurnRef, TimelineEventPayload,
};
use talos_storage::SqliteStorage;
use talos_store::session::{
    AgentActiveTurnRecord, AgentExecutionStore, CommitTerminalSettlementParams,
    DatabaseSessionStore,
};

use super::*;
use crate::test_support::{commit_test_running_turn, run_test_migrations};
use crate::{TimelineSubscribeCommand, TimelineSubscriptionMessage};

struct Fixture {
    store: Arc<DatabaseSessionStore>,
    service: TimelineEventService,
    active: AgentActiveTurnRecord,
    sequence: u64,
}

impl Fixture {
    async fn new() -> Self {
        let storage = Arc::new(
            SqliteStorage::new_with_max_connections(":memory:", 1)
                .await
                .unwrap(),
        );
        run_test_migrations(storage.pool()).await;
        let store = Arc::new(DatabaseSessionStore::new(storage).await.unwrap());
        let identity = AgentTurnRef::new("session-1", "turn-1");
        let admitted = commit_test_running_turn(
            store.as_ref(),
            identity,
            AgentMessage {
                message_id: "input".into(),
                turn_id: "turn-1".into(),
                role: AgentMessageRole::User,
                status: AgentMessageStatus::Completed,
                blocks: vec![],
                created_at: 10,
                completed_at: Some(10),
            },
        )
        .await;
        Self {
            service: TimelineEventService::new(store.clone()),
            store,
            sequence: admitted.events.last().unwrap().sequence,
            active: admitted.active_turn,
        }
    }

    async fn append(&mut self, payload: AgentEventPayload) {
        self.sequence += 1;
        let committed = self
            .store
            .commit_progress_event(
                AgentEvent {
                    identity: self.active.identity.clone(),
                    sequence: self.sequence,
                    occurred_at: 11,
                    payload,
                },
                self.active.last_event_seq,
            )
            .await
            .unwrap();
        self.active = committed.active_turn;
    }

    async fn start_text(&mut self) {
        self.append(AgentEventPayload::MessageStarted(AgentMessageStarted {
            message: AgentMessage {
                message_id: "answer".into(),
                turn_id: "turn-1".into(),
                role: AgentMessageRole::Agent,
                status: AgentMessageStatus::InProgress,
                blocks: vec![],
                created_at: 11,
                completed_at: None,
            },
        }))
        .await;
        self.append(AgentEventPayload::MessageBlockStarted(
            AgentMessageBlockStarted {
                message_id: "answer".into(),
                block: AgentMessageBlock {
                    block_id: "text".into(),
                    status: AgentMessageBlockStatus::Running,
                    content: AgentMessageBlockContent::AssistantText(AgentAssistantText {
                        phase: AgentAssistantTextPhase::FinalAnswer,
                        text: String::new(),
                    }),
                    started_at: 11,
                    completed_at: None,
                },
            },
        ))
        .await;
    }

    async fn text(&mut self, text: &str) {
        self.append(AgentEventPayload::MessageBlockDelta(
            AgentMessageBlockDelta {
                message_id: "answer".into(),
                block_id: "text".into(),
                delta: AgentMessageBlockDeltaContent::Text(text.into()),
            },
        ))
        .await;
    }
}

#[tokio::test]
async fn running_turn_query_returns_content_and_resumes_only_after_its_offset() {
    let mut f = Fixture::new().await;
    f.start_text().await;
    // Cross the canonical scan page boundary; response size follows content,
    // not the count of individual token events.
    for _ in 0..2050 {
        f.text("x").await;
    }
    let response = f.service.read_active_turn("session-1").await.unwrap();
    let snapshot = response.active_turn.clone().unwrap();
    assert_eq!(response.last_completed_event_seq, 0);
    assert_eq!(snapshot.last_event_seq, f.active.last_event_seq);
    assert_eq!(snapshot.last_sequence, f.sequence);
    assert_eq!(snapshot.messages.len(), 2);
    assert_eq!(
        snapshot.messages[1].blocks[0].content,
        AgentMessageBlockContent::AssistantText(AgentAssistantText {
            phase: AgentAssistantTextPhase::FinalAnswer,
            text: "x".repeat(2050),
        })
    );
    let wire = serde_json::to_value(&response).unwrap();
    assert_eq!(
        wire["activeTurn"]["lastEventId"],
        snapshot.active_turn.last_event_id
    );
    assert_eq!(
        serde_json::from_value::<SessionActiveTurnResponse>(wire).unwrap(),
        response
    );

    f.text("tail").await;
    let subscriptions = TimelineSubscriptionService::new(Arc::new(f.service));
    let mut subscription = subscriptions
        .subscribe(TimelineSubscribeCommand {
            session_id: "session-1".into(),
            turn_id: None,
            after_event_id: Some(TimelineEventId::from(snapshot.active_turn.last_event_id)),
            replay_limit: 1,
            close_when_idle: false,
            close_on_terminal: false,
        })
        .await
        .unwrap();
    assert_eq!(subscription.replay.events.len(), 1);
    assert_eq!(subscription.replay.next_cursor, None);
    let TimelineSubscriptionMessage::Event(event) = subscription.events.recv().await.unwrap()
    else {
        panic!("expected the next delta");
    };
    let TimelineEventPayload::Agent(event) = event.event else {
        panic!("expected Agent event");
    };
    assert_eq!(event.sequence, snapshot.last_sequence + 1);
    assert_eq!(
        event.payload,
        AgentEventPayload::MessageBlockDelta(AgentMessageBlockDelta {
            message_id: "answer".into(),
            block_id: "text".into(),
            delta: AgentMessageBlockDeltaContent::Text("tail".into()),
        })
    );
}

#[tokio::test]
async fn running_turn_read_never_claims_content_after_the_captured_offset() {
    let mut f = Fixture::new().await;
    f.start_text().await;
    f.text("before").await;
    let captured = f
        .service
        .display_active_turn("session-1")
        .await
        .unwrap()
        .unwrap();
    let sequence = f.sequence;
    f.text("after").await;
    let snapshot = f
        .service
        .read_turn_snapshot(captured.clone())
        .await
        .unwrap();
    assert_eq!(snapshot.active_turn, captured);
    assert_eq!(snapshot.last_sequence, sequence);
    assert_eq!(
        snapshot.messages[1].blocks[0].content,
        AgentMessageBlockContent::AssistantText(AgentAssistantText {
            phase: AgentAssistantTextPhase::FinalAnswer,
            text: "before".into(),
        })
    );
}

#[tokio::test]
async fn completion_after_running_turn_capture_is_delivered_after_its_offset() {
    let f = Fixture::new().await;
    let captured = f
        .service
        .display_active_turn("session-1")
        .await
        .unwrap()
        .unwrap();
    let terminal = f
        .store
        .commit_terminal_settlement(CommitTerminalSettlementParams {
            event: AgentEvent {
                identity: f.active.identity,
                sequence: f.sequence + 1,
                occurred_at: 12,
                payload: AgentEventPayload::TurnCompleted(AgentTurnCompleted {
                    stop_reason: AgentStopReason::EndTurn,
                }),
            },
            expected_event_seq: f.active.last_event_seq,
            completed_turn_metrics: None,
        })
        .await
        .unwrap();
    let snapshot = f.service.read_turn_snapshot(captured).await.unwrap();
    assert_eq!(snapshot.last_sequence, f.sequence);
    let idle = f.service.read_active_turn("session-1").await.unwrap();
    assert_eq!(idle.active_turn, None);
    assert_eq!(idle.last_completed_event_seq, terminal.event_seq);
    let replay = f
        .service
        .list_events_after(
            "session-1",
            Some(&TimelineEventId::from(snapshot.active_turn.last_event_id)),
            10,
        )
        .await
        .unwrap();
    assert_eq!(
        replay
            .iter()
            .map(|event| event.event_seq)
            .collect::<Vec<_>>(),
        vec![terminal.event_seq]
    );
}

#[tokio::test]
async fn parallel_history_boundary_covers_the_preceding_completed_turn() {
    let f = Fixture::new().await;
    let terminal = f
        .store
        .commit_terminal_settlement(CommitTerminalSettlementParams {
            event: AgentEvent {
                identity: f.active.identity.clone(),
                sequence: f.sequence + 1,
                occurred_at: 12,
                payload: AgentEventPayload::TurnCompleted(AgentTurnCompleted {
                    stop_reason: AgentStopReason::EndTurn,
                }),
            },
            expected_event_seq: f.active.last_event_seq,
            completed_turn_metrics: None,
        })
        .await
        .unwrap();
    let next = commit_test_running_turn(
        f.store.as_ref(),
        AgentTurnRef::new("session-1", "turn-2"),
        AgentMessage {
            message_id: "next-input".into(),
            turn_id: "turn-2".into(),
            role: AgentMessageRole::User,
            status: AgentMessageStatus::Completed,
            blocks: vec![],
            created_at: 13,
            completed_at: Some(13),
        },
    )
    .await;
    let response = f.service.read_active_turn("session-1").await.unwrap();
    assert_eq!(response.last_completed_event_seq, terminal.event_seq);
    let snapshot = response.active_turn.unwrap();
    assert_eq!(snapshot.last_event_seq, next.active_turn.last_event_seq);
    assert_eq!(snapshot.active_turn.identity.turn_id, "turn-2");
}
