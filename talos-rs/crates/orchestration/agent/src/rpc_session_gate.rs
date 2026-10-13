//! One Worker boundary shared by every Harness and transport. Only a small
//! causal marker is durable; histories, queues and business records stay above.
use agent_protocol::{
    AgentSessionContext, SessionExecutionState, SessionHistoryCursor, SessionStateVisibility,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, Mutex},
};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_types::AgentTurnRef;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Marker {
    binding: Binding,
    // Keep the private marker storage key stable; this is a naming change only.
    #[serde(rename = "head")]
    history_cursor: SessionHistoryCursor,
    coordinator_id: String,
    active: Option<AgentTurnRef>,
    visibility: SessionStateVisibility,
    cleanup_confirmed: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    agent_id: String,
    workspace_id: String,
    user_id: String,
}
impl From<&AgentSessionContext> for Binding {
    fn from(context: &AgentSessionContext) -> Self {
        Self {
            agent_id: context.agent_id.clone(),
            workspace_id: context.workspace.workspace_id.clone(),
            user_id: context.workspace.user_id.clone(),
        }
    }
}
#[derive(Debug, Default)]
struct State {
    markers: HashMap<String, Marker>,
    reserved: HashSet<String>,
    configuration_reserved: bool,
}
#[derive(Debug, Clone)]
pub(super) struct SessionGate {
    root: PathBuf,
    state: Arc<Mutex<State>>,
}
#[derive(Clone, Copy)]
pub(super) enum RunAdmission {
    NewTurn,
    Continuation,
}
#[derive(Clone, Copy)]
pub(super) enum HistoryBootstrap {
    Forbidden,
    TrustedLocal,
}
enum Maintenance {
    Idle(SessionHistoryCursor),
    Reflection,
}
#[derive(Debug)]
pub(super) struct Reservation {
    gate: SessionGate,
    session_id: Option<String>,
    consumed: bool,
    previous_marker: Option<Marker>,
}
impl Drop for Reservation {
    fn drop(&mut self) {
        let mut state = self.gate.state.lock().expect("Worker Session gate lock");
        let Some(session_id) = &self.session_id else {
            state.configuration_reserved = false;
            return;
        };
        state.reserved.remove(session_id);
        if !self.consumed {
            if let Some(marker) = self.previous_marker.take() {
                match self.gate.save(session_id, &marker) {
                    Ok(()) => {
                        state.markers.insert(session_id.clone(), marker);
                    }
                    Err(error) => talos_logging::report_error(&error),
                }
            } else {
                let removed = talos_fs::remove_file(&self.gate.path(session_id)).and_then(|()| {
                    std::fs::File::open(&self.gate.root)
                        .and_then(|directory| directory.sync_all())
                        .map_err(io_error)
                });
                match removed {
                    Ok(()) => {
                        state.markers.remove(session_id);
                    }
                    Err(error) => talos_logging::report_error(&error),
                }
            }
        }
    }
}
impl Reservation {
    pub(super) fn consuming(&mut self, identity: &AgentTurnRef) -> TalosResult<()> {
        self.gate.consuming(identity)?;
        self.consumed = true;
        Ok(())
    }
}
impl SessionGate {
    /// Configuration changes exclude every unfinished Session, including waiting
    /// continuations and recovery markers loaded after a Worker restart.
    pub(super) fn reserve_configuration(&self) -> TalosResult<Reservation> {
        let mut state = self.state.lock().expect("Worker Session gate lock");
        if state.configuration_reserved
            || !state.reserved.is_empty()
            || state
                .markers
                .values()
                .any(|marker| marker.active.is_some() || !marker.cleanup_confirmed)
        {
            return Err(busy());
        }
        match std::fs::read_dir(&self.root) {
            Ok(entries) => {
                for entry in entries {
                    let path = entry.map_err(io_error)?.path();
                    if path.extension().is_none_or(|extension| extension != "json") {
                        continue;
                    }
                    let marker: Marker = serde_json::from_slice(
                        &std::fs::read(path).map_err(io_error)?,
                    )
                    .map_err(|error| {
                        TalosError::internal_with_source(
                            "Worker execution marker is invalid; recovery is required.",
                            error,
                        )
                    })?;
                    if marker.active.is_some() || !marker.cleanup_confirmed {
                        return Err(busy());
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(io_error(error)),
        }
        state.configuration_reserved = true;
        Ok(Reservation {
            gate: self.clone(),
            session_id: None,
            consumed: true,
            previous_marker: None,
        })
    }

    pub(super) fn authorize_active(
        &self,
        agent_id: &str,
        identity: &AgentTurnRef,
        peer: &str,
    ) -> TalosResult<bool> {
        let mut state = self.state.lock().expect("Worker Session gate lock");
        let marker = self
            .load(&mut state, &identity.session_id)?
            .ok_or_else(|| {
                TalosError::new(
                    ErrorCode::RunCleanupUnconfirmed,
                    "Agent Turn is not known to this node instance",
                )
            })?;
        if marker.binding.agent_id != agent_id
            || marker.active.as_ref() != Some(identity)
            || marker.coordinator_id != peer
        {
            return Err(conflict());
        }
        Ok(marker.cleanup_confirmed && !state.reserved.contains(&identity.session_id))
    }

    pub(super) fn new(root: PathBuf) -> Self {
        Self {
            root,
            state: Arc::new(Mutex::new(State::default())),
        }
    }
    fn load<'a>(&self, state: &'a mut State, session_id: &str) -> TalosResult<Option<&'a Marker>> {
        if session_id.is_empty() {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Session identity is invalid",
            ));
        }
        if !state.markers.contains_key(session_id) {
            match std::fs::read(self.path(session_id)) {
                Ok(bytes) => {
                    let marker: Marker = serde_json::from_slice(&bytes).map_err(|error| {
                        TalosError::internal_with_source(
                            "Worker execution marker is invalid; recovery is required.",
                            error,
                        )
                    })?;
                    state.markers.insert(session_id.into(), marker);
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(io_error(error)),
            }
        }
        Ok(state.markers.get(session_id))
    }
    fn path(&self, id: &str) -> PathBuf {
        self.root
            .join(format!("{:x}.json", Sha256::digest(id.as_bytes())))
    }
    fn save(&self, id: &str, marker: &Marker) -> TalosResult<()> {
        talos_fs::write_json_pretty_with(
            &self.path(id),
            marker,
            talos_fs::WriteOptions {
                permissions: Some(talos_fs::FilePermissions::unix(0o600)),
                ..Default::default()
            },
        )
    }

    pub(super) fn read(&self, context: &AgentSessionContext) -> TalosResult<SessionExecutionState> {
        let mut state = self.state.lock().expect("Worker Session gate lock");
        match self.load(&mut state, &context.session_id)? {
            Some(marker) => {
                binding(marker, context)?;
                Ok(SessionExecutionState {
                    history_cursor: marker.history_cursor.clone(),
                    active_turn: marker.active.clone(),
                    coordinator_id: Some(marker.coordinator_id.clone()),
                    handoff_ready: marker.active.is_none()
                        && marker.cleanup_confirmed
                        && marker.visibility == SessionStateVisibility::Shared
                        && !state.reserved.contains(&context.session_id),
                })
            }
            None => Ok(SessionExecutionState {
                history_cursor: SessionHistoryCursor::default(),
                active_turn: None,
                coordinator_id: None,
                handoff_ready: true,
            }),
        }
    }
    pub(super) fn reserve(
        &self,
        context: &AgentSessionContext,
        identity: &AgentTurnRef,
        peer: &str,
        expected: &SessionHistoryCursor,
        admission: RunAdmission,
    ) -> TalosResult<Reservation> {
        if uuid::Uuid::parse_str(peer).is_err() {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Coordinator identity is invalid",
            ));
        }
        let mut state = self.state.lock().expect("Worker Session gate lock");
        if state.configuration_reserved || state.reserved.contains(&context.session_id) {
            return Err(busy());
        }
        let previous_marker = self.load(&mut state, &context.session_id)?.cloned();
        match &previous_marker {
            Some(marker) => {
                binding(marker, context)?;
                if &marker.history_cursor != expected {
                    return Err(conflict());
                }
                if !marker.cleanup_confirmed {
                    return Err(TalosError::new(
                        ErrorCode::RunCleanupUnconfirmed,
                        "Worker Session cleanup requires recovery by its last coordinator",
                    ));
                }
                if marker.active.is_some()
                    && !(matches!(admission, RunAdmission::Continuation)
                        && marker.active.as_ref() == Some(identity)
                        && marker.coordinator_id == peer)
                {
                    return Err(busy());
                }
                if marker.coordinator_id != peer
                    && marker.visibility != SessionStateVisibility::Shared
                {
                    return Err(conflict());
                }
            }
            None if expected != &SessionHistoryCursor::default() => return Err(conflict()),
            None => {}
        }
        let mut marker = previous_marker.clone().unwrap_or_else(|| Marker {
            binding: Binding::from(context),
            history_cursor: expected.clone(),
            coordinator_id: peer.into(),
            active: None,
            visibility: SessionStateVisibility::Local,
            cleanup_confirmed: true,
        });
        marker.coordinator_id = peer.into();
        marker.visibility = SessionStateVisibility::Local;
        marker.active = Some(identity.clone());
        self.save(&context.session_id, &marker)?;
        state.markers.insert(context.session_id.clone(), marker);
        state.reserved.insert(context.session_id.clone());
        Ok(Reservation {
            gate: self.clone(),
            session_id: Some(context.session_id.clone()),
            consumed: false,
            previous_marker,
        })
    }
    pub(super) fn consuming(&self, identity: &AgentTurnRef) -> TalosResult<()> {
        let mut state = self.state.lock().expect("Worker Session gate lock");
        let mut marker = self
            .load(&mut state, &identity.session_id)?
            .cloned()
            .ok_or_else(conflict)?;
        if !state.reserved.contains(&identity.session_id) {
            return Err(conflict());
        }
        marker.active = Some(identity.clone());
        marker.cleanup_confirmed = false;
        self.save(&identity.session_id, &marker)?;
        state.markers.insert(identity.session_id.clone(), marker);
        Ok(())
    }
    pub(super) fn cleaned(&self, identity: &AgentTurnRef) -> TalosResult<()> {
        let mut state = self.state.lock().expect("Worker Session gate lock");
        let mut marker = self
            .load(&mut state, &identity.session_id)?
            .cloned()
            .ok_or_else(conflict)?;
        if marker.active.as_ref() != Some(identity) {
            return Err(conflict());
        }
        marker.cleanup_confirmed = true;
        self.save(&identity.session_id, &marker)?;
        state.markers.insert(identity.session_id.clone(), marker);
        Ok(())
    }
    pub(super) fn commit(
        &self,
        context: &AgentSessionContext,
        peer: &str,
        identity: Option<&AgentTurnRef>,
        history_cursor: SessionHistoryCursor,
        visibility: SessionStateVisibility,
        bootstrap: HistoryBootstrap,
    ) -> TalosResult<()> {
        let mut state = self.state.lock().expect("Worker Session gate lock");
        let existing = self.load(&mut state, &context.session_id)?.cloned();
        let mut marker = match existing {
            Some(marker) => marker,
            None if matches!(bootstrap, HistoryBootstrap::TrustedLocal)
                && visibility == SessionStateVisibility::Local =>
            {
                if history_cursor.completed_turn_id.as_deref()
                    != identity.map(|identity| identity.turn_id.as_str())
                {
                    return Err(conflict());
                }
                let marker = Marker {
                    binding: Binding::from(context),
                    history_cursor,
                    coordinator_id: peer.into(),
                    active: None,
                    visibility,
                    cleanup_confirmed: true,
                };
                self.save(&context.session_id, &marker)?;
                state.markers.insert(context.session_id.clone(), marker);
                return Ok(());
            }
            None => return Err(conflict()),
        };
        binding(&marker, context)?;
        if marker.coordinator_id != peer
            || !marker.cleanup_confirmed
            || identity.is_some_and(|identity| identity.session_id != context.session_id)
        {
            return Err(conflict());
        }
        if visibility == SessionStateVisibility::Shared {
            if marker.active.is_some() || marker.history_cursor != history_cursor {
                return Err(conflict());
            }
        } else if let Some(active) = &marker.active {
            if Some(active) != identity
                || history_cursor.completed_turn_id.as_deref() != Some(active.turn_id.as_str())
                || history_cursor.history_generation != marker.history_cursor.history_generation
                || history_cursor.completed_event_sequence == 0
            {
                return Err(conflict());
            }
            marker.active = None;
        } else if marker.history_cursor != history_cursor
            && !(identity.is_none()
                && history_cursor.history_generation
                    == marker.history_cursor.history_generation + 1
                && history_cursor.completed_turn_id.is_none()
                && history_cursor.completed_event_sequence == 0)
        {
            return Err(conflict());
        }
        marker.history_cursor = history_cursor;
        marker.visibility = visibility;
        self.save(&context.session_id, &marker)?;
        state.markers.insert(context.session_id.clone(), marker);
        Ok(())
    }
    pub(super) fn maintenance(
        &self,
        context: &AgentSessionContext,
        peer: &str,
        expected_history_cursor: &SessionHistoryCursor,
    ) -> TalosResult<Reservation> {
        self.reserve_maintenance(
            context,
            peer,
            Maintenance::Idle(expected_history_cursor.clone()),
        )
    }
    pub(super) fn reflection(
        &self,
        context: &AgentSessionContext,
        peer: &str,
    ) -> TalosResult<Reservation> {
        self.reserve_maintenance(context, peer, Maintenance::Reflection)
    }
    fn reserve_maintenance(
        &self,
        context: &AgentSessionContext,
        peer: &str,
        operation: Maintenance,
    ) -> TalosResult<Reservation> {
        if uuid::Uuid::parse_str(peer).is_err() {
            return Err(conflict());
        }
        let mut state = self.state.lock().expect("Worker Session gate lock");
        if state.configuration_reserved || state.reserved.contains(&context.session_id) {
            return Err(busy());
        }
        let mut marker = self
            .load(&mut state, &context.session_id)?
            .cloned()
            .unwrap_or_else(|| Marker {
                binding: Binding::from(context),
                history_cursor: Default::default(),
                coordinator_id: peer.into(),
                active: None,
                visibility: SessionStateVisibility::Local,
                cleanup_confirmed: true,
            });
        binding(&marker, context)?;
        if let Maintenance::Idle(expected) = &operation {
            if marker.history_cursor != *expected {
                return Err(conflict());
            }
        }
        if !marker.cleanup_confirmed
            || marker.active.is_some()
                && !(matches!(operation, Maintenance::Reflection) && marker.coordinator_id == peer)
        {
            return Err(busy());
        }
        if marker.coordinator_id != peer && marker.visibility != SessionStateVisibility::Shared {
            return Err(conflict());
        }
        marker.coordinator_id = peer.into();
        marker.visibility = SessionStateVisibility::Local;
        self.save(&context.session_id, &marker)?;
        state.markers.insert(context.session_id.clone(), marker);
        state.reserved.insert(context.session_id.clone());
        Ok(Reservation {
            gate: self.clone(),
            session_id: Some(context.session_id.clone()),
            consumed: true,
            previous_marker: None,
        })
    }
}
fn binding(marker: &Marker, context: &AgentSessionContext) -> TalosResult<()> {
    if marker.binding != Binding::from(context) {
        Err(conflict())
    } else {
        Ok(())
    }
}
fn conflict() -> TalosError {
    TalosError::new(
        ErrorCode::Conflict,
        "Session execution boundary does not match the bound Worker",
    )
}
fn busy() -> TalosError {
    TalosError::new(
        ErrorCode::ActiveTurnRunning,
        "Session is reserved or active on this Worker",
    )
}
fn io_error(error: std::io::Error) -> TalosError {
    TalosError::from_source(
        ErrorCode::ServiceUnavailable,
        "Worker execution marker could not be persisted",
        error,
    )
}

#[cfg(test)]
#[path = "rpc_session_gate_tests.rs"]
mod tests;
