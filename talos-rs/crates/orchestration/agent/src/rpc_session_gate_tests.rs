use super::*;
fn context(id: &str) -> AgentSessionContext {
    AgentSessionContext {
        agent_id: "kernel".into(),
        session_id: id.into(),
        workspace: talos_config::EffectiveWorkspace {
            workspace_id: "pc".into(),
            user_id: "user".into(),
            root: "/worker/files".into(),
        },
    }
}
#[test]
fn restart_keeps_last_local_coordinator_and_shared_handoff_requires_the_exact_history_cursor() {
    let home = tempfile::tempdir().unwrap();
    let gate = SessionGate::new(home.path().join("history-cursors"));
    let a = uuid::Uuid::new_v4().to_string();
    let b = uuid::Uuid::new_v4().to_string();
    let context = context("session");
    let identity = AgentTurnRef::new("session", "turn-a");
    let mut admitted = gate
        .reserve(
            &context,
            &identity,
            &a,
            &Default::default(),
            RunAdmission::NewTurn,
        )
        .unwrap();
    assert!(gate
        .reserve(
            &context,
            &AgentTurnRef::new("session", "turn-b"),
            &b,
            &Default::default(),
            RunAdmission::NewTurn
        )
        .unwrap_err()
        .is_code(ErrorCode::ActiveTurnRunning));
    admitted.consuming(&identity).unwrap();
    gate.cleaned(&identity).unwrap();
    drop(admitted);
    let history_cursor = SessionHistoryCursor {
        completed_turn_id: Some("turn-a".into()),
        completed_event_sequence: 5,
        ..Default::default()
    };
    gate.commit(
        &context,
        &a,
        Some(&identity),
        history_cursor.clone(),
        SessionStateVisibility::Local,
        HistoryBootstrap::Forbidden,
    )
    .unwrap();
    let restarted = SessionGate::new(home.path().join("history-cursors"));
    assert_eq!(
        restarted.read(&context).unwrap().history_cursor,
        history_cursor
    );
    assert!(!restarted.read(&context).unwrap().handoff_ready);
    assert!(restarted
        .reserve(
            &context,
            &AgentTurnRef::new("session", "turn-b"),
            &b,
            &history_cursor,
            RunAdmission::NewTurn
        )
        .is_err());
    // Same coordinator continues offline; no hub or original creator is consulted.
    drop(
        restarted
            .reserve(
                &context,
                &AgentTurnRef::new("session", "turn-a2"),
                &a,
                &history_cursor,
                RunAdmission::NewTurn,
            )
            .unwrap(),
    );
    restarted
        .commit(
            &context,
            &a,
            Some(&identity),
            history_cursor.clone(),
            SessionStateVisibility::Shared,
            HistoryBootstrap::Forbidden,
        )
        .unwrap();
    assert!(restarted.read(&context).unwrap().handoff_ready);
    assert!(restarted
        .reserve(
            &context,
            &AgentTurnRef::new("session", "turn-b"),
            &b,
            &Default::default(),
            RunAdmission::NewTurn
        )
        .is_err());
    let mut changed_binding = context.clone();
    changed_binding.agent_id = "another-harness".into();
    assert!(restarted
        .reserve(
            &changed_binding,
            &AgentTurnRef::new("session", "turn-b"),
            &b,
            &history_cursor,
            RunAdmission::NewTurn
        )
        .is_err());
    drop(
        restarted
            .reserve(
                &context,
                &AgentTurnRef::new("session", "turn-b"),
                &b,
                &history_cursor,
                RunAdmission::NewTurn,
            )
            .unwrap(),
    );
    assert_eq!(
        restarted.read(&context).unwrap().coordinator_id.as_deref(),
        Some(b.as_str())
    );
}
#[test]
fn unconfirmed_restart_blocks_only_that_session_and_cannot_be_overwritten_by_an_old_store() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join("history-cursors");
    let gate = SessionGate::new(root.clone());
    let a = uuid::Uuid::new_v4().to_string();
    let bound = context("session");
    let identity = AgentTurnRef::new("session", "turn");
    let mut admitted = gate
        .reserve(
            &bound,
            &identity,
            &a,
            &Default::default(),
            RunAdmission::NewTurn,
        )
        .unwrap();
    admitted.consuming(&identity).unwrap();
    drop(admitted);
    let restarted = SessionGate::new(root);
    assert!(restarted
        .reserve(
            &bound,
            &identity,
            &a,
            &Default::default(),
            RunAdmission::Continuation
        )
        .unwrap_err()
        .is_code(ErrorCode::RunCleanupUnconfirmed));
    assert!(restarted
        .commit(
            &bound,
            &a,
            Some(&identity),
            Default::default(),
            SessionStateVisibility::Local,
            HistoryBootstrap::TrustedLocal
        )
        .is_err());
    drop(
        restarted
            .reserve(
                &context("new-session"),
                &AgentTurnRef::new("new-session", "turn"),
                &a,
                &Default::default(),
                RunAdmission::NewTurn,
            )
            .unwrap(),
    );
    std::fs::write(restarted.path("corrupt"), "invalid marker").unwrap();
    assert!(restarted.read(&context("corrupt")).is_err());
}

#[test]
fn configuration_excludes_waiting_recovered_sessions_and_blocks_new_admission() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join("history-cursors");
    let gate = SessionGate::new(root.clone());
    let peer = uuid::Uuid::new_v4().to_string();
    let session = context("waiting");
    let identity = AgentTurnRef::new("waiting", "turn");
    let mut run = gate
        .reserve(
            &session,
            &identity,
            &peer,
            &Default::default(),
            RunAdmission::NewTurn,
        )
        .unwrap();
    run.consuming(&identity).unwrap();
    gate.cleaned(&identity).unwrap();
    drop(run);
    assert!(gate
        .reserve_configuration()
        .unwrap_err()
        .is_code(ErrorCode::ActiveTurnRunning));
    let before = gate.read(&session).unwrap();
    drop(
        gate.reserve(
            &session,
            &identity,
            &peer,
            &Default::default(),
            RunAdmission::Continuation,
        )
        .unwrap(),
    );
    assert_eq!(gate.read(&session).unwrap(), before);
    let restarted = SessionGate::new(root);
    assert!(restarted
        .reserve_configuration()
        .unwrap_err()
        .is_code(ErrorCode::ActiveTurnRunning));
    restarted
        .commit(
            &session,
            &peer,
            Some(&identity),
            SessionHistoryCursor {
                completed_turn_id: Some("turn".into()),
                completed_event_sequence: 3,
                ..Default::default()
            },
            SessionStateVisibility::Local,
            HistoryBootstrap::Forbidden,
        )
        .unwrap();
    let config = restarted.reserve_configuration().unwrap();
    assert!(restarted
        .reserve(
            &context("another"),
            &AgentTurnRef::new("another", "next"),
            &peer,
            &Default::default(),
            RunAdmission::NewTurn
        )
        .unwrap_err()
        .is_code(ErrorCode::ActiveTurnRunning));
    drop(config);
    assert!(restarted.reserve_configuration().is_ok());
}

#[test]
fn stale_maintenance_cannot_take_over_after_another_peer_publishes_a_new_history_cursor() {
    let home = tempfile::tempdir().unwrap();
    let gate = SessionGate::new(home.path().join("history-cursors"));
    let a = uuid::Uuid::new_v4().to_string();
    let b = uuid::Uuid::new_v4().to_string();
    let session = context("clear-race");
    let stale = gate.read(&session).unwrap().history_cursor;
    let identity = AgentTurnRef::new("clear-race", "new-turn");
    let mut run = gate
        .reserve(&session, &identity, &a, &stale, RunAdmission::NewTurn)
        .unwrap();
    run.consuming(&identity).unwrap();
    gate.cleaned(&identity).unwrap();
    drop(run);
    let latest = SessionHistoryCursor {
        completed_turn_id: Some(identity.turn_id.clone()),
        completed_event_sequence: 5,
        ..Default::default()
    };
    gate.commit(
        &session,
        &a,
        Some(&identity),
        latest.clone(),
        SessionStateVisibility::Local,
        HistoryBootstrap::Forbidden,
    )
    .unwrap();
    gate.commit(
        &session,
        &a,
        None,
        latest.clone(),
        SessionStateVisibility::Shared,
        HistoryBootstrap::Forbidden,
    )
    .unwrap();
    let before = gate.read(&session).unwrap();
    assert!(gate
        .maintenance(&session, &b, &stale)
        .unwrap_err()
        .is_code(ErrorCode::Conflict));
    assert_eq!(gate.read(&session).unwrap(), before);
    let maintenance = gate.maintenance(&session, &b, &latest).unwrap();
    assert!(gate
        .reserve(
            &session,
            &AgentTurnRef::new("clear-race", "concurrent"),
            &a,
            &latest,
            RunAdmission::NewTurn
        )
        .unwrap_err()
        .is_code(ErrorCode::ActiveTurnRunning));
    drop(maintenance);
    let reset = SessionHistoryCursor {
        history_generation: 1,
        ..Default::default()
    };
    gate.commit(
        &session,
        &b,
        None,
        reset.clone(),
        SessionStateVisibility::Local,
        HistoryBootstrap::Forbidden,
    )
    .unwrap();
    assert_eq!(gate.read(&session).unwrap().history_cursor, reset);
}
