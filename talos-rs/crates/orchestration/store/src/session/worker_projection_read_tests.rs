use super::*;

#[tokio::test]
async fn unchanged_worker_page_reads_while_another_connection_holds_the_writer() {
    let (store, directory) = store().await;
    let committed = store
        .project_worker_events(params(admission()))
        .await
        .unwrap();
    let blocker = SqliteStorage::new_with_max_connections(
        directory.path().join("projection.db").to_str().unwrap(),
        1,
    )
    .await
    .unwrap();
    let held = blocker.write_transaction().await.unwrap();
    let mut request = params(admission());
    request.after_cursor = Some(committed.cursor.clone());
    request.page.events.clear();
    request.page.next_cursor = committed.cursor.clone();
    let projected = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        store.project_worker_events(request),
    )
    .await
    .expect("an unchanged Worker page must not require the writer")
    .unwrap();
    held.rollback().await.unwrap();
    assert!(projected.events.is_empty());
    assert_eq!(projected.cursor, committed.cursor);
    assert_eq!(
        projected.active_turn.unwrap().identity,
        AgentTurnRef::new("session-1", "turn-1")
    );
}

#[tokio::test]
async fn unchanged_worker_page_still_requires_the_session_owner() {
    let (store, _directory) = store().await;
    let committed = store
        .project_worker_events(params(admission()))
        .await
        .unwrap();
    let mut request = params(admission());
    request.after_cursor = Some(committed.cursor.clone());
    request.page.next_cursor = committed.cursor;
    request.page.events.clear();
    request.execution_node_id = "another-worker".into();
    let error = store.project_worker_events(request).await.err().unwrap();
    assert!(error.is_code(talos_error::ErrorCode::PermissionDenied));
}
