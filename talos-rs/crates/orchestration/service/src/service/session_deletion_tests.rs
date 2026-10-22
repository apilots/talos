use super::*;

async fn deletion_fixture(
    mode: TestAgentExecutionMode,
) -> (TalosService, Arc<DatabaseSessionStore>) {
    let (service, store) = test_application(
        Arc::new(TestAgentExecution { mode }),
        configured_test_config(),
    )
    .await;
    service
        .create_session(
            &Actor::regular("member-user"),
            create_session_params("delete-target", request_source(), None),
        )
        .await
        .unwrap();
    (service, store)
}

#[tokio::test]
async fn unresponsive_worker_verification_or_cleanup_cannot_hang_or_delete_the_session() {
    for mode in [
        TestAgentExecutionMode::WorkerStateBlocked,
        TestAgentExecutionMode::WorkerClearBlocked,
    ] {
        let (service, store) = deletion_fixture(mode).await;
        let actor = Actor::regular("member-user");
        service
            .message_queue
            .add(
                "delete-target",
                crate::test_support::test_user_input("retained"),
            )
            .unwrap();
        let error = tokio::time::timeout(
            Duration::from_secs(3),
            service.delete_session(&actor, "delete-target"),
        )
        .await
        .expect("deletion must return within its bounded preflight")
        .unwrap_err();
        assert_eq!(
            error.error_code(),
            talos_error::ErrorCode::ServiceUnavailable
        );
        assert!(store
            .get_session_record("delete-target")
            .await
            .unwrap()
            .is_some());
        assert_eq!(
            service
                .message_queue
                .list("delete-target")
                .unwrap()
                .messages
                .len(),
            1
        );
        // The timed-out operation must release the Computer admission lock.
        assert!(tokio::time::timeout(
            Duration::from_secs(1),
            service.execution_gate.for_node("embedded").write_owned()
        )
        .await
        .is_ok());
    }
}

#[tokio::test]
async fn worker_reported_active_turn_rejects_deletion_with_records_preserved() {
    let (service, store) = deletion_fixture(TestAgentExecutionMode::WorkerStateActive).await;
    let error = service
        .delete_session(&Actor::regular("member-user"), "delete-target")
        .await
        .unwrap_err();
    assert_eq!(
        error.error_code(),
        talos_error::ErrorCode::ActiveTurnRunning
    );
    assert!(store
        .get_session_record("delete-target")
        .await
        .unwrap()
        .is_some());
}

#[tokio::test]
async fn locally_known_parent_or_child_activity_rejects_deletion_before_worker_verification() {
    for active_child in [false, true] {
        let (service, store) = deletion_fixture(TestAgentExecutionMode::WorkerStateBlocked).await;
        let active_session = if active_child {
            let mut child = store
                .get_session_record("delete-target")
                .await
                .unwrap()
                .unwrap();
            child.id = "idle-child".into();
            child.tree_identity = talos_store::session::SessionTreeIdentity::Subagent {
                parent: AgentTurnRef::new("delete-target", "parent-turn"),
                tree_root_session_id: "delete-target".into(),
                path: "/root/child".parse().unwrap(),
            };
            store.insert_session_record(&child).await.unwrap();
            child.id = "active-child".into();
            if let talos_store::session::SessionTreeIdentity::Subagent { path, .. } =
                &mut child.tree_identity
            {
                *path = "/root/active".parse().unwrap();
            }
            store.insert_session_record(&child).await.unwrap();
            child.id
        } else {
            "delete-target".into()
        };
        let identity = AgentTurnRef::new(&active_session, "running-turn");
        let message = crate::turn::completed_agent_user_input(
            &identity.turn_id,
            "running-input",
            crate::test_support::test_user_input("running"),
            1,
        )
        .unwrap();
        crate::test_support::commit_test_running_turn(store.as_ref(), identity, message).await;
        let error = tokio::time::timeout(
            Duration::from_secs(1),
            service.delete_session(&Actor::regular("member-user"), "delete-target"),
        )
        .await
        .expect("known activity must reject without waiting for Worker")
        .unwrap_err();
        assert_eq!(
            error.error_code(),
            talos_error::ErrorCode::ActiveTurnRunning
        );
        assert!(store
            .get_session_record("delete-target")
            .await
            .unwrap()
            .is_some());
        assert!(store
            .get_agent_active_turn(&active_session)
            .await
            .unwrap()
            .is_some());
    }
}

#[tokio::test]
async fn offline_verification_reports_the_deletion_protection_and_retains_its_typed_cause() {
    let (service, store) = deletion_fixture(TestAgentExecutionMode::WorkerStateUnavailable).await;
    let error = service
        .delete_session(&Actor::regular("member-user"), "delete-target")
        .await
        .unwrap_err();
    assert_eq!(
        error.error_code(),
        talos_error::ErrorCode::ServiceUnavailable
    );
    assert_eq!(
        error.to_api_error().message,
        "Session deletion could not confirm idle execution. Retry when its Computer is available."
    );
    let cause = std::error::Error::source(&error)
        .unwrap()
        .downcast_ref::<TalosError>()
        .unwrap();
    assert_eq!(
        cause.error_code(),
        talos_error::ErrorCode::ServiceUnavailable
    );
    assert!(store
        .get_session_record("delete-target")
        .await
        .unwrap()
        .is_some());
}
