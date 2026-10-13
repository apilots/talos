//! Remote transport hidden behind the same AgentHarness port as the embedded host.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use agent_pb as pb;
use agent_protocol::{
    error_pb, harness_pb, node_pb, AgentAdmission, AgentAdmissionPermit, AgentContext, AgentFuture,
    AgentHarness, AgentInputParams, AgentInterruptParams, AgentProgressEventReceiver, AgentResult,
    AgentRunExecution, AgentRunParams, AgentRunSettlement, AgentRunSettlementReceiver,
    AgentSessionContext, NodeAuth,
};
use talos_agent_api::AgentInterruptOutcome;
use talos_error::{ErrorCode, TalosError, TalosResult};
use tokio::sync::{mpsc, oneshot};
use tokio_stream::wrappers::ReceiverStream;
use tonic::transport::Endpoint;

use crate::AgentTlsTrust;
use tonic::{Response, Status};

#[path = "remote_dream.rs"]
mod dream;

const RPC_WAIT: Duration = Duration::from_secs(30);

/// A single logical Agent deployment on a registered remote node.
#[derive(Clone)]
pub struct RemoteAgentClient {
    transport: crate::harness_transport::HarnessTransport,
    agent_id: String,
    auth: NodeAuth,
    coordinator_id: String,
}

impl RemoteAgentClient {
    /// Local hosts use the same RPC implementation and still supply NodeAuth.
    pub async fn connect_loopback(
        address: std::net::SocketAddr,
        agent_id: String,
        auth: NodeAuth,
    ) -> TalosResult<Self> {
        if agent_id.is_empty() || auth.node_id.is_empty() || auth.token.is_empty() {
            return Err(invalid_response());
        }
        Ok(Self {
            transport: crate::harness_transport::HarnessTransport::Direct(
                crate::tls::loopback_channel(address).await?,
            ),
            agent_id,
            auth,
            coordinator_id: uuid::Uuid::new_v4().to_string(),
        })
    }

    pub async fn connect(
        endpoint: &str,
        trust: AgentTlsTrust,
        agent_id: String,
        auth: NodeAuth,
    ) -> TalosResult<Self> {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
        if !endpoint.starts_with("https://")
            || agent_id.is_empty()
            || auth.node_id.is_empty()
            || auth.token.is_empty()
        {
            return Err(invalid_response());
        }
        let endpoint = Endpoint::from_shared(endpoint.to_owned()).map_err(|error| {
            TalosError::from_source(
                ErrorCode::InvalidRequest,
                "Agent Harness endpoint is invalid",
                error,
            )
        })?;
        let endpoint = endpoint
            .connect_timeout(Duration::from_secs(10))
            .tls_config(trust.into_config())
            .map_err(|error| {
                TalosError::from_source(
                    ErrorCode::InvalidRequest,
                    "Agent Harness TLS trust is invalid",
                    error,
                )
            })?;
        Ok(Self {
            transport: crate::harness_transport::HarnessTransport::Direct(endpoint.connect_lazy()),
            agent_id,
            auth,
            coordinator_id: uuid::Uuid::new_v4().to_string(),
        })
    }

    /// Bind one advertised deployment to an authenticated outbound connection.
    /// The publication owner must supply that connection's current node lease.
    pub fn from_connection(
        connection: Arc<crate::AgentExecutionConnection>,
        agent_id: String,
        auth: NodeAuth,
    ) -> Self {
        Self {
            transport: crate::harness_transport::HarnessTransport::Outbound(connection),
            agent_id,
            auth,
            coordinator_id: uuid::Uuid::new_v4().to_string(),
        }
    }

    /// Management hosts supply their persisted peer UUID before publication.
    pub fn with_coordinator_id(mut self, coordinator_id: String) -> TalosResult<Self> {
        uuid::Uuid::parse_str(&coordinator_id).map_err(|_| invalid_response())?;
        self.coordinator_id = coordinator_id;
        Ok(self)
    }
    fn execution_caller(
        &self,
        session: &AgentSessionContext,
        turn_id: Option<String>,
        project_id: Option<String>,
    ) -> pb::CoordinatorCallContext {
        pb::CoordinatorCallContext {
            coordinator_id: self.coordinator_id.clone(),
            connection_id: String::new(),
            rpc_call_id: 0,
            session: Some(harness_pb::encode_session_context(session.clone())),
            turn_id,
            project_id,
        }
    }
    fn check_agent(&self, agent_id: &str) -> AgentResult<()> {
        if agent_id != self.agent_id {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Agent Harness deployment does not match request",
            ));
        }
        Ok(())
    }

    async fn open_run<T, F>(
        &self,
        sender: mpsc::Sender<T>,
        commit: T,
        call: F,
    ) -> AgentResult<AgentRunExecution>
    where
        T: Send + 'static,
        F: Future<
                Output = Result<
                    Response<crate::transport_stream::RpcStream<pb::RunResponse>>,
                    Status,
                >,
            > + Send,
    {
        let (inbound, first) = wait_for_rpc(async {
            let mut inbound = call.await.map_err(crate::decode_rpc_status)?.into_inner();
            let first = inbound
                .message()
                .await
                .map_err(crate::decode_rpc_status)?
                .ok_or_else(invalid_response)?;
            Ok((inbound, first))
        })
        .await?;
        let admission = match first.result.ok_or_else(invalid_response)? {
            pb::run_response::Result::Admission(value) => {
                harness_pb::decode_admission_events(value)?
            }
            pb::run_response::Result::Failure(value) => {
                return Err(error_pb::decode_failure(value)?)
            }
            _ => return Err(invalid_response()),
        };
        let (progress_tx, progress_rx) = mpsc::channel(32);
        let (settlement_tx, settlement_rx) = oneshot::channel();
        let (accepted_tx, accepted_rx) = oneshot::channel();
        let stop = tokio_util::sync::CancellationToken::new();
        let task_stop = stop.clone();
        tokio::spawn(async move {
            tokio::select! {
                _ = task_stop.cancelled() => {},
                _ = read_run(inbound, accepted_tx, progress_tx, settlement_tx) => {},
            }
        });
        // Dropping or timing out an unconsumed permit closes the network stream.
        let guard = stop.drop_guard();
        let permit = AgentAdmissionPermit::new(async move {
            let result = wait_for_rpc(async {
                sender.send(commit).await.map_err(|_| transport_closed())?;
                drop(sender);
                accepted_rx.await.map_err(|_| transport_closed())?
            })
            .await;
            if result.is_ok() {
                guard.disarm();
            }
            result
        });
        Ok(AgentRunExecution {
            admission: AgentAdmission {
                events: admission,
                permit,
            },
            events: AgentProgressEventReceiver::new(progress_rx),
            settlement: AgentRunSettlementReceiver::new(settlement_rx),
        })
    }

    async fn run_start(&self, params: AgentRunParams) -> AgentResult<AgentRunExecution> {
        self.check_agent(&params.context.session.agent_id)?;
        let caller = self.execution_caller(
            &params.context.session,
            Some(params.context.turn_id.clone()),
            params.context.project_id.clone(),
        );
        let params = harness_pb::encode_run_params(params)?;
        let (tx, rx) = mpsc::channel(2);
        tx.send(pb::StartRequest {
            caller: Some(caller.clone()),
            auth: Some(node_pb::encode_auth(self.auth.clone())),
            params: Some(pb::StartParams {
                body: Some(pb::start_params::Body::Run(params)),
            }),
        })
        .await
        .map_err(|_| transport_closed())?;
        let rpc = self.transport.clone();
        let call = rpc.start(ReceiverStream::new(rx));
        self.open_run(
            tx,
            pb::StartRequest {
                caller: Some(caller.clone()),
                auth: Some(node_pb::encode_auth(self.auth.clone())),
                params: Some(pb::StartParams {
                    body: Some(pb::start_params::Body::Commit(pb::Empty {})),
                }),
            },
            call,
        )
        .await
    }

    async fn run_compact(&self, context: AgentContext) -> AgentResult<AgentRunExecution> {
        self.check_agent(&context.session.agent_id)?;
        let caller = self.execution_caller(
            &context.session,
            Some(context.turn_id.clone()),
            context.project_id.clone(),
        );
        let context = harness_pb::encode_context(context)?;
        let (tx, rx) = mpsc::channel(2);
        tx.send(pb::CompactSessionRequest {
            caller: Some(caller.clone()),
            auth: Some(node_pb::encode_auth(self.auth.clone())),
            params: Some(pb::CompactSessionParams {
                body: Some(pb::compact_session_params::Body::Context(context)),
            }),
        })
        .await
        .map_err(|_| transport_closed())?;
        let rpc = self.transport.clone();
        let call = rpc.compact_session(ReceiverStream::new(rx));
        self.open_run(
            tx,
            pb::CompactSessionRequest {
                caller: Some(caller.clone()),
                auth: Some(node_pb::encode_auth(self.auth.clone())),
                params: Some(pb::CompactSessionParams {
                    body: Some(pb::compact_session_params::Body::Commit(pb::Empty {})),
                }),
            },
            call,
        )
        .await
    }

    async fn run_input(&self, params: AgentInputParams) -> AgentResult<AgentAdmission> {
        self.check_agent(&params.agent_id)?;
        let params = harness_pb::encode_input_params(params)?;
        let (tx, rx) = mpsc::channel(2);
        tx.send(pb::SubmitInputRequest {
            caller: Some(pb::CoordinatorCallContext {
                coordinator_id: self.coordinator_id.clone(),
                ..Default::default()
            }),
            auth: Some(node_pb::encode_auth(self.auth.clone())),
            params: Some(pb::SubmitInputParams {
                body: Some(pb::submit_input_params::Body::Input(params)),
            }),
        })
        .await
        .map_err(|_| transport_closed())?;
        let (mut inbound, first) = wait_for_rpc(async {
            let mut inbound = self
                .transport
                .clone()
                .submit_input(ReceiverStream::new(rx))
                .await
                .map_err(crate::decode_rpc_status)?
                .into_inner();
            let first = inbound
                .message()
                .await
                .map_err(crate::decode_rpc_status)?
                .ok_or_else(invalid_response)?;
            Ok((inbound, first))
        })
        .await?;
        let events = match first.result.ok_or_else(invalid_response)? {
            pb::input_response::Result::Admission(value) => {
                harness_pb::decode_admission_events(value)?
            }
            pb::input_response::Result::Failure(value) => {
                return Err(error_pb::decode_failure(value)?)
            }
            _ => return Err(invalid_response()),
        };
        let auth = self.auth.clone();
        let permit = AgentAdmissionPermit::new(wait_for_rpc(async move {
            tx.send(pb::SubmitInputRequest {
                caller: None,
                auth: Some(node_pb::encode_auth(auth)),
                params: Some(pb::SubmitInputParams {
                    body: Some(pb::submit_input_params::Body::Commit(pb::Empty {})),
                }),
            })
            .await
            .map_err(|_| transport_closed())?;
            drop(tx);
            let frame = inbound
                .message()
                .await
                .map_err(crate::decode_rpc_status)?
                .ok_or_else(transport_closed)?;
            match frame.result.ok_or_else(invalid_response)? {
                pb::input_response::Result::Accepted(_) => {
                    if inbound
                        .message()
                        .await
                        .map_err(crate::decode_rpc_status)?
                        .is_some()
                    {
                        return Err(invalid_response());
                    }
                    Ok(())
                }
                pb::input_response::Result::Failure(value) => Err(error_pb::decode_failure(value)?),
                _ => Err(invalid_response()),
            }
        }));
        Ok(AgentAdmission { events, permit })
    }

    async fn run_interrupt(
        &self,
        params: AgentInterruptParams,
    ) -> AgentResult<AgentInterruptOutcome> {
        self.check_agent(&params.agent_id)?;
        let response = rpc_response(
            self.transport.clone().interrupt(rpc_request(
                pb::InterruptRequest {
                    caller: Some(pb::CoordinatorCallContext {
                        coordinator_id: self.coordinator_id.clone(),
                        ..Default::default()
                    }),
                    auth: Some(node_pb::encode_auth(self.auth.clone())),
                    params: Some(harness_pb::encode_interrupt_params(params)),
                },
                RPC_WAIT,
            )),
            RPC_WAIT,
        )
        .await?
        .into_inner();
        match response.result.ok_or_else(invalid_response)? {
            pb::interrupt_response::Result::Interrupted(value) => {
                let outcome = harness_pb::decode_interrupt_outcome(value)?;
                Ok(outcome)
            }
            pb::interrupt_response::Result::Failure(value) => {
                let error = error_pb::decode_failure(value)?;
                Err(error)
            }
        }
    }

    async fn run_session(
        &self,
        params: AgentSessionContext,
        expected_history_cursor: talos_types::SessionHistoryCursor,
        close: bool,
    ) -> AgentResult<()> {
        self.check_agent(&params.agent_id)?;
        let caller = self.execution_caller(&params, None, None);
        let request = pb::SessionRequest {
            expected_history_cursor: Some(
                agent_protocol::execution_state_pb::encode_history_cursor(expected_history_cursor),
            ),
            caller: Some(caller),
            auth: Some(node_pb::encode_auth(self.auth.clone())),
            params: Some(harness_pb::encode_session_context(params)),
        };
        let client = self.transport.clone();
        let response = wait_for_rpc(async {
            if close {
                client.close_session(rpc_request(request, RPC_WAIT)).await
            } else {
                client.clear_session(rpc_request(request, RPC_WAIT)).await
            }
            .map_err(crate::decode_rpc_status)
        })
        .await?
        .into_inner();
        decode_empty(response)
    }

    async fn run_shutdown(&self, timeout: Duration) -> AgentResult<()> {
        let timeout_ms = u64::try_from(timeout.as_millis()).map_err(|_| {
            TalosError::new(
                ErrorCode::InvalidRequest,
                "Agent shutdown timeout is too large",
            )
        })?;
        let response = rpc_response(
            self.transport.clone().shutdown(rpc_request(
                pb::ShutdownRequest {
                    auth: Some(node_pb::encode_auth(self.auth.clone())),
                    params: Some(pb::ShutdownParams {
                        agent_id: self.agent_id.clone(),
                        timeout_ms,
                    }),
                },
                timeout.saturating_add(RPC_WAIT),
            )),
            timeout.saturating_add(RPC_WAIT),
        )
        .await?
        .into_inner();
        decode_empty(response)
    }
}

impl AgentHarness for RemoteAgentClient {
    fn read_session_execution_state(
        &self,
        session: AgentSessionContext,
    ) -> AgentFuture<'_, AgentResult<Option<agent_protocol::SessionExecutionState>>> {
        Box::pin(async move {
            self.check_agent(&session.agent_id)?;
            let caller = self.execution_caller(&session, None, None);
            let response = rpc_response(
                self.transport.read_session_execution_state(rpc_request(
                    pb::ReadSessionExecutionStateRequest {
                        auth: Some(node_pb::encode_auth(self.auth.clone())),
                        params: Some(harness_pb::encode_session_context(session)),
                        caller: Some(caller),
                    },
                    RPC_WAIT,
                )),
                RPC_WAIT,
            )
            .await?
            .into_inner();
            match response.result.ok_or_else(invalid_response)? {
                pb::session_execution_state_response::Result::State(state) => {
                    agent_protocol::execution_state_pb::decode_state(state).map(Some)
                }
                pb::session_execution_state_response::Result::Failure(error) => {
                    Err(error_pb::decode_failure(error)?)
                }
            }
        })
    }
    fn commit_session_execution_state(
        &self,
        params: agent_protocol::CommitSessionExecutionStateParams,
    ) -> AgentFuture<'_, AgentResult<()>> {
        Box::pin(async move {
            self.check_agent(&params.session.agent_id)?;
            let caller = self.execution_caller(
                &params.session,
                params
                    .identity
                    .as_ref()
                    .map(|identity| identity.turn_id.clone()),
                None,
            );
            let response = rpc_response(
                self.transport.commit_session_execution_state(rpc_request(
                    pb::CommitSessionExecutionStateRequest {
                        auth: Some(node_pb::encode_auth(self.auth.clone())),
                        caller: Some(caller),
                        params: Some(pb::CommitSessionExecutionStateParams {
                            session: Some(harness_pb::encode_session_context(params.session)),
                            identity: params.identity.map(harness_pb::encode_turn_ref),
                            history_cursor: Some(
                                agent_protocol::execution_state_pb::encode_history_cursor(
                                    params.history_cursor,
                                ),
                            ),
                            visibility: match params.visibility {
                                agent_protocol::SessionStateVisibility::Local => {
                                    pb::SessionStateVisibility::Local as i32
                                }
                                agent_protocol::SessionStateVisibility::Shared => {
                                    pb::SessionStateVisibility::Shared as i32
                                }
                            },
                        }),
                    },
                    RPC_WAIT,
                )),
                RPC_WAIT,
            )
            .await?
            .into_inner();
            decode_empty(response)
        })
    }

    fn start(&self, params: AgentRunParams) -> AgentFuture<'_, AgentResult<AgentRunExecution>> {
        Box::pin(self.run_start(params))
    }
    fn submit_input(
        &self,
        params: AgentInputParams,
    ) -> AgentFuture<'_, AgentResult<AgentAdmission>> {
        Box::pin(self.run_input(params))
    }
    fn interrupt(
        &self,
        params: AgentInterruptParams,
    ) -> AgentFuture<'_, AgentResult<AgentInterruptOutcome>> {
        Box::pin(self.run_interrupt(params))
    }
    fn compact_session(
        &self,
        context: AgentContext,
    ) -> AgentFuture<'_, AgentResult<AgentRunExecution>> {
        Box::pin(self.run_compact(context))
    }
    fn clear_session(
        &self,
        params: AgentSessionContext,
        expected_history_cursor: talos_types::SessionHistoryCursor,
    ) -> AgentFuture<'_, AgentResult<()>> {
        Box::pin(self.run_session(params, expected_history_cursor, false))
    }
    fn close_session(
        &self,
        params: AgentSessionContext,
        expected_history_cursor: talos_types::SessionHistoryCursor,
    ) -> AgentFuture<'_, AgentResult<()>> {
        Box::pin(self.run_session(params, expected_history_cursor, true))
    }
    fn read_workspace_memory(
        &self,
        context: AgentSessionContext,
    ) -> AgentFuture<'_, AgentResult<talos_memory::UserWorkspaceMemorySnapshot>> {
        Box::pin(async move {
            self.check_agent(&context.agent_id)?;
            let caller = self.execution_caller(&context, None, None);
            let mut request = tonic::Request::new(pb::SessionRequest {
                expected_history_cursor: None,
                caller: Some(caller),
                auth: Some(node_pb::encode_auth(self.auth.clone())),
                params: Some(harness_pb::encode_session_context(context)),
            });
            request.set_timeout(Duration::from_secs(10));
            let response = rpc_response(
                self.transport.clone().read_workspace_memory(request),
                Duration::from_secs(10),
            )
            .await?
            .into_inner();
            match response.result.ok_or_else(invalid_response)? {
                pb::workspace_memory_response::Result::Snapshot(value) => {
                    agent_protocol::memory_pb::decode_snapshot(value)
                }
                pb::workspace_memory_response::Result::Failure(value) => {
                    Err(error_pb::decode_failure(value)?)
                }
            }
        })
    }

    fn update_workspace_memory(
        &self,
        context: AgentSessionContext,
        update: talos_memory::UserWorkspaceMemoryUpdate,
    ) -> AgentFuture<'_, AgentResult<talos_memory::UserWorkspaceMemorySnapshot>> {
        Box::pin(async move {
            self.check_agent(&context.agent_id)?;
            let mut request = tonic::Request::new(pb::UpdateWorkspaceMemoryRequest {
                auth: Some(node_pb::encode_auth(self.auth.clone())),
                params: Some(pb::UpdateWorkspaceMemoryParams {
                    context: Some(harness_pb::encode_session_context(context)),
                    update: Some(agent_protocol::memory_pb::encode_update(update)),
                }),
            });
            request.set_timeout(Duration::from_secs(10));
            let response = rpc_response(
                self.transport.clone().update_workspace_memory(request),
                Duration::from_secs(10),
            )
            .await?
            .into_inner();
            match response.result.ok_or_else(invalid_response)? {
                pb::workspace_memory_response::Result::Snapshot(value) => {
                    agent_protocol::memory_pb::decode_snapshot(value)
                }
                pb::workspace_memory_response::Result::Failure(value) => {
                    Err(error_pb::decode_failure(value)?)
                }
            }
        })
    }

    fn form_dream_memory(
        &self,
        context: AgentSessionContext,
        model_call: agent_protocol::DreamMemoryModelCall,
    ) -> AgentFuture<'_, AgentResult<Option<u64>>> {
        self.run_dream_memory(context, model_call)
    }

    fn shutdown(&self, timeout: Duration) -> AgentFuture<'_, AgentResult<()>> {
        Box::pin(self.run_shutdown(timeout))
    }
}

async fn read_run(
    mut inbound: crate::transport_stream::RpcStream<pb::RunResponse>,
    accepted_tx: oneshot::Sender<AgentResult<()>>,
    progress_tx: mpsc::Sender<talos_agent_api::AgentEvent>,
    settlement_tx: oneshot::Sender<AgentResult<AgentRunSettlement>>,
) {
    let mut accepted_tx = Some(accepted_tx);
    let mut settlement_tx = Some(settlement_tx);
    loop {
        let frame = match inbound.message().await {
            Ok(Some(value)) => value,
            Ok(None) => {
                let error = transport_closed();
                if let Some(tx) = accepted_tx.take() {
                    let _ = tx.send(Err(error.clone()));
                }
                if let Some(tx) = settlement_tx.take() {
                    let _ = tx.send(Err(error));
                }
                return;
            }
            Err(status) => {
                let error = crate::decode_rpc_status(status);
                if let Some(tx) = accepted_tx.take() {
                    let _ = tx.send(Err(error.clone()));
                }
                if let Some(tx) = settlement_tx.take() {
                    let _ = tx.send(Err(error));
                }
                return;
            }
        };
        let result = match frame.result {
            Some(value) => value,
            None => {
                let error = invalid_response();
                if let Some(tx) = accepted_tx.take() {
                    let _ = tx.send(Err(error.clone()));
                }
                if let Some(tx) = settlement_tx.take() {
                    let _ = tx.send(Err(error));
                }
                return;
            }
        };
        match result {
            pb::run_response::Result::Accepted(_) if accepted_tx.is_some() => {
                let _ = accepted_tx.take().expect("checked").send(Ok(()));
            }
            pb::run_response::Result::Progress(value) if accepted_tx.is_none() => {
                match harness_pb::decode_event(value) {
                    Ok(event) => {
                        tokio::select! {
                            biased;
                            error = inbound.failed() => {
                                if let Some(tx) = settlement_tx.take() { let _ = tx.send(Err(error)); }
                                return;
                            },
                            sent = progress_tx.send(event) => { if sent.is_err() { return; } },
                        }
                    }
                    Err(error) => {
                        if let Some(tx) = settlement_tx.take() {
                            let _ = tx.send(Err(error));
                        }
                        return;
                    }
                }
            }
            pb::run_response::Result::Settlement(value) if accepted_tx.is_none() => {
                let outcome = harness_pb::decode_settlement(value);
                if let Some(tx) = settlement_tx.take() {
                    let _ = tx.send(outcome);
                }
                return;
            }
            pb::run_response::Result::Failure(value) => {
                let error = error_pb::decode_failure(value).unwrap_or_else(|error| error);
                if let Some(tx) = accepted_tx.take() {
                    let _ = tx.send(Err(error.clone()));
                }
                if let Some(tx) = settlement_tx.take() {
                    let _ = tx.send(Err(error));
                }
                return;
            }
            _ => {
                let error = invalid_response();
                if let Some(tx) = accepted_tx.take() {
                    let _ = tx.send(Err(error.clone()));
                }
                if let Some(tx) = settlement_tx.take() {
                    let _ = tx.send(Err(error));
                }
                return;
            }
        }
    }
}

fn decode_empty(response: pb::EmptyResponse) -> TalosResult<()> {
    match response.result.ok_or_else(invalid_response)? {
        pb::empty_response::Result::Completed(_) => Ok(()),
        pb::empty_response::Result::Failure(value) => Err(error_pb::decode_failure(value)?),
    }
}

fn rpc_request<T>(body: T, timeout: Duration) -> tonic::Request<T> {
    let mut request = tonic::Request::new(body);
    request.set_timeout(timeout);
    request
}

async fn wait_for_rpc<T>(future: impl Future<Output = TalosResult<T>>) -> TalosResult<T> {
    tokio::time::timeout(RPC_WAIT, future)
        .await
        .map_err(|_| TalosError::new(ErrorCode::Timeout, "Agent RPC response timed out"))?
}

async fn rpc_response<T>(
    future: impl Future<Output = Result<T, Status>>,
    timeout: Duration,
) -> TalosResult<T> {
    tokio::time::timeout(timeout, future)
        .await
        .map_err(|error| {
            TalosError::from_source(ErrorCode::Timeout, "Agent RPC response timed out", error)
        })?
        .map_err(crate::decode_rpc_status)
}

fn invalid_response() -> TalosError {
    TalosError::internal("Agent Harness RPC response is malformed")
}
fn transport_closed() -> TalosError {
    TalosError::new(
        ErrorCode::ServiceUnavailable,
        "Agent Harness RPC stream closed before settlement",
    )
}
