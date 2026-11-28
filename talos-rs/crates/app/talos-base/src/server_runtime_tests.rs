use super::*;

#[tokio::test]
async fn handle_shutdown_waits_for_stopped_state() {
    let lifecycle = Arc::new(ServerLifecycle::new());
    let handle = TalosServerHandle {
        shutdown_token: CancellationToken::new(),
        lifecycle: lifecycle.clone(),
        wait_timeout: Duration::from_secs(1),
    };

    let token = handle.shutdown_token.clone();
    let lifecycle_for_task = lifecycle.clone();
    let waiter = tokio::spawn(async move { handle.shutdown().await });
    token.cancelled().await;
    lifecycle_for_task.store(ServerLifecycleState::Quiescing);
    lifecycle_for_task.store(ServerLifecycleState::Draining);
    lifecycle_for_task.store(ServerLifecycleState::Stopped);

    waiter
        .await
        .expect("task should join")
        .expect("shutdown should finish");
}

#[tokio::test]
async fn handle_shutdown_times_out_without_stopped_state() {
    let lifecycle = Arc::new(ServerLifecycle::new());
    let handle = TalosServerHandle {
        shutdown_token: CancellationToken::new(),
        lifecycle,
        wait_timeout: Duration::from_millis(50),
    };

    let err = handle
        .shutdown()
        .await
        .expect_err("shutdown should time out");
    assert!(err.is_internal());
}

#[tokio::test]
async fn lifecycle_watch_reports_expected_state_order() {
    let lifecycle = Arc::new(ServerLifecycle::new());
    let mut updates = lifecycle.subscribe();

    lifecycle.store(ServerLifecycleState::Quiescing);
    updates
        .changed()
        .await
        .expect("quiescing update should arrive");
    assert_eq!(
        *updates.borrow_and_update(),
        ServerLifecycleState::Quiescing
    );

    lifecycle.store(ServerLifecycleState::Draining);
    updates
        .changed()
        .await
        .expect("draining update should arrive");
    assert_eq!(*updates.borrow_and_update(), ServerLifecycleState::Draining);

    lifecycle.store(ServerLifecycleState::Stopped);
    updates
        .changed()
        .await
        .expect("stopped update should arrive");
    assert_eq!(*updates.borrow_and_update(), ServerLifecycleState::Stopped);

    assert!(SERVER_SHUTDOWN_WAIT_TIMEOUT > Duration::from_secs(0));
}
