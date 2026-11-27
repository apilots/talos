use super::*;
use talos_agent_api::{ActiveTurnState, AgentTurnRef};

#[tokio::test]
async fn waiting_continuation_survives_manager_restart_without_reissuing_input() {
    let execution = Arc::new(RecordingAgentExecution::default());
    let (service, _, store, _) = test_service(execution.clone()).await;
    create_test_session_record(store.clone()).await;
    insert_pending_dynamic_action(store.as_ref(), Some("tool-original")).await;
    seed_worker_fixture(&service, store.as_ref(), execution.as_ref()).await;
    let active = store.get_agent_active_turn("s1").await.unwrap().unwrap();
    service
        .recover_active_turn(test_session(), active)
        .await
        .unwrap();
    assert!(execution.stream_requests.lock().unwrap().is_empty());
    assert_eq!(
        store
            .get_agent_active_turn("s1")
            .await
            .unwrap()
            .unwrap()
            .state,
        ActiveTurnState::WaitingForInput
    );
}

#[tokio::test]
async fn manager_restart_observes_running_worker_without_interrupting_or_resubmitting() {
    let execution = Arc::new(RecordingAgentExecution::default());
    *execution.hold_running_turn.lock().unwrap() = true;
    let (mut service, events, store, queue) = test_service(execution.clone()).await;
    create_test_session_record(store.clone()).await;
    queue.add("s1", test_user_input("running")).unwrap();
    service.try_dispatch_next("s1").await.unwrap().unwrap();
    service.reconcile_worker_events("s1").await.unwrap();
    let active = store.get_agent_active_turn("s1").await.unwrap().unwrap();
    service.execution = AgentExecutionDriver::new(
        service.execution.coordinator.clone(),
        store.clone(),
        events.clone(),
        service.execution.sessions.clone(),
    );
    service
        .recover_active_turn(test_session(), active.clone())
        .await
        .unwrap();
    assert_eq!(execution.stream_requests.lock().unwrap().len(), 1);
    assert_eq!(
        store
            .get_agent_active_turn("s1")
            .await
            .unwrap()
            .unwrap()
            .identity,
        active.identity
    );
}

#[tokio::test]
async fn product_history_without_worker_facts_cannot_manufacture_a_terminal() {
    let execution = Arc::new(RecordingAgentExecution::default());
    let (service, _, store, _) = test_service(execution.clone()).await;
    create_test_session_record(store.clone()).await;
    let identity = AgentTurnRef::new("s1", "unconfirmed");
    let input = completed_agent_user_input(
        &identity.turn_id,
        "message-unconfirmed",
        test_user_input("run"),
        1,
    )
    .unwrap();
    let admitted = commit_test_running_turn(store.as_ref(), identity.clone(), input).await;
    let failure = service
        .recover_active_turn(test_session(), admitted.active_turn)
        .await
        .unwrap_err();
    assert!(failure.is_code(talos_error::ErrorCode::ServiceUnavailable));
    assert!(store
        .get_latest_agent_terminal_event("s1")
        .await
        .unwrap()
        .is_none());
    assert!(execution.stream_requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn released_product_owner_lock_does_not_terminate_a_running_worker() {
    let execution = Arc::new(RecordingAgentExecution::default());
    *execution.hold_running_turn.lock().unwrap() = true;
    let (mut service, events, store, queue) = test_service(execution.clone()).await;
    create_test_session_record(store.clone()).await;
    queue.add("s1", test_user_input("running")).unwrap();
    service.try_dispatch_next("s1").await.unwrap().unwrap();
    service.reconcile_worker_events("s1").await.unwrap();
    let active = store.get_agent_active_turn("s1").await.unwrap().unwrap();
    service.execution = AgentExecutionDriver::new(
        service.execution.coordinator.clone(),
        store.clone(),
        events.clone(),
        service.execution.sessions.clone(),
    );
    let directory = tempfile::tempdir().unwrap();
    service.set_session_owner(talos_agent::SessionOwner::new(
        directory.path(),
        talos_error::SessionOwnerKind::Tui,
        talos_agent::SessionHold::Attached,
    ));
    service
        .recover_active_turn(test_session(), active)
        .await
        .unwrap();
    assert!(store
        .get_latest_agent_terminal_event("s1")
        .await
        .unwrap()
        .is_none());
}
