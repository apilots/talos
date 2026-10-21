//! Cold remote deletion must establish execution access only during bounded preflight.
use super::*;

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
        .iter()
        .any(|session| session.id == "cold-remote-delete"));
    assert!(
        !a.sync_host
            .sync_stores
            .connections
            .lock()
            .await
            .contains_key(computer),
        "metadata and History must not open remote execution access"
    );
    assert!(tokio::time::timeout(
        Duration::from_secs(3),
        a.service.delete_session(actor, "cold-remote-delete")
    )
    .await
    .expect("cold remote deletion must remain bounded")
    .unwrap());
    wait_for("cold deletion returned to source", async || {
        b.session_store
            .get_session_record("cold-remote-delete")
            .await
            .unwrap()
            .is_none()
    })
    .await;
}
