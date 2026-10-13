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
        ) -> TalosResult<Box<dyn Send>>
        + Send
        + Sync,
>;

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
    config: Arc<std::sync::RwLock<Config>>,
    harnesses: Arc<Mutex<HashMap<String, Arc<dyn AgentHarness>>>>,
    lease: Arc<Mutex<Option<Lease>>>,
    local_auth: Arc<Mutex<Option<Lease>>>,
    active: Arc<Mutex<HashMap<TurnKey, AuthorizationOwner>>>,
    cleanup_failure: Arc<Mutex<Option<TalosError>>>,
}

impl AgentHarnessRpcAdapter {
    pub fn new(
        node_id: String,
        config: Arc<std::sync::RwLock<Config>>,
        harnesses: Vec<(String, Arc<dyn AgentHarness>)>,
    ) -> Self {
        let root = config
            .read()
            .expect("Worker config lock")
            .talos_paths()
            .data_dir
            .join("worker/session-heads");
        Self {
            session_gate: session_gate::SessionGate::new(root),
            node_id,
            environment: None,
            resource_binder: None,
            registration_revoked: None,
            config,
            harnesses: Arc::new(Mutex::new(harnesses.into_iter().collect())),
            lease: Arc::new(Mutex::new(None)),
            local_auth: Arc::new(Mutex::new(None)),
            active: Arc::new(Mutex::new(HashMap::new())),
            cleanup_failure: Arc::new(Mutex::new(None)),
        }
    }

    /// Bind the Worker-owned ports after Harness reservation and before permit
    /// consumption. Errors reject the invocation; the guard survives cleanup.
    #[must_use]
    pub fn with_resource_binder(mut self, binder: AgentInvocationResourceBinder) -> Self {
        self.resource_binder = Some(binder);
        self
    }
    fn bind_resources(
        &self,
        auth: &NodeAuth,
        context: &agent_protocol::AgentSessionContext,
        caller: Option<&pb::CoordinatorCallContext>,
    ) -> TalosResult<Box<dyn Send>> {
        // Coordinator identity is independent of an invocation callback route.
        // Loopback keeps the original private resource client; G supplies a route.
        let route = caller.filter(|caller| !caller.connection_id.is_empty());
        if let Some(binder) = &self.resource_binder {
            return binder(auth, context, route);
        }
        if route.is_some() {
            return Err(TalosError::new(
                ErrorCode::ServiceUnavailable,
                "Worker invocation resources are unavailable",
            ));
        }
        Ok(Box::new(()))
    }

    /// Renew the same lease in place, or replace a registration and cancel its
    /// old streams. Renewal must not change the execution's lease identity.
    pub fn install_registration(
        &self,
        registration: Registration,
        deadline: Instant,
    ) -> TalosResult<()> {
        self.install_node_registration(&self.node_id, registration, deadline)
    }

    /// Attach the control plane's verified UUID to this same execution runtime.
    /// The host alias and its private credential retain their independent lifetime.
    pub fn install_node_registration(
        &self,
        node_id: &str,
        registration: Registration,
        deadline: Instant,
    ) -> TalosResult<()> {
        if deadline <= Instant::now()
            || node_id.trim().is_empty()
            || registration.lease_token.is_empty()
            || registration.owner_user_id.is_empty()
        {
            return Err(invalid_request());
        }
        let lease = Lease {
            node_id: node_id.into(),
            token_hash: Sha256::digest(registration.lease_token.as_bytes()).into(),
            owner: AuthorizationOwner::User(registration.owner_user_id),
            deadline: watch::channel(deadline).0,
            revoked: CancellationToken::new(),
        };
        let mut current = self.lease.lock().expect("Agent lease lock poisoned");
        if let Some(previous) = current.as_ref() {
            if previous.node_id == lease.node_id
                && previous.token_hash == lease.token_hash
                && previous.owner == lease.owner
            {
                previous.deadline.send_replace(deadline);
                return Ok(());
            }
        }
        if let Some(previous) = current.replace(lease) {
            previous.revoked.cancel();
            if let Some(revoked) = &self.registration_revoked {
                revoked();
            }
        }
        Ok(())
    }

    /// Hosts attach node-local authorization cancellation to the existing lease lifecycle.
    #[must_use]
    pub fn with_registration_revocation_hook(mut self, hook: Arc<dyn Fn() + Send + Sync>) -> Self {
        self.registration_revoked = Some(hook);
        self
    }

    pub fn clear_registration(&self) {
        if let Some(previous) = self.lease.lock().expect("Agent lease lock poisoned").take() {
            previous.revoked.cancel();
            if let Some(revoked) = &self.registration_revoked {
                revoked();
            }
        }
    }

    /// A protected host credential permits the host's already-authorized users.
    /// It is never advertised or sent through the cloud registration connection.
    pub fn install_local_auth(&self, auth: &NodeAuth) -> TalosResult<()> {
        if auth.node_id != self.node_id || auth.token.is_empty() {
            return Err(invalid_request());
        }
        let lease = Lease {
            node_id: auth.node_id.clone(),
            token_hash: Sha256::digest(auth.token.as_bytes()).into(),
            owner: AuthorizationOwner::Host,
            deadline: watch::channel(Instant::now()).0,
            revoked: CancellationToken::new(),
        };
        if let Some(previous) = self
            .local_auth
            .lock()
            .expect("Local authorization lock poisoned")
            .replace(lease)
        {
            previous.revoked.cancel();
        }
        Ok(())
    }

    pub fn clear_local_auth(&self) {
        if let Some(previous) = self
            .local_auth
            .lock()
            .expect("Local authorization lock poisoned")
            .take()
        {
            previous.revoked.cancel();
        }
    }

    fn authorize(&self, auth: &NodeAuth) -> TalosResult<Lease> {
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

#[tonic::async_trait]
impl pb::agent_harness_rpc_server::AgentHarnessRpc for AgentHarnessRpcAdapter {
    type StartStream = RpcStream<pb::RunResponse>;
    type SubmitInputStream = RpcStream<pb::InputResponse>;
    type CompactSessionStream = RpcStream<pb::RunResponse>;
    type FormDreamMemoryStream = RpcStream<pb::FormDreamMemoryResponse>;

    async fn read_session_execution_state(
        &self,
        request: Request<pb::ReadSessionExecutionStateRequest>,
    ) -> Result<Response<pb::SessionExecutionStateResponse>, Status> {
        Ok(Response::new(pb::SessionExecutionStateResponse {
            result: Some(match self.read_execution_state(request.into_inner()) {
                Ok(state) => pb::session_execution_state_response::Result::State(
                    agent_protocol::execution_state_pb::encode_state(state),
                ),
                Err(error) => {
                    pb::session_execution_state_response::Result::Failure(failure(&error))
                }
            }),
        }))
    }
    async fn commit_session_execution_state(
        &self,
        request: Request<pb::CommitSessionExecutionStateRequest>,
    ) -> Result<Response<pb::EmptyResponse>, Status> {
        Ok(Response::new(empty_response(
            self.commit_execution_state(request.into_inner()),
        )))
    }

    async fn start(
        &self,
        request: Request<Streaming<pb::StartRequest>>,
    ) -> Result<Response<Self::StartStream>, Status> {
        self.serve_start(Box::pin(request.into_inner())).await
    }

    async fn compact_session(
        &self,
        request: Request<Streaming<pb::CompactSessionRequest>>,
    ) -> Result<Response<Self::CompactSessionStream>, Status> {
        self.serve_compact_session(Box::pin(request.into_inner()))
            .await
    }

    async fn submit_input(
        &self,
        request: Request<Streaming<pb::SubmitInputRequest>>,
    ) -> Result<Response<Self::SubmitInputStream>, Status> {
        self.serve_submit_input(Box::pin(request.into_inner()))
            .await
    }

    async fn interrupt(
        &self,
        request: Request<pb::InterruptRequest>,
    ) -> Result<Response<pb::InterruptResponse>, Status> {
        let request = request.into_inner();
        let auth = node_pb::decode_auth(request.auth).map_err(status_from_error)?;
        self.authorize(&auth).map_err(status_from_error)?;
        let params = harness_pb::decode_interrupt_params(request.params.ok_or_else(|| {
            status_from_error(TalosError::new(
                ErrorCode::InvalidRequest,
                "missing interrupt params",
            ))
        })?)
        .map_err(status_from_error)?;
        let peer = request
            .caller
            .as_ref()
            .map(|caller| caller.coordinator_id.as_str())
            .ok_or_else(|| status_from_error(invalid_request()))?;
        let cleanup_confirmed = self
            .session_gate
            .authorize_active(&params.agent_id, &params.identity, peer)
            .map_err(status_from_error)?;
        let result = if self
            .active
            .lock()
            .expect("Agent active lock poisoned")
            .contains_key(&(
                params.agent_id.clone(),
                params.identity.session_id.clone(),
                params.identity.turn_id.clone(),
            )) {
            self.harness(&params.agent_id)
                .map_err(status_from_error)?
                .interrupt(params)
                .await
        } else if cleanup_confirmed {
            // Waiting releases the Harness live slot, while the original Gate
            // retains the exact coordinator/Turn and confirmed cleanup.
            Ok(talos_agent_api::AgentInterruptOutcome::Cancelled)
        } else {
            Err(TalosError::new(
                ErrorCode::RunCleanupUnconfirmed,
                "Agent Turn is not known to this node instance",
            ))
        };
        let result = result.map_err(|error| {
            if !cleanup_confirmed && error.is_code(ErrorCode::ResourceNotFound) {
                TalosError::from_source(
                    ErrorCode::RunCleanupUnconfirmed,
                    "Agent Turn cleanup is not confirmed by this node instance",
                    error,
                )
            } else {
                error
            }
        });
        Ok(Response::new(pb::InterruptResponse {
            result: Some(match result {
                Ok(value) => pb::interrupt_response::Result::Interrupted(
                    harness_pb::encode_interrupt_outcome(value),
                ),
                Err(error) => pb::interrupt_response::Result::Failure(failure(&error)),
            }),
        }))
    }

    async fn clear_session(
        &self,
        request: Request<pb::SessionRequest>,
    ) -> Result<Response<pb::EmptyResponse>, Status> {
        let request = request.into_inner();
        let auth = node_pb::decode_auth(request.auth).map_err(status_from_error)?;
        self.authorize(&auth).map_err(status_from_error)?;
        let params = harness_pb::decode_session_context(
            request.params,
            &self.config.read().expect("Worker config lock poisoned"),
        )
        .map_err(status_from_error)?;
        let peer = self
            .coordinator_id(request.caller.as_ref(), &params)
            .map_err(status_from_error)?;
        let expected_history_cursor = agent_protocol::execution_state_pb::decode_history_cursor(
            request.expected_history_cursor.ok_or_else(|| {
                status_from_error(TalosError::new(
                    ErrorCode::InvalidRequest,
                    "Session history cursor is required",
                ))
            })?,
        )
        .map_err(status_from_error)?;
        let _reservation = self
            .session_gate
            .maintenance(&params, peer, &expected_history_cursor)
            .map_err(status_from_error)?;
        let result = self
            .harness(&params.agent_id)
            .map_err(status_from_error)?
            .clear_session(params, expected_history_cursor)
            .await;
        Ok(Response::new(empty_response(result)))
    }

    async fn close_session(
        &self,
        request: Request<pb::SessionRequest>,
    ) -> Result<Response<pb::EmptyResponse>, Status> {
        let request = request.into_inner();
        let auth = node_pb::decode_auth(request.auth).map_err(status_from_error)?;
        self.authorize(&auth).map_err(status_from_error)?;
        let params = harness_pb::decode_session_context(
            request.params,
            &self.config.read().expect("Worker config lock poisoned"),
        )
        .map_err(status_from_error)?;
        let peer = self
            .coordinator_id(request.caller.as_ref(), &params)
            .map_err(status_from_error)?;
        let expected_history_cursor = agent_protocol::execution_state_pb::decode_history_cursor(
            request.expected_history_cursor.ok_or_else(|| {
                status_from_error(TalosError::new(
                    ErrorCode::InvalidRequest,
                    "Session history cursor is required",
                ))
            })?,
        )
        .map_err(status_from_error)?;
        let _reservation = self
            .session_gate
            .maintenance(&params, peer, &expected_history_cursor)
            .map_err(status_from_error)?;
        let result = self
            .harness(&params.agent_id)
            .map_err(status_from_error)?
            .close_session(params, expected_history_cursor)
            .await;
        Ok(Response::new(empty_response(result)))
    }

    async fn read_workspace_memory(
        &self,
        request: Request<pb::SessionRequest>,
    ) -> Result<Response<pb::WorkspaceMemoryResponse>, Status> {
        let request = request.into_inner();
        let context = self.memory_context(request.auth, request.params)?;
        let harness = self.harness(&context.agent_id).map_err(status_from_error)?;
        let result = harness.read_workspace_memory(context).await;
        Ok(Response::new(pb::WorkspaceMemoryResponse {
            result: Some(
                match result.and_then(agent_protocol::memory_pb::encode_snapshot) {
                    Ok(snapshot) => pb::workspace_memory_response::Result::Snapshot(snapshot),
                    Err(error) => pb::workspace_memory_response::Result::Failure(failure(&error)),
                },
            ),
        }))
    }

    async fn update_workspace_memory(
        &self,
        request: Request<pb::UpdateWorkspaceMemoryRequest>,
    ) -> Result<Response<pb::WorkspaceMemoryResponse>, Status> {
        let request = request.into_inner();
        let params = request.params.ok_or_else(|| {
            status_from_error(TalosError::new(
                ErrorCode::InvalidRequest,
                "missing memory params",
            ))
        })?;
        let context = self.memory_context(request.auth, params.context)?;
        let update = agent_protocol::memory_pb::decode_update(params.update.ok_or_else(|| {
            status_from_error(TalosError::new(
                ErrorCode::InvalidRequest,
                "missing memory update",
            ))
        })?)
        .map_err(status_from_error)?;
        let harness = self.harness(&context.agent_id).map_err(status_from_error)?;
        let result = harness.update_workspace_memory(context, update).await;
        Ok(Response::new(pb::WorkspaceMemoryResponse {
            result: Some(
                match result.and_then(agent_protocol::memory_pb::encode_snapshot) {
                    Ok(snapshot) => pb::workspace_memory_response::Result::Snapshot(snapshot),
                    Err(error) => pb::workspace_memory_response::Result::Failure(failure(&error)),
                },
            ),
        }))
    }

    async fn form_dream_memory(
        &self,
        request: Request<Streaming<pb::FormDreamMemoryRequest>>,
    ) -> Result<Response<Self::FormDreamMemoryStream>, Status> {
        self.serve_dream_memory(Box::pin(request.into_inner()))
            .await
    }

    async fn shutdown(
        &self,
        request: Request<pb::ShutdownRequest>,
    ) -> Result<Response<pb::EmptyResponse>, Status> {
        let request = request.into_inner();
        let auth = node_pb::decode_auth(request.auth).map_err(status_from_error)?;
        self.authorize(&auth).map_err(status_from_error)?;
        let params = request.params.ok_or_else(|| {
            status_from_error(TalosError::new(
                ErrorCode::InvalidRequest,
                "missing shutdown params",
            ))
        })?;
        let harness = self.harness(&params.agent_id).map_err(status_from_error)?;
        let result = harness
            .shutdown(Duration::from_millis(params.timeout_ms))
            .await;
        if result.is_ok() {
            let mut harnesses = self.harnesses.lock().expect("Agent harness lock poisoned");
            if harnesses
                .get(&params.agent_id)
                .is_some_and(|current| Arc::ptr_eq(current, &harness))
            {
                harnesses.remove(&params.agent_id);
            }
        }
        Ok(Response::new(empty_response(result)))
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
        result: Some(pb::run_response::Result::Failure(failure(&error))),
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
