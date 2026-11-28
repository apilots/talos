use super::*;

#[tokio::test]
async fn settlement_notice_commits_atomically_and_replays_once() {
    let (store, _directory) = store().await;
    let mut events = admission();
    events.push(fact(
        14,
        3,
        AgentEventPayload::TurnCompleted(AgentTurnCompleted {
            stop_reason: AgentStopReason::EndTurn,
        }),
    ));
    store.project_worker_events(params(events)).await.unwrap();
    let cursor = store
        .pending_worker_settlements("session-1")
        .await
        .unwrap()
        .remove(0)
        .cursor;
    let notice = talos_agent_api::TimelineSystemNotice {
        identity: AgentTurnRef::new("session-1", "turn-1"),
        occurred_at: 10,
        notice: talos_agent_api::AgentSystemNotice {
            level: talos_types::TalosTimelineNoticeLevel::Warning,
            content: talos_agent_api::SystemNoticeContent::Error(
                TalosError::new(
                    ErrorCode::ToolExecutionFailed,
                    "No Goal was created in this Turn.",
                )
                .to_api_error(),
            ),
        },
    };
    let mut wrong_turn = notice.clone();
    wrong_turn.identity.turn_id = "another-turn".into();
    assert!(store
        .complete_worker_settlement_with_notice(&cursor, wrong_turn)
        .await
        .unwrap_err()
        .is_code(ErrorCode::InvalidRequest));
    let mut stale = cursor.clone();
    stale.generation += 1;
    assert!(store
        .complete_worker_settlement_with_notice(&stale, notice.clone())
        .await
        .unwrap_err()
        .is_code(ErrorCode::ResourceNotFound));
    store.storage.execute("CREATE TRIGGER reject_notice BEFORE INSERT ON agent_events WHEN NEW.sequence IS NULL BEGIN SELECT RAISE(FAIL,'notice-unavailable'); END", &[]).await.unwrap();
    assert!(store
        .complete_worker_settlement_with_notice(&cursor, notice.clone())
        .await
        .is_err());
    assert_eq!(
        store
            .pending_worker_settlements("session-1")
            .await
            .unwrap()
            .len(),
        1
    );
    store
        .storage
        .execute("DROP TRIGGER reject_notice", &[])
        .await
        .unwrap();
    let committed = store
        .complete_worker_settlement_with_notice(&cursor, notice.clone())
        .await
        .unwrap()
        .unwrap();
    assert!(store
        .complete_worker_settlement_with_notice(&cursor, notice)
        .await
        .unwrap()
        .is_none());
    assert!(store
        .pending_worker_settlements("session-1")
        .await
        .unwrap()
        .is_empty());
    let notices = store
        .list_turn_notices(&[AgentTurnRef::new("session-1", "turn-1")], i64::MAX)
        .await
        .unwrap();
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0].event_seq, committed.event_seq);
}
