use super::*;
use talos_types::AgentTurnRef;

#[test]
fn other_owner_gets_a_typed_refusal_until_the_session_is_released() {
    let root = tempfile::tempdir().unwrap();
    let first = SessionOwner::new(root.path(), SessionOwnerKind::Tui, SessionHold::Attached);
    let second = SessionOwner::new(root.path(), SessionOwnerKind::Server, SessionHold::Turn);
    first.claim_open("session-a").unwrap();
    let Err(error) = second.pin("session-a") else {
        panic!("the attached session must remain locked");
    };
    assert!(error.is_code(ErrorCode::SessionHeld));
    assert!(matches!(
        error.to_api_error().localization,
        Some(ErrorLocalization::SessionHeld { session_id, owner_kind: SessionOwnerKind::Tui, pid, .. })
            if session_id == "session-a" && pid == u64::from(std::process::id())
    ));
    drop(first);
    assert!(second.pin("session-a").is_ok());
}

#[test]
fn settling_a_turn_does_not_release_a_new_admission() {
    let root = tempfile::tempdir().unwrap();
    let owner = SessionOwner::new(root.path(), SessionOwnerKind::Server, SessionHold::Turn);
    let other = SessionOwner::new(root.path(), SessionOwnerKind::Server, SessionHold::Turn);
    let old = AgentTurnRef::new("session", "old");
    let new = AgentTurnRef::new("session", "new");
    let old_admission = owner.pin("session").unwrap();
    old_admission.retain_turn(&old.turn_id);
    drop(old_admission);
    let new_admission = owner.pin("session").unwrap();
    owner.release_turn(&old);
    assert!(other.pin("session").is_err());
    new_admission.retain_turn(&new.turn_id);
    drop(new_admission);
    owner.release_turn(&old);
    assert!(other.pin("session").is_err());
    owner.release_turn(&new);
    assert!(other.pin("session").is_ok());
}

#[test]
fn scan_scope_distinguishes_resident_foreground_and_one_shot_commands() {
    let root = tempfile::tempdir().unwrap();
    let server = SessionOwner::new(root.path(), SessionOwnerKind::Server, SessionHold::Turn);
    let exec = SessionOwner::new(root.path(), SessionOwnerKind::Exec, SessionHold::Process);
    let foreground = SessionOwner::new(root.path(), SessionOwnerKind::Cli, SessionHold::Process);
    let tui = SessionOwner::new(root.path(), SessionOwnerKind::Tui, SessionHold::Attached);
    exec.claim_open("exec").unwrap();
    foreground.claim_open("foreground").unwrap();
    tui.claim_open("tui").unwrap();
    assert!(exec.try_for_scan("exec").unwrap().is_none());
    assert!(exec.try_for_scan("free").unwrap().is_none());
    assert!(foreground.try_for_scan("free").unwrap().is_none());
    assert!(foreground.try_for_scan("foreground").unwrap().is_some());
    assert!(tui.try_for_scan("free").unwrap().is_none());
    let permit = tui.try_for_scan("tui").unwrap().unwrap();
    assert!(tui.claim_open("other").is_err());
    drop(permit);
    tui.claim_open("other").unwrap();
    assert!(server.try_for_scan("foreground").unwrap().is_none());
    assert!(server.try_for_scan("free").unwrap().is_some());
    assert!(server.try_for_scan("tui").unwrap().is_some());
}
