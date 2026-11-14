//! Per-session exclusive ownership for the worker that writes the session.
//!
//! The OS file lock is the source of truth. File contents exist only so a
//! refused caller can say who holds the session. The kernel releases the lock
//! when this process dies.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use talos_error::{ErrorCode, ErrorLocalization, SessionOwnerKind, TalosError, TalosResult};
use talos_fs::{std_fs, FileLock, FileSystem};

/// How long a successful claim stays with this process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionHold {
    /// One open session until switch or process exit. TUI and interactive CLI.
    Attached,
    /// Hold until this process exits. `exec`, `goal start`, and `loop start`.
    Process,
    /// Hold from admission through terminal settlement. Server, Desktop, SDK.
    Turn,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LockNote {
    owner_kind: String,
    pid: u64,
    host: String,
    since: String,
}

struct HeldSession {
    /// The kernel releases this descriptor, and the exclusive lock, on drop.
    #[allow(dead_code)]
    lock: FileLock,
    pins: u32,
    release_when_unpinned: bool,
}

struct State {
    held: HashMap<String, HeldSession>,
    open_session: Option<String>,
}

struct Inner {
    root: PathBuf,
    kind: SessionOwnerKind,
    hold: SessionHold,
    state: Mutex<State>,
}

/// Process-local table of session file locks. Clone shares the same table.
#[derive(Clone)]
pub struct SessionOwner {
    inner: Arc<Inner>,
}

/// Keeps a hold across a nested terminal settlement, such as dead-owner
/// recovery immediately followed by a new turn.
pub struct SessionAdmissionPin {
    owner: SessionOwner,
    session_id: String,
}

/// Scan admission. Dropping it does not release a turn that dispatch started.
pub struct SessionScanPermit {
    owner: SessionOwner,
    session_id: String,
    release_on_drop: bool,
    created: bool,
}

/// Releases a hold created by this start if admission does not succeed.
pub struct SessionStartGuard {
    owner: SessionOwner,
    session_id: String,
    release_on_drop: bool,
}

impl SessionOwner {
    pub fn new(root: impl Into<PathBuf>, kind: SessionOwnerKind, hold: SessionHold) -> Self {
        Self {
            inner: Arc::new(Inner {
                root: root.into(),
                kind,
                hold,
                state: Mutex::new(State {
                    held: HashMap::new(),
                    open_session: None,
                }),
            }),
        }
    }

    pub fn hold(&self) -> SessionHold {
        self.inner.hold
    }

    pub fn kind(&self) -> SessionOwnerKind {
        self.inner.kind
    }

    pub fn root(&self) -> &Path {
        &self.inner.root
    }

    pub fn holds(&self, session_id: &str) -> bool {
        self.inner
            .state
            .lock()
            .expect("session owner lock")
            .held
            .contains_key(session_id)
    }

    pub fn held_sessions(&self) -> Vec<String> {
        self.inner
            .state
            .lock()
            .expect("session owner lock")
            .held
            .keys()
            .cloned()
            .collect()
    }

    pub fn open_session(&self) -> Option<String> {
        self.inner
            .state
            .lock()
            .expect("session owner lock")
            .open_session
            .clone()
    }

    /// Take the open-session hold. `Turn` does not lock merely because a
    /// session was viewed. `retain_others` keeps locks for sessions that still
    /// have a live turn; the new session is then refused rather than stolen.
    pub fn claim_open(&self, session_id: &str, retain_others: bool) -> TalosResult<()> {
        match self.inner.hold {
            SessionHold::Turn => Ok(()),
            SessionHold::Process => self.acquire(session_id),
            SessionHold::Attached => self.claim_attached(session_id, retain_others),
        }
    }

    /// Lock the session when this process does not already hold it.
    pub fn acquire(&self, session_id: &str) -> TalosResult<()> {
        self.ensure_held(session_id, false, true).map(|_| ())
    }

    /// Admission pin. The matching drop does not release a follow-up turn.
    pub fn pin(&self, session_id: &str) -> TalosResult<SessionAdmissionPin> {
        self.ensure_held(session_id, true, true)?;
        Ok(SessionAdmissionPin {
            owner: self.clone(),
            session_id: session_id.to_owned(),
        })
    }

    /// Start guard. It releases only a hold this call created, and only when
    /// admission does not keep the turn.
    pub fn guard_start(&self, session_id: &str) -> TalosResult<SessionStartGuard> {
        let created = self.ensure_held(session_id, false, true)?;
        Ok(SessionStartGuard {
            owner: self.clone(),
            session_id: session_id.to_owned(),
            release_on_drop: created,
        })
    }

    /// `Ok(None)` means another process holds the session. The scan must not
    /// clear `next_due_at` in that case. Every hold policy may take a free lock.
    pub fn try_for_scan(&self, session_id: &str) -> TalosResult<Option<SessionScanPermit>> {
        if self.holds(session_id) {
            self.adjust_pins(session_id, 1)?;
            return Ok(Some(SessionScanPermit {
                owner: self.clone(),
                session_id: session_id.to_owned(),
                release_on_drop: false,
                created: false,
            }));
        }
        match self.ensure_held(session_id, true, false) {
            Ok(_) => Ok(Some(SessionScanPermit {
                owner: self.clone(),
                session_id: session_id.to_owned(),
                release_on_drop: false,
                created: true,
            })),
            Err(error) if error.is_code(ErrorCode::SessionHeld) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// A logical turn reached a terminal fact, or a turn-scoped claim did not
    /// leave one. Attached open sessions and process holds stay.
    pub fn on_terminal(&self, session_id: &str) {
        if self.keeps_after_terminal(session_id) {
            return;
        }
        let mut state = self.inner.state.lock().expect("session owner lock");
        let Some(held) = state.held.get_mut(session_id) else {
            return;
        };
        if held.pins > 0 {
            held.release_when_unpinned = true;
            return;
        }
        state.held.remove(session_id);
    }

    /// The new turn owns the lock; a nested settlement must not drop it.
    pub fn keep_for_follow_up(&self, session_id: &str) {
        if let Some(held) = self
            .inner
            .state
            .lock()
            .expect("session owner lock")
            .held
            .get_mut(session_id)
        {
            held.release_when_unpinned = false;
        }
    }

    fn claim_attached(&self, session_id: &str, retain_others: bool) -> TalosResult<()> {
        if retain_others && !self.holds(session_id) {
            let others = self.held_sessions().into_iter().any(|id| id != session_id);
            if others {
                return Err(TalosError::new(
                    ErrorCode::ActiveTurnRunning,
                    "Finish the active turn before switching sessions.",
                ));
            }
        }
        self.ensure_held(session_id, false, true)?;
        let mut state = self.inner.state.lock().expect("session owner lock");
        state.open_session = Some(session_id.to_owned());
        if !retain_others {
            state
                .held
                .retain(|id, held| id == session_id || held.pins > 0);
        }
        Ok(())
    }

    fn ensure_held(&self, session_id: &str, pin: bool, become_open: bool) -> TalosResult<bool> {
        if session_id.is_empty() {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Session identity is invalid",
            ));
        }
        let mut state = self.inner.state.lock().expect("session owner lock");
        if let Some(held) = state.held.get_mut(session_id) {
            if pin {
                held.pins = held.pins.saturating_add(1);
            }
            if become_open && self.inner.hold != SessionHold::Turn && state.open_session.is_none() {
                state.open_session = Some(session_id.to_owned());
            }
            return Ok(false);
        }
        let path = self.lock_path(session_id);
        let Some(lock) = std_fs().try_file_lock(&path)? else {
            return Err(session_held(session_id, &path));
        };
        let note = LockNote {
            owner_kind: self.inner.kind.as_str().to_owned(),
            pid: u64::from(std::process::id()),
            host: current_host(),
            since: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        };
        if let Ok(bytes) = serde_json::to_vec(&note) {
            let _ = lock.overwrite(&bytes);
        }
        state.held.insert(
            session_id.to_owned(),
            HeldSession {
                lock,
                pins: u32::from(pin),
                release_when_unpinned: false,
            },
        );
        if become_open && self.inner.hold != SessionHold::Turn && state.open_session.is_none() {
            state.open_session = Some(session_id.to_owned());
        }
        Ok(true)
    }

    fn adjust_pins(&self, session_id: &str, delta: i32) -> TalosResult<()> {
        let mut state = self.inner.state.lock().expect("session owner lock");
        let Some(held) = state.held.get_mut(session_id) else {
            return Ok(());
        };
        if delta > 0 {
            held.pins = held.pins.saturating_add(delta as u32);
            return Ok(());
        }
        held.pins = held.pins.saturating_sub(delta.unsigned_abs());
        let release = held.pins == 0
            && held.release_when_unpinned
            && !policy_keeps(self.inner.hold, &state.open_session, session_id);
        if release {
            state.held.remove(session_id);
        }
        Ok(())
    }

    fn drop_created_scan_hold(&self, session_id: &str) {
        let mut state = self.inner.state.lock().expect("session owner lock");
        let Some(held) = state.held.get_mut(session_id) else {
            return;
        };
        held.pins = held.pins.saturating_sub(1);
        if held.pins == 0 {
            state.held.remove(session_id);
        }
    }

    fn keeps_after_terminal(&self, session_id: &str) -> bool {
        let state = self.inner.state.lock().expect("session owner lock");
        policy_keeps(self.inner.hold, &state.open_session, session_id)
    }

    fn lock_path(&self, session_id: &str) -> PathBuf {
        self.inner
            .root
            .join(format!("{:x}.lock", Sha256::digest(session_id.as_bytes())))
    }
}

impl SessionAdmissionPin {
    fn release(&mut self) {
        let _ = self.owner.adjust_pins(&self.session_id, -1);
        self.session_id.clear();
    }
}

impl Drop for SessionAdmissionPin {
    fn drop(&mut self) {
        self.release();
    }
}

impl SessionScanPermit {
    /// Release a lock this scan created when dispatch did not leave a turn.
    /// A session this process already held stays with its hold policy.
    pub fn release_if_idle(&mut self) {
        if self.created {
            self.release_on_drop = true;
        }
    }
}

impl Drop for SessionScanPermit {
    fn drop(&mut self) {
        if self.session_id.is_empty() {
            return;
        }
        if self.release_on_drop {
            self.owner.drop_created_scan_hold(&self.session_id);
        } else {
            let _ = self.owner.adjust_pins(&self.session_id, -1);
        }
        self.session_id.clear();
    }
}

impl SessionStartGuard {
    /// Admission succeeded. The logical turn keeps the hold until it settles.
    pub fn keep(mut self) {
        self.release_on_drop = false;
    }
}

impl Drop for SessionStartGuard {
    fn drop(&mut self) {
        if self.release_on_drop {
            self.owner.on_terminal(&self.session_id);
        }
    }
}

fn policy_keeps(hold: SessionHold, open: &Option<String>, session_id: &str) -> bool {
    match hold {
        SessionHold::Process => true,
        SessionHold::Attached => open.as_deref() == Some(session_id),
        SessionHold::Turn => false,
    }
}

fn session_held(session_id: &str, path: &Path) -> TalosError {
    let note = std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<LockNote>(&bytes).ok());
    let (owner_kind, pid, host, since) = note
        .map(|note| {
            (
                parse_kind(&note.owner_kind),
                note.pid,
                note.host,
                note.since,
            )
        })
        .unwrap_or((SessionOwnerKind::Unknown, 0, String::new(), String::new()));
    TalosError::localized(ErrorLocalization::SessionHeld {
        session_id: session_id.to_owned(),
        owner_kind,
        pid,
        host,
        since,
    })
}

fn parse_kind(value: &str) -> SessionOwnerKind {
    match value {
        "tui" => SessionOwnerKind::Tui,
        "exec" => SessionOwnerKind::Exec,
        "cli" => SessionOwnerKind::Cli,
        "server" => SessionOwnerKind::Server,
        "desktop" => SessionOwnerKind::Desktop,
        _ => SessionOwnerKind::Unknown,
    }
}

fn current_host() -> String {
    let mut buffer = [0u8; 256];
    let rc = unsafe { libc::gethostname(buffer.as_mut_ptr().cast(), buffer.len()) };
    if rc != 0 {
        return "localhost".to_owned();
    }
    let end = buffer
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(buffer.len());
    let host = String::from_utf8_lossy(&buffer[..end]).trim().to_owned();
    if host.is_empty() {
        "localhost".to_owned()
    } else {
        host
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owner(root: &Path, hold: SessionHold) -> SessionOwner {
        SessionOwner::new(root, SessionOwnerKind::Tui, hold)
    }

    #[test]
    fn a_second_process_table_is_refused_with_the_holder_note() {
        let root = tempfile::tempdir().unwrap();
        let first = owner(root.path(), SessionHold::Attached);
        first.claim_open("session-a", false).unwrap();
        let second = owner(root.path(), SessionHold::Process);
        let error = second.acquire("session-a").unwrap_err();
        assert!(error.is_code(ErrorCode::SessionHeld));
        let localization = error.to_api_error().localization.unwrap();
        match localization {
            ErrorLocalization::SessionHeld {
                session_id,
                owner_kind,
                pid,
                ..
            } => {
                assert_eq!(session_id, "session-a");
                assert_eq!(owner_kind, SessionOwnerKind::Tui);
                assert_eq!(pid, u64::from(std::process::id()));
            }
            other => panic!("unexpected localization {other:?}"),
        }
        assert!(error.to_api_error().message.contains("session-a"));
        assert!(error.to_api_error().message.contains("tui"));
    }

    #[test]
    fn dropping_the_holder_lets_the_next_owner_acquire() {
        let root = tempfile::tempdir().unwrap();
        let first = owner(root.path(), SessionHold::Turn);
        first.acquire("session-a").unwrap();
        drop(first);
        let second = owner(root.path(), SessionHold::Turn);
        second.acquire("session-a").unwrap();
        assert!(second.holds("session-a"));
    }

    #[test]
    fn the_same_table_does_not_lock_twice() {
        let root = tempfile::tempdir().unwrap();
        let first = owner(root.path(), SessionHold::Turn);
        first.acquire("session-a").unwrap();
        let again = first.clone();
        again.acquire("session-a").unwrap();
        assert!(again.holds("session-a"));
    }

    #[test]
    fn turn_settlement_releases_and_attached_open_keeps() {
        let root = tempfile::tempdir().unwrap();
        let turn = owner(root.path(), SessionHold::Turn);
        turn.acquire("session-a").unwrap();
        turn.on_terminal("session-a");
        assert!(!turn.holds("session-a"));

        let attached = owner(root.path(), SessionHold::Attached);
        attached.claim_open("session-a", false).unwrap();
        attached.on_terminal("session-a");
        assert!(attached.holds("session-a"));
        assert_eq!(attached.open_session().as_deref(), Some("session-a"));
    }

    #[test]
    fn any_hold_scans_a_free_session_and_skips_one_another_process_holds() {
        let root = tempfile::tempdir().unwrap();
        let holder = owner(root.path(), SessionHold::Process);
        holder.acquire("session-a").unwrap();
        let exec = owner(root.path(), SessionHold::Process);
        assert!(exec.try_for_scan("session-a").unwrap().is_none());
        let mut permit = exec.try_for_scan("other-session").unwrap().unwrap();
        assert!(exec.holds("other-session"));
        permit.release_if_idle();
        drop(permit);
        assert!(!exec.holds("other-session"));
        assert!(holder.holds("session-a"));

        let attached = owner(root.path(), SessionHold::Attached);
        assert!(attached.try_for_scan("session-a").unwrap().is_none());
        let mut permit = attached.try_for_scan("session-b").unwrap().unwrap();
        permit.release_if_idle();
        drop(permit);
        assert!(!attached.holds("session-b"));
    }

    #[test]
    fn turn_scan_acquires_a_free_session_and_can_release_it() {
        let root = tempfile::tempdir().unwrap();
        let scanner = owner(root.path(), SessionHold::Turn);
        let mut permit = scanner.try_for_scan("session-b").unwrap().unwrap();
        assert!(scanner.holds("session-b"));
        permit.release_if_idle();
        drop(permit);
        assert!(!scanner.holds("session-b"));
    }

    #[test]
    fn attached_claim_refuses_to_drop_another_held_session() {
        let root = tempfile::tempdir().unwrap();
        let owner = owner(root.path(), SessionHold::Attached);
        owner.claim_open("session-a", false).unwrap();
        let error = owner.claim_open("session-b", true).unwrap_err();
        assert!(error.is_code(ErrorCode::ActiveTurnRunning));
        assert!(owner.holds("session-a"));
        assert!(!owner.holds("session-b"));
    }
}
