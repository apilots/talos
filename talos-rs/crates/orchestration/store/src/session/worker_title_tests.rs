use super::*;

#[tokio::test]
async fn worker_title_requires_a_visible_completed_turn_and_preserves_existing_titles() {
    let (store, _directory) = store().await;
    store
        .storage
        .execute(
            "UPDATE sessions SET title=? WHERE session_key='session-1'",
            &[talos_types::DEFAULT_SESSION_TITLE],
        )
        .await
        .unwrap();
    store
        .project_worker_events(params(admission()))
        .await
        .unwrap();
    let identity = AgentTurnRef::new("session-1", "turn-1");
    assert!(!store
        .set_generated_title(&identity, "Too early")
        .await
        .unwrap());
    let mut events = admission();
    events.push(fact(
        14,
        3,
        AgentEventPayload::TurnCompleted(AgentTurnCompleted {
            stop_reason: AgentStopReason::EndTurn,
        }),
    ));
    store.project_worker_events(params(events)).await.unwrap();
    assert!(store
        .set_generated_title(&identity, "Generated title")
        .await
        .unwrap());
    assert!(!store
        .set_generated_title(&identity, "Replace existing title")
        .await
        .unwrap());
    let cursor = store
        .pending_worker_settlements("session-1")
        .await
        .unwrap()
        .remove(0)
        .cursor;
    store.complete_worker_settlement(&cursor).await.unwrap();
    store.clear_session_history("session-1", 20).await.unwrap();
    store
        .storage
        .execute(
            "UPDATE sessions SET title=? WHERE session_key='session-1'",
            &[talos_types::DEFAULT_SESSION_TITLE],
        )
        .await
        .unwrap();
    assert!(!store
        .set_generated_title(&identity, "Stale title")
        .await
        .unwrap());
}
