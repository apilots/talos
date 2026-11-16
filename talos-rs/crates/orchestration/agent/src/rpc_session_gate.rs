//! One Worker boundary shared by every Harness and transport. Only a small
//! causal marker is durable; histories, queues and business records stay above.
use agent_protocol::{
    AgentSessionContext, SessionExecutionState, SessionHistoryCursor, SessionStateVisibility,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
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
    reservations: HashMap<String, Marker>,
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
        state.reservations.remove(session_id);
        if !self.consumed {
            let restored = if let Some(marker) = self.previous_marker.take() {
                self.gate.save(session_id, &marker)
            } else {
                talos_fs::remove_file(&self.gate.path(session_id)).and_then(|()| {
                    std::fs::File::open(&self.gate.root)
                        .and_then(|directory| directory.sync_all())
                        .map_err(io_error)
                })
            };
            if let Err(error) = restored {
                talos_logging::report_error(&error);
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
        if state.configuration_reserved || !state.reservations.is_empty() {
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
        let state = self.state.lock().expect("Worker Session gate lock");
        let marker = self.load(&state, &identity.session_id)?.ok_or_else(|| {
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
        Ok(marker.cleanup_confirmed && !state.reservations.contains_key(&identity.session_id))
    }

    /// The caller holds the Worker session lease. Reconcile only the marker
    /// authorized for this coordinator and exact Turn.
    pub(super) fn confirm_orphan_cleanup(
        &self,
        agent_id: &str,
        identity: &AgentTurnRef,
        peer: &str,
        can_confirm: impl FnOnce() -> bool,
    ) -> TalosResult<()> {
        let state = self.state.lock().expect("Worker Session gate lock");
        if state.reservations.contains_key(&identity.session_id) {
            return Err(cleanup_unconfirmed());
        }
        let mut marker = self
            .load(&state, &identity.session_id)?
            .ok_or_else(cleanup_unconfirmed)?;
        if marker.binding.agent_id != agent_id
            || marker.active.as_ref() != Some(identity)
            || marker.coordinator_id != peer
        {
            return Err(conflict());
        }
        if !marker.cleanup_confirmed {
            if !can_confirm() {
                return Err(cleanup_unconfirmed());
            }
            marker.cleanup_confirmed = true;
            self.save(&identity.session_id, &marker)?;
        }
        Ok(())
    }

    pub(super) fn new(root: PathBuf) -> Self {
        Self {
            root,
            state: Arc::new(Mutex::new(State::default())),
        }
    }
    fn load(&self, state: &State, session_id: &str) -> TalosResult<Option<Marker>> {
        if session_id.is_empty() {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Session identity is invalid",
            ));
        }
        if let Some(marker) = state.reservations.get(session_id) {
            return Ok(Some(marker.clone()));
        }
        // Without a local reservation, another owner may have advanced the marker.
        match std::fs::read(self.path(session_id)) {
            Ok(bytes) => serde_json::from_slice(&bytes).map(Some).map_err(|error| {
                TalosError::internal_with_source(
                    "Worker execution marker is invalid; recovery is required.",
                    error,
                )
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(io_error(error)),
        }
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
        let state = self.state.lock().expect("Worker Session gate lock");
        match self.load(&state, &context.session_id)? {
            Some(marker) => {
                binding(&marker, context)?;
                let handoff_ready = marker.active.is_none()
                    && marker.cleanup_confirmed
                    && marker.visibility == SessionStateVisibility::Shared
                    && !state.reservations.contains_key(&context.session_id);
                Ok(SessionExecutionState {
                    history_cursor: marker.history_cursor,
                    active_turn: marker.active,
                    coordinator_id: Some(marker.coordinator_id),
                    handoff_ready,
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
        if state.configuration_reserved || state.reservations.contains_key(&context.session_id) {
            return Err(busy());
        }
        let previous_marker = self.load(&state, &context.session_id)?;
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
        state
            .reservations
            .insert(context.session_id.clone(), marker);
        Ok(Reservation {
            gate: self.clone(),
            session_id: Some(context.session_id.clone()),
            consumed: false,
            previous_marker,
        })
    }
    pub(super) fn consuming(&self, identity: &AgentTurnRef) -> TalosResult<()> {
        let mut state = self.state.lock().expect("Worker Session gate lock");
        let cached = state
            .reservations
            .get_mut(&identity.session_id)
            .ok_or_else(conflict)?;
        let mut marker = cached.clone();
        marker.active = Some(identity.clone());
        marker.cleanup_confirmed = false;
        self.save(&identity.session_id, &marker)?;
        *cached = marker;
        Ok(())
    }
    pub(super) fn cleaned(&self, identity: &AgentTurnRef) -> TalosResult<()> {
        let mut state = self.state.lock().expect("Worker Session gate lock");
        let mut marker = self
            .load(&state, &identity.session_id)?
            .ok_or_else(conflict)?;
        if marker.active.as_ref() != Some(identity) {
            return Err(conflict());
        }
        marker.cleanup_confirmed = true;
        self.save(&identity.session_id, &marker)?;
        if let Some(cached) = state.reservations.get_mut(&identity.session_id) {
            *cached = marker;
        }
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
        let existing = self.load(&state, &context.session_id)?;
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
        if let Some(cached) = state.reservations.get_mut(&context.session_id) {
            *cached = marker;
        }
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
        if state.configuration_reserved || state.reservations.contains_key(&context.session_id) {
            return Err(busy());
        }
        let mut marker = self
            .load(&state, &context.session_id)?
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
        state
            .reservations
            .insert(context.session_id.clone(), marker);
        Ok(Reservation {
            gate: self.clone(),
            session_id: Some(context.session_id.clone()),
            consumed: true,
            previous_marker: None,
        })
    }
}
fn cleanup_unconfirmed() -> TalosError {
    TalosError::new(
        ErrorCode::RunCleanupUnconfirmed,
        "Agent Turn cleanup is not confirmed by this node instance",
    )
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
