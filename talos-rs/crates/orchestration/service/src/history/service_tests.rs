use std::sync::Arc;

use talos_agent::{SessionHistoryReadParams, SessionHistoryWindow};
use talos_agent_api::{
    AgentEvent, AgentEventPayload, AgentMessage, AgentMessageBlock, AgentMessageBlockContent,
    AgentMessageBlockStatus, AgentMessageRole, AgentMessageStatus, AgentStopReason,
    AgentTurnCompleted, AgentTurnRef,
};
use talos_storage::SqliteStorage;
use talos_store::session::{
    AgentExecutionStore, CommitTerminalSettlementParams, DatabaseSessionStore,
};

use super::SessionHistoryService;
use crate::test_support::{
    canonical_completed_message_events, commit_test_running_turn, run_test_migrations,
};
use crate::TimelineEventService;

async fn fixture() -> (Arc<DatabaseSessionStore>, SessionHistoryService) {
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
    (store, SessionHistoryService::new(event_service))
}

#[tokio::test]
async fn recent_prompt_history_reads_the_terminal_metrics_beyond_the_first_event_page() {
    let (store, history) = fixture().await;
    let identity = AgentTurnRef::new("session-long", "turn-long");
    let user = completed_message(
        &identity,
        "user-long",
        AgentMessageRole::User,
        AgentMessageBlockContent::UserContent(talos_agent_api::AgentUserContent::Text {
            text: "complete a long analysis".to_string(),
        }),
    );
    let admission = commit_test_running_turn(store.as_ref(), identity.clone(), user).await;
    let mut tail = admission.active_turn.last_event_seq;
    let mut sequence = admission.events.last().unwrap().sequence;
    let mut metrics = talos_agent_api::AgentRunMetrics::default();
    for duration in 1..=10_001 {
        metrics.duration_ms = Some(duration);
        sequence += 1;
        tail = store
            .commit_progress_event(
                AgentEvent {
                    identity: identity.clone(),
                    sequence,
                    occurred_at: 12,
                    payload: AgentEventPayload::MetricsUpdated(metrics.clone()),
                },
                tail,
            )
            .await
            .unwrap()
            .active_turn
            .last_event_seq;
    }
    let terminal = store
        .commit_terminal_settlement(CommitTerminalSettlementParams {
            event: AgentEvent {
                identity: identity.clone(),
                sequence: sequence + 1,
                occurred_at: 13,
                payload: AgentEventPayload::TurnCompleted(AgentTurnCompleted {
                    stop_reason: AgentStopReason::EndTurn,
                }),
            },
            expected_event_seq: tail,
            completed_turn_metrics: None,
        })
        .await
        .unwrap();
    let turns = talos_agent::SessionHistoryReader::read(
        &history,
        SessionHistoryReadParams {
            session_id: identity.session_id.clone(),
            window: SessionHistoryWindow::Latest,
            limit: 1,
        },
    )
    .await
    .unwrap();
    assert_eq!(
        (
            turns[0].identity.clone(),
            turns[0].completed_event_seq,
            match &turns[0].result {
                talos_agent_api::AgentRunResult::Completed(output) => Some(output.metrics.clone()),
                talos_agent_api::AgentRunResult::Failed(_) => None,
            }
        ),
        (identity, terminal.event_seq, Some(metrics)),
    );
}

fn completed_message(
    identity: &AgentTurnRef,
    message_id: &str,
    role: AgentMessageRole,
    content: AgentMessageBlockContent,
) -> AgentMessage {
    AgentMessage {
        message_id: message_id.to_string(),
        turn_id: identity.turn_id.clone(),
        role,
        status: AgentMessageStatus::Completed,
        blocks: vec![AgentMessageBlock {
            block_id: format!("{message_id}-block"),
            status: AgentMessageBlockStatus::Completed,
            content,
            started_at: 10,
            completed_at: Some(11),
        }],
        created_at: 10,
        completed_at: Some(11),
    }
}

async fn commit_completed_turn(
    store: &dyn AgentExecutionStore,
    identity: AgentTurnRef,
    suffix: &str,
) {
    let user = completed_message(
        &identity,
        &format!("user-{suffix}"),
        AgentMessageRole::User,
        AgentMessageBlockContent::UserContent(talos_agent_api::AgentUserContent::Text {
            text: format!("question-{suffix}"),
        }),
    );
    let admission = commit_test_running_turn(store, identity.clone(), user).await;
    let assistant = completed_message(
        &identity,
        &format!("assistant-{suffix}"),
        AgentMessageRole::Agent,
        AgentMessageBlockContent::AssistantText(talos_agent_api::AgentAssistantText {
            phase: talos_agent_api::AgentAssistantTextPhase::FinalAnswer,
            text: format!("answer-{suffix}"),
        }),
    );
    let assistant_events = canonical_completed_message_events(
        identity.clone(),
        assistant,
        admission
            .events
            .last()
            .expect("admission should persist events")
            .sequence
            + 1,
    );
    let terminal_sequence = assistant_events
        .last()
        .expect("assistant lifecycle should contain events")
        .sequence
        + 1;
    let mut expected_event_seq = admission.active_turn.last_event_seq;
    for event in assistant_events {
        expected_event_seq = store
            .commit_progress_event(event, expected_event_seq)
            .await
            .expect("assistant message lifecycle should commit")
            .active_turn
            .last_event_seq;
    }
    store
        .commit_terminal_settlement(CommitTerminalSettlementParams {
            event: AgentEvent {
                identity,
                sequence: terminal_sequence,
                occurred_at: 13,
                payload: AgentEventPayload::TurnCompleted(AgentTurnCompleted {
                    stop_reason: AgentStopReason::EndTurn,
                }),
            },
            expected_event_seq,
            completed_turn_metrics: None,
        })
        .await
        .expect("terminal event should commit");
}

#[tokio::test]
async fn history_reduces_only_canonical_events_into_complete_turns() {
    let (store, history) = fixture().await;
    let identity = AgentTurnRef::new("session-1", "turn-1");
    commit_completed_turn(store.as_ref(), identity.clone(), "1").await;

    let response = history
        .read("session-1", None, Some(10))
        .await
        .expect("canonical history should load");

    assert_eq!(response.data.len(), 1);
    assert_eq!(response.data[0].identity, identity);
    let messages = match &response.data[0].result {
        talos_agent_api::AgentRunResult::Completed(output) => &output.messages,
        other => panic!("expected completed history Turn, got {other:?}"),
    };
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].message_id, "user-1");
    assert_eq!(messages[1].message_id, "assistant-1");
    assert_eq!(response.next_cursor, None);
}

#[tokio::test]
async fn history_cursor_keeps_a_fixed_high_water_mark() {
    let (store, history) = fixture().await;
    commit_completed_turn(
        store.as_ref(),
        AgentTurnRef::new("session-1", "turn-1"),
        "1",
    )
    .await;
    commit_completed_turn(
        store.as_ref(),
        AgentTurnRef::new("session-1", "turn-2"),
        "2",
    )
    .await;

    let first = history
        .read("session-1", None, Some(1))
        .await
        .expect("first history page should load");
    let cursor = first
        .next_cursor
        .as_deref()
        .expect("another completed Turn should produce a cursor");
    commit_completed_turn(
        store.as_ref(),
        AgentTurnRef::new("session-1", "turn-3"),
        "3",
    )
    .await;

    let second = history
        .read("session-1", Some(cursor), Some(10))
        .await
        .expect("second history page should load");

    assert_eq!(first.data[0].identity.turn_id, "turn-1");
    assert_eq!(second.data.len(), 1);
    assert_eq!(second.data[0].identity.turn_id, "turn-2");
    assert_eq!(second.next_cursor, None);
}

#[tokio::test]
async fn history_cursor_rejects_a_nonterminal_page_boundary() {
    let (store, history) = fixture().await;
    let identity = AgentTurnRef::new("session-1", "turn-1");
    let admission = commit_test_running_turn(
        store.as_ref(),
        identity.clone(),
        completed_message(
            &identity,
            "user-1",
            AgentMessageRole::User,
            AgentMessageBlockContent::UserContent(talos_agent_api::AgentUserContent::Text {
                text: "question".to_string(),
            }),
        ),
    )
    .await;
    let high_water = admission
        .events
        .last()
        .expect("admission has a tail")
        .event_seq;
    let nonterminal_boundary = admission.events[0].event_seq;
    let cursor = super::format_history_cursor(high_water, nonterminal_boundary);

    let error = history
        .read("session-1", Some(&cursor), Some(10))
        .await
        .expect_err("forged nonterminal history boundary must fail");

    assert_eq!(error.error_code(), talos_error::ErrorCode::InvalidRequest);
}

#[tokio::test]
async fn bounded_history_windows_preserve_order_across_global_event_sequence_gaps() {
    let (store, history) = fixture().await;
    commit_completed_turn(
        store.as_ref(),
        AgentTurnRef::new("session-1", "turn-1"),
        "1",
    )
    .await;
    let first_terminal = history
        .read("session-1", None, Some(10))
        .await
        .expect("first history should load")
        .data[0]
        .completed_event_seq;
    commit_completed_turn(
        store.as_ref(),
        AgentTurnRef::new("session-2", "foreign-turn"),
        "foreign",
    )
    .await;
    commit_completed_turn(
        store.as_ref(),
        AgentTurnRef::new("session-1", "turn-2"),
        "2",
    )
    .await;
    commit_completed_turn(
        store.as_ref(),
        AgentTurnRef::new("session-1", "turn-3"),
        "3",
    )
    .await;

    let latest = history
        .read_window(SessionHistoryReadParams {
            session_id: "session-1".to_string(),
            window: SessionHistoryWindow::Latest,
            limit: 2,
        })
        .await
        .expect("latest bounded history should load");
    let after_first = history
        .read_window(SessionHistoryReadParams {
            session_id: "session-1".to_string(),
            window: SessionHistoryWindow::OldestAfter {
                completed_event_seq: first_terminal,
            },
            limit: 10,
        })
        .await
        .expect("oldest bounded history should load");

    assert_eq!(
        latest
            .iter()
            .map(|turn| turn.identity.turn_id.as_str())
            .collect::<Vec<_>>(),
        vec!["turn-2", "turn-3"]
    );
    assert_eq!(
        after_first
            .iter()
            .map(|turn| turn.identity.turn_id.as_str())
            .collect::<Vec<_>>(),
        vec!["turn-2", "turn-3"]
    );
}
