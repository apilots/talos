use super::*;

#[tokio::test]
async fn patch_session_updates_permission_mode_when_idle() {
    let (service, _store) = test_application(
        Arc::new(TestAgentExecution {
            mode: TestAgentExecutionMode::StartFailed,
        }),
        configured_test_config(),
    )
    .await;
    let actor = Actor::regular("member-user");
    service
        .create_session(
            &actor,
            create_session_params("session-patch-permission", request_source(), None),
        )
        .await
        .expect("session should be created");

    let patched = service
        .patch_session(
            &actor,
            "session-patch-permission",
            SessionPatch {
                conf: Some(talos_types::SessionConf {
                    user_permission_mode: Some(talos_types::UserPermissionMode::AskForApproval),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .await
        .expect("idle session patch should succeed");

    assert_eq!(
        patched.effective_mode,
        talos_types::UserPermissionMode::AskForApproval
    );
    let session = service
        .get_session(&actor, "session-patch-permission")
        .await
        .expect("patched session should be readable");
    assert_eq!(
        session.effective_mode,
        talos_types::UserPermissionMode::AskForApproval
    );
}

#[tokio::test]
async fn patch_session_rejects_running_active_turn() {
    let (service, store) = test_application(
        Arc::new(TestAgentExecution {
            mode: TestAgentExecutionMode::StartFailed,
        }),
        configured_test_config(),
    )
    .await;
    let actor = Actor::regular("member-user");
    service
        .create_session(
            &actor,
            create_session_params("session-patch-active", request_source(), None),
        )
        .await
        .expect("session should be created");
    let identity = talos_agent_api::AgentTurnRef::new("session-patch-active", "turn-existing");
    let message = crate::turn::completed_agent_user_input(
        &identity.turn_id,
        "message-existing",
        crate::test_support::test_user_input("active"),
        1,
    )
    .expect("canonical User message should be valid");
    crate::test_support::commit_test_running_turn(store.as_ref(), identity, message).await;

    let error = service
        .patch_session(&actor, "session-patch-active", SessionPatch::default())
        .await
        .expect_err("active session patch should be rejected");

    assert!(error.is_code(talos_error::ErrorCode::ActiveTurnRunning));
}
