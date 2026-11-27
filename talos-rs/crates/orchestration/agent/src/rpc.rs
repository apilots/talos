//! Network projection of the existing in-process Harness contract.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use agent_pb as pb;
use agent_protocol::{error_pb, harness_pb, node_pb, NodeAuth, Registration};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use talos_config::Config;
use talos_error::{ErrorCode, TalosError, TalosResult};
use tokio::sync::{mpsc, watch};
use tokio::time::Instant;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;
use tokio_util::sync::CancellationToken;
use tonic::{Request, Response, Status, Streaming};

#[path = "rpc_connect.rs"]
mod connect;
#[path = "rpc_environment.rs"]
mod environment;
#[path = "rpc_execution_state.rs"]
mod execution_state;
#[path = "rpc_registration.rs"]
mod registration;
#[path = "rpc_service.rs"]
mod service;
#[path = "rpc_session_gate.rs"]
mod session_gate;
#[path = "rpc_streams.rs"]
mod streams;

use crate::{AgentHarness, AgentInterruptParams, AgentRunExecution};

#[path = "rpc_dream.rs"]
mod dream;

type RpcStream<T> = Pin<Box<dyn tokio_stream::Stream<Item = Result<T, Status>> + Send>>;
type CommitFuture = Pin<Box<dyn Future<Output = TalosResult<()>> + Send>>;
type TurnKey = (String, String, String);
/// Transport adapters bind immutable existing resource ports after reservation
/// and retain the returned guard until execution cleanup. The Harness SPI and
/// business Params do not carry network objects or coordinator policy.
pub type AgentInvocationResourceBinder = Arc<
    dyn Fn(
            &NodeAuth,
            &agent_protocol::AgentSessionContext,
            Option<&pb::CoordinatorCallContext>,
        ) -> crate::AgentFuture<'static, TalosResult<Box<dyn Send>>>
        + Send
        + Sync,
>;

pub type ManagedDesktopAdmission = Arc<
    dyn Fn(
            agent_protocol::RegistrationAdmission,
            CancellationToken,
        ) -> agent_protocol::AgentFuture<'static, TalosResult<()>>
        + Send
        + Sync,
>;

struct ActiveInterrupt {
    harness: Arc<dyn AgentHarness>,
    cleanup_confirmed: bool,
}

#[derive(Clone, PartialEq, Eq)]
enum AuthorizationOwner {
    Host,
    User(String),
}

#[derive(Clone)]
struct Lease {
    node_id: String,
    token_hash: [u8; 32],
    owner: AuthorizationOwner,
    deadline: watch::Sender<Instant>,
    revoked: CancellationToken,
}

impl Lease {
    fn expired(&self) -> bool {
        self.owner != AuthorizationOwner::Host && *self.deadline.borrow() <= Instant::now()
    }

    async fn ended(&self) {
        if self.owner == AuthorizationOwner::Host {
            self.revoked.cancelled().await;
            return;
        }
        let mut deadline = self.deadline.subscribe();
        loop {
            let expires_at = *deadline.borrow_and_update();
            tokio::select! {
                biased;
                _ = self.revoked.cancelled() => return,
                changed = deadline.changed() => { if changed.is_err() { return; } },
                _ = tokio::time::sleep_until(expires_at) => return,
            }
        }
    }

    // Backpressure must never prevent the lease from stopping execution.
    async fn send<T>(&self, sender: &mpsc::Sender<T>, frame: T) -> Result<(), ()> {
        tokio::select! {
            biased;
            _ = self.ended() => Err(()),
            result = sender.send(frame) => result.map_err(|_| ()),
        }
    }
}

/// Serves a node's existing Harness instances without owning product state.
#[derive(Clone)]
pub struct AgentHarnessRpcAdapter {
    node_id: String,
    session_gate: session_gate::SessionGate,
    resource_binder: Option<AgentInvocationResourceBinder>,
    environment: Option<Arc<dyn agent_protocol::WorkerEnvironment>>,
    registration_revoked: Option<Arc<dyn Fn() + Send + Sync>>,
    managed_desktop_admission: Option<ManagedDesktopAdmission>,
    config: Arc<std::sync::RwLock<Config>>,
    harnesses: Arc<Mutex<HashMap<String, Arc<dyn AgentHarness>>>>,
    lease: Arc<Mutex<Option<Lease>>>,
    blocked_registration: Arc<Mutex<Option<[u8; 32]>>>,
    local_auth: Arc<Mutex<Option<Lease>>>,
    active: Arc<Mutex<HashMap<TurnKey, AuthorizationOwner>>>,
    cleanup_failure: Arc<Mutex<Option<TalosError>>>,
    session_owner: crate::SessionOwner,
}

impl AgentHarnessRpcAdapter {
    /// Bind the existing typed Harness API to this host's authorization and
    /// causal gate. Shares state with remote ingress; never opens a listener.
    pub fn local_harness(
        &self,
        agent_id: String,
        auth: NodeAuth,
        coordinator_id: String,
    ) -> TalosResult<Arc<dyn AgentHarness>> {
        if self.authorize(&auth)?.owner != AuthorizationOwner::Host
            || uuid::Uuid::parse_str(&coordinator_id).is_err()
        {
            return Err(unauthenticated());
        }
        self.harness(&agent_id)?;
        Ok(Arc::new(execution_state::LocalAgentHarness {
            adapter: self.clone(),
            agent_id,
            auth,
            coordinator_id,
        }))
    }

    pub fn new(
        node_id: String,
        config: Arc<std::sync::RwLock<Config>>,
        harnesses: Vec<(String, Arc<dyn AgentHarness>)>,
    ) -> Self {
        let data_dir = config
            .read()
            .expect("Worker config lock")
            .talos_paths()
            .data_dir
            .clone();
        Self {
            session_gate: session_gate::SessionGate::new(
                data_dir.join("worker/execution-admission"),
            ),
            session_owner: crate::SessionOwner::new(
                data_dir.join("worker/session-locks"),
                talos_error::SessionOwnerKind::Server,
                crate::SessionHold::Turn,
            ),
            node_id,
            environment: None,
            resource_binder: None,
            registration_revoked: None,
            managed_desktop_admission: None,
            config,
            harnesses: Arc::new(Mutex::new(harnesses.into_iter().collect())),
            lease: Arc::new(Mutex::new(None)),
            blocked_registration: Default::default(),
            local_auth: Arc::new(Mutex::new(None)),
            active: Arc::new(Mutex::new(HashMap::new())),
            cleanup_failure: Arc::new(Mutex::new(None)),
        }
    }

    /// Selects who is named in a refusal and how long a successful claim is held.
    /// Call this before the adapter admits any session.
    #[must_use]
    pub fn with_session_policy(
        mut self,
        kind: talos_error::SessionOwnerKind,
        hold: crate::SessionHold,
    ) -> Self {
        let root = self.session_owner.root().to_path_buf();
        self.session_owner = crate::SessionOwner::new(root, kind, hold);
        self
    }

    pub fn session_owner(&self) -> crate::SessionOwner {
        self.session_owner.clone()
    }

    /// Bind the Worker-owned ports after Harness reservation and before permit
    /// consumption. Errors reject the invocation; the guard survives cleanup.
    #[must_use]
    pub fn with_resource_binder(mut self, binder: AgentInvocationResourceBinder) -> Self {
        self.resource_binder = Some(binder);
        self
    }
    async fn bind_resources(
        &self,
        auth: &NodeAuth,
        context: &agent_protocol::AgentSessionContext,
        caller: Option<&pb::CoordinatorCallContext>,
    ) -> TalosResult<Box<dyn Send>> {
        if let Some(binder) = &self.resource_binder {
            return binder(auth, context, caller).await;
        }
        Ok(Box::new(()))
    }

    fn authorize(&self, auth: &NodeAuth) -> TalosResult<Lease> {
        let lease = self.authorize_transport(auth)?;
        self.authorize_admission(&lease)?;
        Ok(lease)
    }

    fn authorize_transport(&self, auth: &NodeAuth) -> TalosResult<Lease> {
        if auth.token.is_empty() {
            return Err(unauthenticated());
        }
        let actual: [u8; 32] = Sha256::digest(auth.token.as_bytes()).into();
        if let Some(local) = self
            .local_auth
            .lock()
            .expect("Local authorization lock poisoned")
            .as_ref()
        {
            if auth.node_id == local.node_id
                && bool::from(actual.ct_eq(&local.token_hash))
                && !local.revoked.is_cancelled()
            {
                return Ok(local.clone());
            }
        }
        let lease = self
            .lease
            .lock()
            .expect("Agent lease lock poisoned")
            .clone()
            .ok_or_else(unauthenticated)?;
        if auth.node_id != lease.node_id
            || !bool::from(actual.ct_eq(&lease.token_hash))
            || lease.revoked.is_cancelled()
            || lease.expired()
        {
            return Err(unauthenticated());
        }
        Ok(lease)
    }

    fn harness(&self, agent_id: &str) -> TalosResult<Arc<dyn AgentHarness>> {
        self.harnesses
            .lock()
            .expect("Agent harness lock poisoned")
            .get(agent_id)
            .cloned()
            .ok_or_else(|| {
                TalosError::new(
                    ErrorCode::ResourceNotFound,
                    "Agent Harness is not registered",
                )
            })
    }

    async fn prepare_maintenance(
        &self,
        session: &agent_protocol::AgentSessionContext,
        peer: &str,
        expected: &talos_types::SessionHistoryCursor,
    ) -> TalosResult<(
        crate::SessionLease,
        session_gate::Reservation,
        Arc<dyn AgentHarness>,
    )> {
        let ownership = self.session_owner.pin(&session.session_id)?;
        let state = self.durable_state(session).await?;
        if state.active_turn.is_some() {
            return Err(TalosError::new(
                ErrorCode::ActiveTurnRunning,
                "Worker Session must be idle for maintenance.",
            ));
        }
        if state.history_cursor != *expected {
            return Err(TalosError::new(
                ErrorCode::EventCursorExpired,
                "Worker maintenance targets another causal history cursor.",
            ));
        }
        let reservation = self.session_gate.maintenance(session, peer, expected)?;
        let harness = self.harness(&session.agent_id)?;
        Ok((ownership, reservation, harness))
    }

    fn prepare_interrupt(
        &self,
        auth: &NodeAuth,
        params: &AgentInterruptParams,
        peer: &str,
    ) -> TalosResult<Option<ActiveInterrupt>> {
        self.authorize(auth)?;
        let local = self
            .active
            .lock()
            .expect("Agent active lock poisoned")
            .contains_key(&(
                params.agent_id.clone(),
                params.identity.session_id.clone(),
                params.identity.turn_id.clone(),
            ));
        if local {
            let cleanup_confirmed =
                self.session_gate
                    .authorize_active(&params.agent_id, &params.identity, peer)?;
            Ok(Some(ActiveInterrupt {
                harness: self.harness(&params.agent_id)?,
                cleanup_confirmed,
            }))
        } else {
            Ok(None)
        }
    }

    async fn interrupt_typed(
        &self,
        auth: &NodeAuth,
        params: AgentInterruptParams,
        peer: &str,
    ) -> TalosResult<talos_agent_api::AgentInterruptOutcome> {
        let prepared = self.prepare_interrupt(auth, &params, peer)?;
        self.finish_interrupt(params, peer, prepared).await
    }

    async fn finish_interrupt(
        &self,
        params: AgentInterruptParams,
        peer: &str,
        prepared: Option<ActiveInterrupt>,
    ) -> TalosResult<talos_agent_api::AgentInterruptOutcome> {
        if let Some(prepared) = prepared {
            prepared.harness.interrupt(params).await.map_err(|error| {
                if !prepared.cleanup_confirmed && error.is_code(ErrorCode::ResourceNotFound) {
                    TalosError::from_source(
                        ErrorCode::RunCleanupUnconfirmed,
                        "Agent Turn cleanup is not confirmed by this node instance",
                        error,
                    )
                } else {
                    error
                }
            })
        } else {
            let _ownership = self.session_owner.pin(&params.identity.session_id)?;
            self.session_gate.confirm_orphan_cleanup(
                &params.agent_id,
                &params.identity,
                peer,
                || !self.session_owner.holds_turn(&params.identity),
            )?;
            // The local journal owns waiting/restarted execution settlement.
            self.harness(&params.agent_id)?.interrupt(params).await
        }
    }

    async fn shutdown_harness(
        &self,
        agent_id: &str,
        harness: Arc<dyn AgentHarness>,
        timeout: Duration,
    ) -> TalosResult<()> {
        let result = harness.shutdown(timeout).await;
        if result.is_ok() {
            let mut harnesses = self.harnesses.lock().expect("Agent harness lock poisoned");
            if harnesses
                .get(agent_id)
                .is_some_and(|current| Arc::ptr_eq(current, &harness))
            {
                harnesses.remove(agent_id);
            }
        }
        result
    }

    fn memory_context(
        &self,
        auth: Option<pb::NodeAuth>,
        context: Option<pb::SessionContext>,
    ) -> Result<crate::AgentSessionContext, Status> {
        let auth = node_pb::decode_auth(auth).map_err(status_from_error)?;
        self.authorize(&auth).map_err(status_from_error)?;
        let context = harness_pb::decode_session_context(
            context,
            &self.config.read().expect("Worker config lock poisoned"),
        )
        .map_err(status_from_error)?;
        Ok(context)
    }
}

fn input_failure(error: TalosError) -> pb::InputResponse {
    pb::InputResponse {
        result: Some(pb::input_response::Result::Failure(failure(&error))),
    }
}

fn empty_response(result: TalosResult<()>) -> pb::EmptyResponse {
    pb::EmptyResponse {
        result: Some(match result {
            Ok(()) => pb::empty_response::Result::Completed(pb::Empty {}),
            Err(error) => pb::empty_response::Result::Failure(failure(&error)),
        }),
    }
}

async fn read_commit<T>(
    mut inbound: RpcStream<T>,
    extract: fn(T) -> TalosResult<NodeAuth>,
    adapter: AgentHarnessRpcAdapter,
    original: Lease,
) -> TalosResult<()>
where
    T: prost::Message + Default + Send + 'static,
{
    tokio::select! {
        biased;
        _ = original.ended() => Err(unauthenticated()),
        result = async {
    let frame = tokio::time::timeout(Duration::from_secs(30), inbound.next())
        .await
        .map_err(|_| TalosError::new(ErrorCode::Timeout, "Agent admission commit timed out"))?
        .transpose().map_err(transport_error)?
        .ok_or_else(invalid_request)?;
    let auth = extract(frame)?;
    let current = adapter.authorize(&auth)?;
    if !bool::from(current.token_hash.ct_eq(&original.token_hash))
        || original.revoked.is_cancelled()
    {
        return Err(unauthenticated());
    }
    let extra = tokio::time::timeout(Duration::from_secs(30), inbound.next())
        .await
        .map_err(|_| {
            TalosError::new(
                ErrorCode::Timeout,
                "Agent admission commit stream did not close",
            )
        })?
        .transpose().map_err(transport_error)?;
    if extra.is_some() {
        return Err(invalid_request());
    }
    Ok(())
        } => result,
    }
}

fn start_commit(frame: pb::StartRequest) -> TalosResult<NodeAuth> {
    let auth = node_pb::decode_auth(frame.auth)?;
    if !matches!(
        frame.params.and_then(|params| params.body),
        Some(pb::start_params::Body::Commit(_))
    ) {
        return Err(invalid_request());
    }
    Ok(auth)
}

fn compact_commit(frame: pb::CompactSessionRequest) -> TalosResult<NodeAuth> {
    let auth = node_pb::decode_auth(frame.auth)?;
    if !matches!(
        frame.params.and_then(|params| params.body),
        Some(pb::compact_session_params::Body::Commit(_))
    ) {
        return Err(invalid_request());
    }
    Ok(auth)
}

fn input_commit(frame: pb::SubmitInputRequest) -> TalosResult<NodeAuth> {
    let auth = node_pb::decode_auth(frame.auth)?;
    if !matches!(
        frame.params.and_then(|params| params.body),
        Some(pb::submit_input_params::Body::Commit(_))
    ) {
        return Err(invalid_request());
    }
    Ok(auth)
}

fn transport_error(error: Status) -> TalosError {
    TalosError::from_source(
        ErrorCode::ServiceUnavailable,
        "Agent RPC stream did not complete",
        error,
    )
}

fn run_failure(error: TalosError) -> pb::RunResponse {
    pb::RunResponse {
        result: Some(pb::run_response::Result::Failure(Box::new(failure(&error)))),
    }
}

fn invalid_request() -> TalosError {
    TalosError::new(ErrorCode::InvalidRequest, "Agent RPC request is invalid")
}
fn unauthenticated() -> TalosError {
    TalosError::new(ErrorCode::Unauthenticated, "Agent node lease is invalid")
}
fn failure(error: &TalosError) -> pb::Failure {
    talos_logging::report_error(error);
    error_pb::encode_failure(error)
}

fn status_from_error(error: TalosError) -> Status {
    talos_logging::report_error(&error);
    error_pb::encode_status(&error)
}

async fn interrupt_disconnected(
    harness: &Arc<dyn AgentHarness>,
    agent_id: &str,
    identity: &talos_agent_api::AgentTurnRef,
) -> TalosResult<()> {
    match harness
        .interrupt(AgentInterruptParams {
            agent_id: agent_id.to_owned(),
            identity: identity.clone(),
        })
        .await
    {
        Err(error) => Err(error),
        Ok(talos_agent_api::AgentInterruptOutcome::CleanupUnconfirmed { .. }) => {
            Err(TalosError::new(
                ErrorCode::RunCleanupUnconfirmed,
                "Disconnected Agent Turn cleanup could not be confirmed",
            ))
        }
        Ok(_) => Ok(()),
    }
}

#[cfg(test)]
#[path = "rpc_backpressure_tests.rs"]
mod backpressure_tests;
