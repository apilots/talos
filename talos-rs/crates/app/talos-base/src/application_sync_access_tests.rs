//! A replicated display view never grants authority to delete remote execution.
use super::*;
use talos_coordinator::AgentCoordinator;

pub(super) async fn verify_unconnected_remote_deletion(
    a: &TalosApplicationRuntime,
    b: &TalosApplicationRuntime,
    actor: &talos_service::Actor,
) {
    let computer = &b.config.agent_node.runner.as_ref().unwrap().node_id;
    b.service
        .start_turn(
            actor,
            talos_types::talos::SessionSource::http(),
            StartTurnRequest {
                node_id: Some(computer.clone()),
                ..start_request("cold-remote-delete", "delete from a cold management peer")
            },
            None,
        )
        .await
        .unwrap();
    wait_for("cold delete source History Shared", async || {
        worker_state(b, "cold-remote-delete").await.handoff_ready
    })
    .await;
    let request = talos_types::SessionHistoryRequest {
        session_id: "cold-remote-delete".into(),
        cursor: None,
        limit: None,
    };
    wait_for("cold delete History imported", async || {
        a.service
            .get_session_history(actor, request.clone())
            .await
            .is_ok_and(|history| !history.data.is_empty())
    })
    .await;
    let session = a
        .service
        .get_session(actor, "cold-remote-delete")
        .await
        .unwrap();
    assert_eq!(&session.node_id, computer);
    let listing = a
        .service
        .list_sessions(
            actor,
            talos_types::SessionListRequest {
                node_id: Some(computer.clone()),
                limit: None,
                offset: None,
                project_id: None,
            },
        )
        .await
        .unwrap();
    assert!(listing
        .sessions
        .iter()
        .any(|entry| entry.session.id == "cold-remote-delete"));
    assert!(
        a.coordinator
            .worker_environment(computer, &actor.user_id)
            .await
            .is_err(),
        "Sync display facts must not publish a remote execution runtime"
    );
    let error = tokio::time::timeout(
        Duration::from_secs(3),
        a.service.delete_session(actor, "cold-remote-delete"),
    )
    .await
    .expect("unsupported remote deletion must remain bounded")
    .unwrap_err();
    assert!(matches!(
        error.error_code(),
        talos_error::ErrorCode::ServiceUnavailable | talos_error::ErrorCode::InvalidRequest
    ));
    assert!(a
        .session_store
        .get_session_record("cold-remote-delete")
        .await
        .unwrap()
        .is_some());
    assert!(b
        .session_store
        .get_session_record("cold-remote-delete")
        .await
        .unwrap()
        .is_some());
}
