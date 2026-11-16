//! Per-session exclusive ownership for the worker that writes the session.
//!
//! The OS file lock is the source of truth. File contents exist only so a
//! refused caller can say who holds the session. The kernel releases the lock
//! when this process dies.

use std::collections::{HashMap, HashSet};
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
    _lock: FileLock,
    leases: usize,
    turns: HashSet<String>,
    process_hold: bool,
}

#[derive(Default)]
struct State {
    held: HashMap<String, HeldSession>,
    open_session: Option<String>,
}

impl State {
    fn release_idle(&mut self, session_id: &str) {
        if self.open_session.as_deref() != Some(session_id)
            && self
                .held
                .get(session_id)
                .is_some_and(|held| held.leases == 0 && held.turns.is_empty() && !held.process_hold)
        {
            self.held.remove(session_id);
        }
    }
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

/// Keeps a session locked for one operation, including admission and settlement.
/// Dropping a lease cannot release another operation's lease or turn.
pub struct SessionLease {
    owner: SessionOwner,
    session_id: String,
}

impl SessionOwner {
    pub fn new(root: impl Into<PathBuf>, kind: SessionOwnerKind, hold: SessionHold) -> Self {
        Self {
            inner: Arc::new(Inner {
                root: root.into(),
                kind,
                hold,
                state: Mutex::default(),
            }),
        }
    }

    pub fn hold(&self) -> SessionHold {
        self.inner.hold
    }

    pub fn root(&self) -> &Path {
        &self.inner.root
    }

    /// Retain an explicitly opened session according to the runtime policy.
    pub fn claim_open(&self, session_id: &str) -> TalosResult<()> {
        let mut state = self.inner.state.lock().expect("session owner lock");
        match self.inner.hold {
            SessionHold::Turn => {}
            SessionHold::Process => self.acquire(&mut state, session_id)?.process_hold = true,
            SessionHold::Attached => {
                if let Some(other) = state.open_session.as_deref() {
                    if other == session_id {
                        return Ok(());
                    }
                    let held = &state.held[other];
                    if held.leases > 0 || !held.turns.is_empty() {
                        return Err(TalosError::new(
                            ErrorCode::ActiveTurnRunning,
                            "Finish the active turn before switching sessions.",
                        ));
                    }
                }
                self.acquire(&mut state, session_id)?;
                if let Some(previous) = state.open_session.replace(session_id.to_owned()) {
                    state.release_idle(&previous);
                }
            }
        }
        Ok(())
    }

    pub fn pin(&self, session_id: &str) -> TalosResult<SessionLease> {
        let mut state = self.inner.state.lock().expect("session owner lock");
        self.acquire(&mut state, session_id)?.leases += 1;
        Ok(self.lease(session_id))
    }

    pub fn background_scan_enabled(&self) -> bool {
        self.scans_unowned()
            || matches!(
                (self.inner.kind, self.inner.hold),
                (SessionOwnerKind::Tui, SessionHold::Attached)
                    | (SessionOwnerKind::Cli, SessionHold::Process)
            )
    }

    fn scans_unowned(&self) -> bool {
        matches!(
            self.inner.kind,
            SessionOwnerKind::Server | SessionOwnerKind::Desktop
        )
    }

    /// The scanner can run only on an explicitly attached session for a foreground runtime.
    pub fn try_for_scan(&self, session_id: &str) -> TalosResult<Option<SessionLease>> {
        let mut state = self.inner.state.lock().expect("session owner lock");
        let foreground_owner = match (self.inner.kind, self.inner.hold) {
            (SessionOwnerKind::Tui, SessionHold::Attached) => {
                state.open_session.as_deref() == Some(session_id)
            }
            (SessionOwnerKind::Cli, SessionHold::Process) => state
                .held
                .get(session_id)
                .is_some_and(|held| held.process_hold),
            _ => false,
        };
        if !self.scans_unowned() && !foreground_owner {
            return Ok(None);
        }
        match self.acquire(&mut state, session_id) {
            Ok(held) => {
                held.leases += 1;
                Ok(Some(self.lease(session_id)))
            }
            Err(error) if error.is_code(ErrorCode::SessionHeld) => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub(crate) fn holds_turn(&self, identity: &talos_types::AgentTurnRef) -> bool {
        self.inner
            .state
            .lock()
            .expect("session owner lock")
            .held
            .get(&identity.session_id)
            .is_some_and(|held| held.turns.contains(&identity.turn_id))
    }

    pub(crate) fn release_turn(&self, identity: &talos_types::AgentTurnRef) {
        let mut state = self.inner.state.lock().expect("session owner lock");
        if let Some(held) = state.held.get_mut(&identity.session_id) {
            held.turns.remove(&identity.turn_id);
            state.release_idle(&identity.session_id);
        }
    }

    fn lease(&self, session_id: &str) -> SessionLease {
        SessionLease {
            owner: self.clone(),
            session_id: session_id.to_owned(),
        }
    }

    fn acquire<'a>(
        &self,
        state: &'a mut State,
        session_id: &str,
    ) -> TalosResult<&'a mut HeldSession> {
        if session_id.is_empty() {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Session identity is invalid",
            ));
        }
        use std::collections::hash_map::Entry;
        match state.held.entry(session_id.to_owned()) {
            Entry::Occupied(entry) => Ok(entry.into_mut()),
            Entry::Vacant(entry) => {
                let path = self
                    .inner
                    .root
                    .join(format!("{:x}.lock", Sha256::digest(session_id.as_bytes())));
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
                Ok(entry.insert(HeldSession {
                    _lock: lock,
                    leases: 0,
                    turns: HashSet::new(),
                    process_hold: false,
                }))
            }
        }
    }
}

impl SessionLease {
    /// An in-process operation may still be settling a committed terminal event.
    /// A scan can reconcile that work once it is the only operation holding a lease.
    pub fn can_reconcile(&self) -> bool {
        self.owner
            .inner
            .state
            .lock()
            .expect("session owner lock")
            .held[&self.session_id]
            .leases
            == 1
    }

    pub(crate) fn retain_turn(&self, turn_id: &str) {
        let mut state = self.owner.inner.state.lock().expect("session owner lock");
        state
            .held
            .get_mut(&self.session_id)
            .expect("held session lease")
            .turns
            .insert(turn_id.to_owned());
    }
}

impl Drop for SessionLease {
    fn drop(&mut self) {
        let mut state = self.owner.inner.state.lock().expect("session owner lock");
        let held = state
            .held
            .get_mut(&self.session_id)
            .expect("held session lease");
        held.leases -= 1;
        state.release_idle(&self.session_id);
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
#[path = "session_owner_tests.rs"]
mod tests;
