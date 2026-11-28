use super::*;

#[tokio::test]
async fn view_rebuild_authorizes_before_contacting_an_unavailable_worker() {
    let (service, store) = test_application(
        Arc::new(TestAgentExecution {
            mode: TestAgentExecutionMode::WorkerStateUnavailable,
        }),
        configured_test_config(),
    )
    .await;
    let actor = Actor::regular("member-user");
    service
        .create_session(
            &actor,
            create_session_params("rebuild-target", request_source(), None),
        )
        .await
        .unwrap();
    let before = store.get_session_record("rebuild-target").await.unwrap();
    let denied = service
        .rebuild_session_view(&Actor::regular("other-user"), "rebuild-target")
        .await
        .unwrap_err();
    assert_eq!(
        denied.error_code(),
        talos_error::ErrorCode::ResourceNotFound
    );
    let unavailable = service
        .rebuild_session_view(&actor, "rebuild-target")
        .await
        .unwrap_err();
    assert_eq!(
        unavailable.error_code(),
        talos_error::ErrorCode::ServiceUnavailable
    );
    assert_eq!(
        store.get_session_record("rebuild-target").await.unwrap(),
        before
    );
    assert!(service
        .turns
        .run_when_session_idle("rebuild-target", || async { Ok(()) })
        .await
        .unwrap()
        .is_some());
}

#[tokio::test]
async fn view_rebuild_preserves_empty_session_configuration_and_releases_maintenance() {
    let (service, store) = test_application(
        Arc::new(TestAgentExecution {
            mode: TestAgentExecutionMode::Accepted,
        }),
        configured_test_config(),
    )
    .await;
    let actor = Actor::regular("member-user");
    let created = service
        .create_session(
            &actor,
            create_session_params("rebuild-empty", request_source(), None),
        )
        .await
        .unwrap();
    let before = store.get_session_record("rebuild-empty").await.unwrap();
    let rebuilt = service
        .rebuild_session_view(&actor, "rebuild-empty")
        .await
        .unwrap();
    assert_eq!(rebuilt, created);
    assert_eq!(
        store.get_session_record("rebuild-empty").await.unwrap(),
        before
    );
    assert!(service
        .turns
        .run_when_session_idle("rebuild-empty", || async { Ok(()) })
        .await
        .unwrap()
        .is_some());
}
