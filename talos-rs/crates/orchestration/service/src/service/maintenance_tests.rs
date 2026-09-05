use super::*;

#[tokio::test]
async fn session_maintenance_excludes_turn_admission_until_it_finishes() {
    let (service, _store) = test_application(
        Arc::new(TestAgentExecution {
            mode: TestAgentExecutionMode::StartFailed,
        }),
        configured_test_config(),
    )
    .await;
    let service = Arc::new(service);
    let actor = Actor::regular("member-user");
    let session_id = "session-maintenance-admission";
    service
        .create_session(
            &actor,
            create_session_params(session_id, request_source(), None),
        )
        .await
        .expect("Session should exist before maintenance");

    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let maintenance = {
        let service = service.clone();
        let entered = entered.clone();
        let release = release.clone();
        tokio::spawn(async move {
            service
                .run_idle_session_maintenance(session_id, move || async move {
                    entered.notify_one();
                    release.notified().await;
                    Ok(())
                })
                .await
        })
    };
    entered.notified().await;

    let error = service
        .start_turn(
            &actor,
            request_source(),
            start_turn_request(session_id, "blocked while maintenance owns the Session"),
            None,
        )
        .await
        .expect_err("Turn admission must not overlap Session maintenance");
    assert!(error.is_code(talos_error::ErrorCode::ActiveTurnRunning));

    release.notify_one();
    assert_eq!(
        maintenance
            .await
            .expect("maintenance task")
            .expect("maintenance"),
        Some(())
    );
    let error = service
        .start_turn(
            &actor,
            request_source(),
            start_turn_request(
                session_id,
                "admitted after maintenance releases the Session",
            ),
            None,
        )
        .await
        .expect_err("scenario Coordinator should now receive the Turn");
    assert!(error.is_code(talos_error::ErrorCode::InternalError));
}
