use super::*;

fn adapter(home: &std::path::Path) -> AgentHarnessRpcAdapter {
    let adapter = AgentHarnessRpcAdapter::new(
        "embedded".into(),
        Arc::new(std::sync::RwLock::new(
            Config::default()
                .with_runtime_paths(talos_config::TalosPaths::from_home(home.to_owned())),
        )),
        vec![],
    );
    adapter
        .install_node_registration(
            "computer-1",
            Registration {
                lease_token: "remote-lease".into(),
                owner_user_id: "owner".into(),
                validity: agent_protocol::LeaseValidity {
                    server_at: 1,
                    expires_at: 91,
                },
            },
            Instant::now() + Duration::from_secs(90),
        )
        .unwrap();
    adapter
}
fn request(blocked: bool) -> pb::SetRegistrationAdmissionRequest {
    pb::SetRegistrationAdmissionRequest {
        auth: Some(pb::NodeAuth {
            node_id: "computer-1".into(),
            token: "remote-lease".into(),
        }),
        blocked,
    }
}
#[test]
fn deletion_blocks_remote_credentials_and_stale_admission_without_revoking_local_auth() {
    let home = tempfile::tempdir().unwrap();
    let adapter = adapter(home.path());
    let remote = NodeAuth {
        node_id: "computer-1".into(),
        token: "remote-lease".into(),
    };
    let previously_authorized = adapter.authorize(&remote).unwrap();
    let local = NodeAuth {
        node_id: "embedded".into(),
        token: "private-loopback".into(),
    };
    adapter.install_local_auth(&local).unwrap();
    adapter
        .registration_admission(request(/* blocked */ true))
        .unwrap();
    assert!(adapter.authorize(&remote).is_err());
    assert!(adapter.authorize_admission(&previously_authorized).is_err());
    assert!(adapter.authorize(&local).is_ok());
    assert!(adapter.validate_connection_auth(&remote).is_ok());
    adapter
        .registration_admission(request(/* blocked */ false))
        .unwrap();
    assert!(adapter.authorize(&remote).is_ok());
}
#[test]
fn waiting_turn_marker_prevents_registration_deletion_after_worker_restart() {
    let home = tempfile::tempdir().unwrap();
    let adapter = adapter(home.path());
    let context = agent_protocol::AgentSessionContext {
        agent_id: "default".into(),
        session_id: "waiting".into(),
        workspace: talos_config::EffectiveWorkspace {
            workspace_id: "default".into(),
            user_id: "owner".into(),
            root: home.path().to_owned(),
        },
    };
    let identity = talos_types::AgentTurnRef::new("waiting", "turn-1");
    let mut reservation = adapter
        .session_gate
        .reserve(
            &context,
            &identity,
            &uuid::Uuid::new_v4().to_string(),
            &Default::default(),
            session_gate::RunAdmission::NewTurn,
        )
        .unwrap();
    reservation.consuming(&identity).unwrap();
    adapter.session_gate.cleaned(&identity).unwrap();
    drop(reservation);
    let restarted = self::adapter(home.path());
    assert!(restarted
        .registration_admission(request(/* blocked */ true))
        .unwrap_err()
        .is_code(ErrorCode::ActiveTurnRunning));
}
