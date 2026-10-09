//! Network projection of the existing in-process Harness contract.

use std::collections::{HashMap, HashSet};
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
use tokio_util::sync::CancellationToken;
use tonic::{Request, Response, Status, Streaming};

use crate::{AgentHarness, AgentInterruptParams, AgentRunExecution};

#[path = "rpc_dream.rs"]
mod dream;

type RpcStream<T> = Pin<Box<dyn tokio_stream::Stream<Item = Result<T, Status>> + Send>>;
type CommitFuture = Pin<Box<dyn Future<Output = TalosResult<()>> + Send>>;
type TurnKey = (String, String, String);

#[derive(Clone)]
struct Lease {
    token_hash: [u8; 32],
    owner_user_id: String,
    deadline: watch::Sender<Instant>,
    revoked: CancellationToken,
}

impl Lease {
    async fn ended(&self) {
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
    config: Arc<Config>,
    harnesses: Arc<Mutex<HashMap<String, Arc<dyn AgentHarness>>>>,
    lease: Arc<Mutex<Option<Lease>>>,
    active: Arc<Mutex<HashSet<TurnKey>>>,
}

impl AgentHarnessRpcAdapter {
    pub fn new(
        node_id: String,
        config: Arc<Config>,
        harnesses: Vec<(String, Arc<dyn AgentHarness>)>,
    ) -> Self {
        Self {
            node_id,
            config,
            harnesses: Arc::new(Mutex::new(harnesses.into_iter().collect())),
            lease: Arc::new(Mutex::new(None)),
            active: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Renew the same lease in place, or replace a registration and cancel its
    /// old streams. Renewal must not change the execution's lease identity.
    pub fn install_registration(
        &self,
        registration: Registration,
        deadline: Instant,
    ) -> TalosResult<()> {
        if deadline <= Instant::now()
            || registration.lease_token.is_empty()
            || registration.owner_user_id.is_empty()
        {
            return Err(invalid_request());
        }
        let lease = Lease {
            token_hash: Sha256::digest(registration.lease_token.as_bytes()).into(),
            owner_user_id: registration.owner_user_id,
            deadline: watch::channel(deadline).0,
            revoked: CancellationToken::new(),
        };
        let mut current = self.lease.lock().expect("Agent lease lock poisoned");
        if let Some(previous) = current.as_ref() {
            if previous.token_hash == lease.token_hash
                && previous.owner_user_id == lease.owner_user_id
            {
                previous.deadline.send_replace(deadline);
                return Ok(());
            }
        }
        if let Some(previous) = current.replace(lease) {
            previous.revoked.cancel();
        }
        Ok(())
    }

    pub fn clear_registration(&self) {
        if let Some(previous) = self.lease.lock().expect("Agent lease lock poisoned").take() {
            previous.revoked.cancel();
        }
    }

    fn authorize(&self, auth: &NodeAuth) -> TalosResult<Lease> {
        if auth.node_id != self.node_id || auth.token.is_empty() {
            return Err(unauthenticated());
        }
        let lease = self
            .lease
            .lock()
            .expect("Agent lease lock poisoned")
            .clone()
            .ok_or_else(unauthenticated)?;
        let actual: [u8; 32] = Sha256::digest(auth.token.as_bytes()).into();
        if !bool::from(actual.ct_eq(&lease.token_hash))
            || lease.revoked.is_cancelled()
            || *lease.deadline.borrow() <= Instant::now()
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

    fn check_owner(lease: &Lease, user_id: &str) -> TalosResult<()> {
        if user_id != lease.owner_user_id {
            return Err(TalosError::new(
                ErrorCode::PermissionDenied,
                "Agent Session does not belong to node owner",
            ));
        }
        Ok(())
    }

    fn memory_context(
        &self,
        auth: Option<pb::NodeAuth>,
        context: Option<pb::SessionContext>,
    ) -> Result<crate::AgentSessionContext, Status> {
        let auth = node_pb::decode_auth(auth).map_err(status_from_error)?;
        let lease = self.authorize(&auth).map_err(status_from_error)?;
        let context =
            harness_pb::decode_session_context(context, &self.config).map_err(status_from_error)?;
        Self::check_owner(&lease, &context.workspace.user_id).map_err(status_from_error)?;
        Ok(context)
    }

    fn serve_run(
        &self,
        commit: CommitFuture,
        lease: Lease,
        harness: Arc<dyn AgentHarness>,
        execution: TalosResult<AgentRunExecution>,
        identity: talos_agent_api::AgentTurnRef,
        agent_id: String,
    ) -> Response<RpcStream<pb::RunResponse>> {
        let (tx, rx) = mpsc::channel(32);
        let adapter = self.clone();
        tokio::spawn(async move {
            let mut execution = match execution {
                Ok(value) => value,
                Err(error) => {
                    let _ = lease.send(&tx, Ok(run_failure(error))).await;
                    return;
                }
            };
            let admission = match harness_pb::encode_admission_events(execution.admission.events) {
                Ok(value) => value,
                Err(error) => {
                    let _ = lease.send(&tx, Ok(run_failure(error))).await;
                    return;
                }
            };
            if lease
                .send(
                    &tx,
                    Ok(pb::RunResponse {
                        result: Some(pb::run_response::Result::Admission(admission)),
                    }),
                )
                .await
                .is_err()
            {
                return;
            }
            if let Err(error) = commit.await {
                let _ = lease.send(&tx, Ok(run_failure(error))).await;
                return;
            }
            if lease.revoked.is_cancelled() || *lease.deadline.borrow() <= Instant::now() {
                return;
            }
            if let Err(error) = execution.admission.permit.consume().await {
                let _ = lease.send(&tx, Ok(run_failure(error))).await;
                return;
            }
            adapter
                .active
                .lock()
                .expect("Agent active lock poisoned")
                .insert((
                    agent_id.clone(),
                    identity.session_id.clone(),
                    identity.turn_id.clone(),
                ));
            if lease
                .send(
                    &tx,
                    Ok(pb::RunResponse {
                        result: Some(pb::run_response::Result::Accepted(pb::Empty {})),
                    }),
                )
                .await
                .is_err()
            {
                drop(execution.events);
                interrupt_disconnected(&harness, &agent_id, &identity).await;
                adapter
                    .active
                    .lock()
                    .expect("Agent active lock poisoned")
                    .remove(&(agent_id, identity.session_id, identity.turn_id));
                return;
            }
            let mut settlement = Box::pin(execution.settlement.recv());
            let mut events_open = true;
            loop {
                tokio::select! {
                    biased;
                    _ = lease.ended() => break,
                    _ = tx.closed() => break,
                    event = execution.events.recv(), if events_open => {
                        if let Some(event) = event {
                            let frame = harness_pb::encode_event(event).map(|value| pb::RunResponse { result: Some(pb::run_response::Result::Progress(value)) });
                            match frame { Ok(frame) => { if lease.send(&tx, Ok(frame)).await.is_err() { break; } }, Err(error) => { let _ = lease.send(&tx, Ok(run_failure(error))).await; break; } }
                        } else { events_open = false; }
                    }
                    result = &mut settlement, if !events_open => {
                        let frame = match result.and_then(|settlement| {
                            if let agent_protocol::AgentRunSettlement::Finished { result: Err(error), .. } = &settlement { talos_logging::report_error(error); }
                            harness_pb::encode_settlement(settlement)
                        }) {
                            Ok(value) => pb::RunResponse { result: Some(pb::run_response::Result::Settlement(value)) },
                            Err(error) => run_failure(error),
                        };
                        let _ = lease.send(&tx, Ok(frame)).await;
                        adapter.active.lock().expect("Agent active lock poisoned").remove(&(agent_id, identity.session_id, identity.turn_id));
                        return;
                    }
                }
            }
            // A disconnected client or revoked lease is never a confirmed
            // business result. The existing Harness owns cancellation cleanup.
            drop(execution.events);
            interrupt_disconnected(&harness, &agent_id, &identity).await;
            adapter
                .active
                .lock()
                .expect("Agent active lock poisoned")
                .remove(&(agent_id, identity.session_id, identity.turn_id));
        });
        Response::new(Box::pin(ReceiverStream::new(rx)))
    }
}

#[tonic::async_trait]
impl pb::agent_harness_rpc_server::AgentHarnessRpc for AgentHarnessRpcAdapter {
    type StartStream = RpcStream<pb::RunResponse>;
    type SubmitInputStream = RpcStream<pb::InputResponse>;
    type CompactSessionStream = RpcStream<pb::RunResponse>;
    type FormDreamMemoryStream = RpcStream<pb::FormDreamMemoryResponse>;

    async fn start(
        &self,
        request: Request<Streaming<pb::StartRequest>>,
    ) -> Result<Response<Self::StartStream>, Status> {
        let mut inbound = request.into_inner();
        let first = inbound.message().await?.ok_or_else(|| {
            status_from_error(TalosError::new(
                ErrorCode::InvalidRequest,
                "missing Agent request",
            ))
        })?;
        let auth = node_pb::decode_auth(first.auth).map_err(status_from_error)?;
        let lease = self.authorize(&auth).map_err(status_from_error)?;
        let Some(pb::start_params::Body::Run(params)) = first.params.and_then(|value| value.body)
        else {
            return Err(status_from_error(TalosError::new(
                ErrorCode::InvalidRequest,
                "missing run params",
            )));
        };
        let params =
            harness_pb::decode_run_params(params, &self.config).map_err(status_from_error)?;
        Self::check_owner(&lease, &params.context.session.workspace.user_id)
            .map_err(status_from_error)?;
        let identity = params.context.identity();
        let agent_id = params.context.session.agent_id.clone();
        let harness = self.harness(&agent_id).map_err(status_from_error)?;
        let execution = harness.start(params).await;
        let commit = Box::pin(read_commit(
            inbound,
            start_commit,
            self.clone(),
            lease.clone(),
        ));
        Ok(self.serve_run(commit, lease, harness, execution, identity, agent_id))
    }

    async fn compact_session(
        &self,
        request: Request<Streaming<pb::CompactSessionRequest>>,
    ) -> Result<Response<Self::CompactSessionStream>, Status> {
        let mut inbound = request.into_inner();
        let first = inbound.message().await?.ok_or_else(|| {
            status_from_error(TalosError::new(
                ErrorCode::InvalidRequest,
                "missing Agent request",
            ))
        })?;
        let auth = node_pb::decode_auth(first.auth).map_err(status_from_error)?;
        let lease = self.authorize(&auth).map_err(status_from_error)?;
        let Some(pb::compact_session_params::Body::Context(context)) =
            first.params.and_then(|value| value.body)
        else {
            return Err(status_from_error(TalosError::new(
                ErrorCode::InvalidRequest,
                "missing compaction context",
            )));
        };
        let context =
            harness_pb::decode_context(Some(context), &self.config).map_err(status_from_error)?;
        Self::check_owner(&lease, &context.session.workspace.user_id).map_err(status_from_error)?;
        let identity = context.identity();
        let agent_id = context.session.agent_id.clone();
        let harness = self.harness(&agent_id).map_err(status_from_error)?;
        let execution = harness.compact_session(context).await;
        let commit = Box::pin(read_commit(
            inbound,
            compact_commit,
            self.clone(),
            lease.clone(),
        ));
        Ok(self.serve_run(commit, lease, harness, execution, identity, agent_id))
    }

    async fn submit_input(
        &self,
        request: Request<Streaming<pb::SubmitInputRequest>>,
    ) -> Result<Response<Self::SubmitInputStream>, Status> {
        let mut inbound = request.into_inner();
        let first = inbound.message().await?.ok_or_else(|| {
            status_from_error(TalosError::new(
                ErrorCode::InvalidRequest,
                "missing Agent request",
            ))
        })?;
        let auth = node_pb::decode_auth(first.auth).map_err(status_from_error)?;
        let lease = self.authorize(&auth).map_err(status_from_error)?;
        let Some(pb::submit_input_params::Body::Input(params)) =
            first.params.and_then(|value| value.body)
        else {
            return Err(status_from_error(TalosError::new(
                ErrorCode::InvalidRequest,
                "missing input params",
            )));
        };
        let params = harness_pb::decode_input_params(params).map_err(status_from_error)?;
        if !self
            .active
            .lock()
            .expect("Agent active lock poisoned")
            .contains(&(
                params.agent_id.clone(),
                params.identity.session_id.clone(),
                params.identity.turn_id.clone(),
            ))
        {
            return Err(status_from_error(TalosError::new(
                ErrorCode::ResourceNotFound,
                "Agent Turn is not active on this node",
            )));
        }
        let harness = self.harness(&params.agent_id).map_err(status_from_error)?;
        let admission = harness.submit_input(params).await;
        let (tx, rx) = mpsc::channel(4);
        let adapter = self.clone();
        tokio::spawn(async move {
            let admission = match admission {
                Ok(value) => value,
                Err(error) => {
                    let _ = tx.send(Ok(input_failure(error))).await;
                    return;
                }
            };
            let events = match harness_pb::encode_admission_events(admission.events) {
                Ok(value) => value,
                Err(error) => {
                    let _ = tx.send(Ok(input_failure(error))).await;
                    return;
                }
            };
            if tx
                .send(Ok(pb::InputResponse {
                    result: Some(pb::input_response::Result::Admission(events)),
                }))
                .await
                .is_err()
            {
                return;
            }
            let commit = read_commit(inbound, input_commit, adapter, lease.clone()).await;
            if let Err(error) = commit {
                let _ = tx.send(Ok(input_failure(error))).await;
                return;
            }
            let accepted = tokio::select! {
                biased;
                _ = lease.ended() => return,
                _ = tx.closed() => return,
                result = admission.permit.consume() => result,
            };
            let frame = match accepted {
                Ok(()) => pb::InputResponse {
                    result: Some(pb::input_response::Result::Accepted(pb::Empty {})),
                },
                Err(error) => input_failure(error),
            };
            let _ = tx.send(Ok(frame)).await;
        });
        Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
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
        let result = if self
            .active
            .lock()
            .expect("Agent active lock poisoned")
            .contains(&(
                params.agent_id.clone(),
                params.identity.session_id.clone(),
                params.identity.turn_id.clone(),
            )) {
            self.harness(&params.agent_id)
                .map_err(status_from_error)?
                .interrupt(params)
                .await
        } else {
            Err(TalosError::new(
                ErrorCode::RunCleanupUnconfirmed,
                "Agent Turn is not known to this node instance",
            ))
        };
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
        let lease = self.authorize(&auth).map_err(status_from_error)?;
        let params = harness_pb::decode_session_context(request.params, &self.config)
            .map_err(status_from_error)?;
        Self::check_owner(&lease, &params.workspace.user_id).map_err(status_from_error)?;
        let result = self
            .harness(&params.agent_id)
            .map_err(status_from_error)?
            .clear_session(params)
            .await;
        Ok(Response::new(empty_response(result)))
    }

    async fn close_session(
        &self,
        request: Request<pb::SessionRequest>,
    ) -> Result<Response<pb::EmptyResponse>, Status> {
        let request = request.into_inner();
        let auth = node_pb::decode_auth(request.auth).map_err(status_from_error)?;
        let lease = self.authorize(&auth).map_err(status_from_error)?;
        let params = harness_pb::decode_session_context(request.params, &self.config)
            .map_err(status_from_error)?;
        Self::check_owner(&lease, &params.workspace.user_id).map_err(status_from_error)?;
        let result = self
            .harness(&params.agent_id)
            .map_err(status_from_error)?
            .close_session(params)
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
        self.serve_dream_memory(request).await
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
    mut inbound: Streaming<T>,
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
    let frame = tokio::time::timeout(Duration::from_secs(30), inbound.message())
        .await
        .map_err(|_| TalosError::new(ErrorCode::Timeout, "Agent admission commit timed out"))?
        .map_err(transport_error)?
        .ok_or_else(invalid_request)?;
    let auth = extract(frame)?;
    let current = adapter.authorize(&auth)?;
    if !bool::from(current.token_hash.ct_eq(&original.token_hash))
        || original.revoked.is_cancelled()
    {
        return Err(unauthenticated());
    }
    let extra = tokio::time::timeout(Duration::from_secs(30), inbound.message())
        .await
        .map_err(|_| {
            TalosError::new(
                ErrorCode::Timeout,
                "Agent admission commit stream did not close",
            )
        })?
        .map_err(transport_error)?;
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
) {
    match harness
        .interrupt(AgentInterruptParams {
            agent_id: agent_id.to_owned(),
            identity: identity.clone(),
        })
        .await
    {
        Err(error) => talos_logging::report_error(&error),
        Ok(talos_agent_api::AgentInterruptOutcome::CleanupUnconfirmed { .. }) => {
            talos_logging::report_error(&TalosError::new(
                ErrorCode::RunCleanupUnconfirmed,
                "Disconnected Agent Turn cleanup could not be confirmed",
            ));
        }
        Ok(_) => {}
    }
}

#[cfg(test)]
#[path = "rpc_backpressure_tests.rs"]
mod backpressure_tests;
