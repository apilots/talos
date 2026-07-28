use super::*;
use crate::test_support::run_test_migrations;
use std::sync::Arc;
use talos_storage::SqliteStorage;
use talos_store::session::{
    ActiveTurnStatus, ActiveTurnStore, DatabaseSessionStore, PendingUserAction,
};
use talos_types::talos::{
    AgentEvent, AgentEventPayload, AgentEventType, StopReason, TalosAssistantTextCommittedEvent,
    TalosItemDeltaEvent, TalosTimelineDelta, TalosTimelinePhase, TalosTurnCompletedEvent,
    TalosTurnStartedEvent, TokenUsage,
};

async fn service() -> (TimelineEventService, Arc<DatabaseSessionStore>) {
    let storage = Arc::new(SqliteStorage::new(":memory:").await.unwrap());
    run_test_migrations(storage.pool()).await;
    let store = Arc::new(DatabaseSessionStore::new(storage).await.unwrap());
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

fn timeline_event(
    sequence: u64,
    event_type: AgentEventType,
    payload: AgentEventPayload,
) -> AgentEvent {
    AgentEvent {
        event_seq: None,
        event_id: None,
        created_at_ms: Some(1000 + sequence as i64),
        event_type,
        turn_id: "turn_1".to_string(),
        message_id: "msg_1".to_string(),
        session_id: "s1".to_string(),
        sequence,
        payload,
    }
}

fn completed_event(sequence: u64) -> AgentEvent {
    timeline_event(
        sequence,
        AgentEventType::TurnCompleted,
        AgentEventPayload::TurnCompleted(TalosTurnCompletedEvent {
            request_id: "req_1".to_string(),
            turn_id: "turn_1".to_string(),
            completed_at_ms: 1000,
            stop_reason: StopReason::EndTurn,
            usage: TokenUsage::default(),
            metrics: None,
            artifact_seq: None,
        }),
    )
}

fn started_event(sequence: u64) -> AgentEvent {
    timeline_event(
        sequence,
        AgentEventType::TurnStarted,
        AgentEventPayload::TurnStarted(TalosTurnStartedEvent {
            request_id: "req_1".to_string(),
            turn_id: "turn_1".to_string(),
            started_at_ms: 1000,
        }),
    )
}

fn dynamic_event(sequence: u64) -> AgentEvent {
    timeline_event(
        sequence,
        AgentEventType::ItemDelta,
        AgentEventPayload::ItemDelta(TalosItemDeltaEvent {
            request_id: "req_1".to_string(),
            turn_id: "turn_1".to_string(),
            item_id: "tool-1".to_string(),
            delta: TalosTimelineDelta::ToolDynamicRequest {
                call_id: "call_1".to_string(),
                tool_name: "ExternalTool".to_string(),
                arguments: serde_json::json!({"query": "input"}).into(),
            },
        }),
    )
}

fn final_answer_committed_event(sequence: u64) -> AgentEvent {
    timeline_event(
        sequence,
        AgentEventType::AssistantTextCommitted,
        AgentEventPayload::AssistantTextCommitted(TalosAssistantTextCommittedEvent {
            request_id: "req_1".to_string(),
            turn_id: "turn_1".to_string(),
            item_id: "assistant-1".to_string(),
            phase: TalosTimelinePhase::FinalAnswer,
        }),
    )
}

#[tokio::test]
async fn completed_event_updates_active_turn_terminal_status() {
    let (service, store) = service().await;
    let record = service
        .record_talos_event(completed_event(1))
        .await
        .unwrap();

    let active = store.get_active_turn("s1").await.unwrap().unwrap();
    assert_eq!(active.status, ActiveTurnStatus::Completed);
    assert_eq!(
        active.last_event_id,
        Some(TimelineEventId::from(record.event_id))
    );
}

#[tokio::test]
async fn final_answer_commit_does_not_update_active_turn_terminal_status() {
    let (service, store) = service().await;
    service.record_talos_event(started_event(1)).await.unwrap();
    let committed = service
        .record_talos_event(final_answer_committed_event(2))
        .await
        .unwrap();

    let active = store.get_active_turn("s1").await.unwrap().unwrap();
    assert_eq!(active.status, ActiveTurnStatus::Running);
    assert_eq!(
        active.last_event_id,
        Some(TimelineEventId::from(committed.event_id))
    );
}

#[tokio::test]
async fn pending_dynamic_event_updates_active_turn_user_action() {
    let (service, store) = service().await;
    service.record_talos_event(dynamic_event(1)).await.unwrap();

    let active = store.get_active_turn("s1").await.unwrap().unwrap();
    assert_eq!(active.status, ActiveTurnStatus::WaitingUserAction);
    assert!(matches!(
        active.pending_user_action,
        Some(PendingUserAction::DynamicToolResult(ref pending))
            if pending.item_id.as_deref() == Some("tool-1")
                && pending.call_id == "call_1"
                && pending.tool_name == "ExternalTool"
    ));
}

#[tokio::test]
async fn completed_event_preserves_pending_dynamic_active_turn() {
    let (service, store) = service().await;
    service.record_talos_event(dynamic_event(1)).await.unwrap();
    let completed = service
        .record_talos_event(completed_event(2))
        .await
        .unwrap();

    let active = store.get_active_turn("s1").await.unwrap().unwrap();
    assert_eq!(active.status, ActiveTurnStatus::WaitingUserAction);
    assert_eq!(
        active.last_event_id,
        Some(TimelineEventId::from(completed.event_id))
    );
    assert!(matches!(
        active.pending_user_action,
        Some(PendingUserAction::DynamicToolResult(ref pending))
            if pending.call_id == "call_1" && pending.tool_name == "ExternalTool"
    ));
}
