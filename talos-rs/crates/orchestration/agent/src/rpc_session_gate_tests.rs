use super::*;
fn context() -> AgentSessionContext {
    AgentSessionContext {
        agent_id: "kernel".into(),
        session_id: "session".into(),
        workspace: talos_config::EffectiveWorkspace {
            workspace_id: "pc".into(),
            user_id: "user".into(),
            root: "/worker/files".into(),
        },
    }
}
fn terminal(identity: &AgentTurnRef) -> SessionHistoryCursor {
    SessionHistoryCursor {
        history_generation: 0,
        completed_turn_id: Some(identity.turn_id.clone()),
        completed_event_sequence: 5,
    }
}

#[test]
fn every_ingress_shares_the_same_session_reservation() {
    let home = tempfile::tempdir().unwrap();
    let gate = SessionGate::new(home.path().join("gate"));
    let manager = uuid::Uuid::new_v4().to_string();
    let scope = context();
    let identity = AgentTurnRef::new("session", "turn");
    let _reserved = gate
        .reserve(
            &scope,
            &identity,
            &manager,
            &Default::default(),
            RunAdmission::NewTurn,
        )
        .unwrap();
    assert!(gate
        .reserve(
            &scope,
            &identity,
            &manager,
            &Default::default(),
            RunAdmission::NewTurn
        )
        .unwrap_err()
        .is_code(ErrorCode::ActiveTurnRunning));
}

#[test]
fn manager_transfer_requires_worker_terminal_settlement_and_idle_marker() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join("gate");
    let gate = SessionGate::new(root.clone());
    let a = uuid::Uuid::new_v4().to_string();
    let b = uuid::Uuid::new_v4().to_string();
    let scope = context();
    let identity = AgentTurnRef::new("session", "first");
    let mut reservation = gate
        .reserve(
            &scope,
            &identity,
            &a,
            &Default::default(),
            RunAdmission::NewTurn,
        )
        .unwrap();
    reservation.consuming(&identity).unwrap();
    gate.cleaned(&identity).unwrap();
    drop(reservation);
    let next = AgentTurnRef::new("session", "next");
    assert!(gate
        .reserve(
            &scope,
            &next,
            &b,
            &Default::default(),
            RunAdmission::NewTurn
        )
        .is_err());
    gate.settled(&scope, &identity, terminal(&identity))
        .unwrap();
    let reopened = SessionGate::new(root);
    assert!(reopened.read(&scope).unwrap().handoff_ready);
    assert!(reopened
        .reserve(
            &scope,
            &next,
            &b,
            &terminal(&identity),
            RunAdmission::NewTurn
        )
        .is_ok());
}

#[test]
fn rejected_admission_restores_idle_without_releasing_another_reservation() {
    let home = tempfile::tempdir().unwrap();
    let gate = SessionGate::new(home.path().join("gate"));
    let manager = uuid::Uuid::new_v4().to_string();
    let scope = context();
    let identity = AgentTurnRef::new("session", "turn");
    let reservation = gate
        .reserve(
            &scope,
            &identity,
            &manager,
            &Default::default(),
            RunAdmission::NewTurn,
        )
        .unwrap();
    drop(reservation);
    assert!(gate.read(&scope).unwrap().active_turn.is_none());
    assert!(gate
        .reserve(
            &scope,
            &identity,
            &manager,
            &Default::default(),
            RunAdmission::NewTurn
        )
        .is_ok());
}

#[test]
fn generation_reset_is_idempotent_and_rejects_old_terminal_facts() {
    let home = tempfile::tempdir().unwrap();
    let gate = SessionGate::new(home.path().join("gate"));
    let manager = uuid::Uuid::new_v4().to_string();
    let scope = context();
    let identity = AgentTurnRef::new("session", "turn");
    let mut reservation = gate
        .reserve(
            &scope,
            &identity,
            &manager,
            &Default::default(),
            RunAdmission::NewTurn,
        )
        .unwrap();
    reservation.consuming(&identity).unwrap();
    gate.cleaned(&identity).unwrap();
    drop(reservation);
    gate.settled(&scope, &identity, terminal(&identity))
        .unwrap();
    let cleared = SessionHistoryCursor {
        history_generation: 1,
        ..Default::default()
    };
    gate.reset_cursor(&scope, cleared.clone()).unwrap();
    gate.reset_cursor(&scope, cleared).unwrap();
    assert!(gate
        .settled(&scope, &identity, terminal(&identity))
        .is_err());
}

#[test]
fn context_binding_and_configuration_exclusion_are_shared() {
    let home = tempfile::tempdir().unwrap();
    let gate = SessionGate::new(home.path().join("gate"));
    let manager = uuid::Uuid::new_v4().to_string();
    let scope = context();
    let identity = AgentTurnRef::new("session", "turn");
    let reservation = gate
        .reserve(
            &scope,
            &identity,
            &manager,
            &Default::default(),
            RunAdmission::NewTurn,
        )
        .unwrap();
    assert!(gate.reserve_configuration().is_err());
    drop(reservation);
    let configuration = gate.reserve_configuration().unwrap();
    assert!(gate
        .reserve(
            &scope,
            &identity,
            &manager,
            &Default::default(),
            RunAdmission::NewTurn
        )
        .is_err());
    drop(configuration);
}
