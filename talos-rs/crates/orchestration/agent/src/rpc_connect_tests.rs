//! Deterministic carrier regression; real Provider acceptance lives in group 18.
use super::{test_support::*, Harness};
use crate::AgentRunSettlement;
use agent_client::AgentHarnessRpcAdapter;
use agent_client::{AgentExecutionConnection, AgentNodeClient, AgentTlsTrust, RemoteAgentClient};
use agent_pb as pb;
use agent_protocol::{
    AgentHarness, AgentInputParams, AgentInterruptParams, LeaseValidity, NodeAuth, Registration,
};
use std::{
    pin::Pin,
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
use tokio::{sync::mpsc, time::Instant};
use tokio_stream::{wrappers::ReceiverStream, StreamExt};
use tokio_util::sync::CancellationToken;
use tonic::{
    transport::{Identity, Server, ServerTlsConfig},
    Request, Response, Status, Streaming,
};

struct DreamHarness(Arc<dyn AgentHarness>);
impl AgentHarness for DreamHarness {
    fn start(
        &self,
        params: agent_protocol::AgentRunParams,
    ) -> agent_protocol::AgentFuture<
        '_,
        agent_protocol::AgentResult<agent_protocol::AgentRunExecution>,
    > {
        self.0.start(params)
    }
    fn submit_input(
        &self,
        params: AgentInputParams,
    ) -> agent_protocol::AgentFuture<'_, agent_protocol::AgentResult<agent_protocol::AgentAdmission>>
    {
        self.0.submit_input(params)
    }
    fn interrupt(
        &self,
        params: AgentInterruptParams,
    ) -> agent_protocol::AgentFuture<
        '_,
        agent_protocol::AgentResult<talos_agent_api::AgentInterruptOutcome>,
    > {
        self.0.interrupt(params)
    }
    fn compact_session(
        &self,
        context: agent_protocol::AgentContext,
    ) -> agent_protocol::AgentFuture<
        '_,
        agent_protocol::AgentResult<agent_protocol::AgentRunExecution>,
    > {
        self.0.compact_session(context)
    }
    fn clear_session(
        &self,
        context: agent_protocol::AgentSessionContext,
        expected_history_cursor: talos_types::SessionHistoryCursor,
    ) -> agent_protocol::AgentFuture<'_, agent_protocol::AgentResult<()>> {
        self.0.clear_session(context, expected_history_cursor)
    }
    fn close_session(
        &self,
        context: agent_protocol::AgentSessionContext,
        expected_history_cursor: talos_types::SessionHistoryCursor,
    ) -> agent_protocol::AgentFuture<'_, agent_protocol::AgentResult<()>> {
        self.0.close_session(context, expected_history_cursor)
    }
    fn shutdown(
        &self,
        timeout: Duration,
    ) -> agent_protocol::AgentFuture<'_, agent_protocol::AgentResult<()>> {
        self.0.shutdown(timeout)
    }
    fn form_dream_memory(
        &self,
        _: agent_protocol::AgentSessionContext,
        model_call: agent_protocol::DreamMemoryModelCall,
    ) -> agent_protocol::AgentFuture<'_, agent_protocol::AgentResult<Option<u64>>> {
        Box::pin(async move {
            let reply = model_call(agent_protocol::dream_pb::decode_prompt(
                pb::DreamMemoryModelPrompt {
                    system: "Form memory".into(),
                    user: "Session facts".into(),
                },
            ))
            .await?;
            assert_eq!(reply.content, "formed memory");
            assert_eq!(reply.usage.total_tokens, 3);
            Ok(Some(17))
        })
    }
}

struct Control(mpsc::Sender<Arc<AgentExecutionConnection>>);
#[tonic::async_trait]
impl pb::agent_node_control_server::AgentNodeControl for Control {
    type ConnectStream =
        Pin<Box<dyn tokio_stream::Stream<Item = Result<pb::ControlToNodeFrame, Status>> + Send>>;
    async fn connect(
        &self,
        request: Request<Streaming<pb::NodeToControlFrame>>,
    ) -> Result<Response<Self::ConnectStream>, Status> {
        let mut input = request.into_inner();
        let first = input.message().await?.unwrap();
        let Some(pb::node_to_control_frame::Body::Connect(first)) = first.body else {
            unreachable!()
        };
        let auth = agent_protocol::node_pb::decode_auth(first.auth).unwrap();
        let (connection, output) = AgentExecutionConnection::accept(auth, input);
        connection.ready().unwrap();
        self.0.send(connection).await.unwrap();
        Ok(Response::new(Box::pin(ReceiverStream::new(output).map(Ok))))
    }
    async fn register(
        &self,
        _: Request<pb::RegisterRequest>,
    ) -> Result<Response<pb::RegisterResponse>, Status> {
        unreachable!()
    }
    async fn renew_lease(
        &self,
        _: Request<pb::RenewLeaseRequest>,
    ) -> Result<Response<pb::RenewLeaseResponse>, Status> {
        unreachable!()
    }
    async fn heartbeat(
        &self,
        _: Request<pb::HeartbeatRequest>,
    ) -> Result<Response<pb::HeartbeatResponse>, Status> {
        unreachable!()
    }
    async fn unregister(
        &self,
        _: Request<pb::UnregisterRequest>,
    ) -> Result<Response<pb::UnregisterResponse>, Status> {
        unreachable!()
    }
}

// A second management node has no incoming listener: its SDK opens an
// authenticated outgoing stream and receives the original Worker protocol.
struct RelayControl(
    Arc<AgentExecutionConnection>,
    CancellationToken,
    mpsc::Sender<Arc<AgentExecutionConnection>>,
);
#[tonic::async_trait]
impl pb::agent_coordinator_rpc_server::AgentCoordinatorRpc for RelayControl {
    async fn invoke_active_command(
        &self,
        _: tonic::Request<pb::ActiveSessionCommandRequest>,
    ) -> Result<tonic::Response<pb::ActiveSessionCommandResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented(
            "This transport fixture has no management Service.",
        ))
    }
    type ConnectStream = Pin<
        Box<dyn tokio_stream::Stream<Item = Result<pb::ControlToCoordinatorFrame, Status>> + Send>,
    >;
    async fn connect(
        &self,
        request: Request<Streaming<pb::CoordinatorToControlFrame>>,
    ) -> Result<Response<Self::ConnectStream>, Status> {
        assert_eq!(
            request.metadata().get("authorization").unwrap(),
            "Bearer account-secret"
        );
        let mut input = request.into_inner();
        let first = input.message().await?.unwrap();
        let Some(pb::coordinator_to_control_frame::Body::Connect(first)) = first.body else {
            unreachable!()
        };
        assert_eq!(first.computer_id, "node-1");
        let auth = NodeAuth {
            node_id: first.computer_id,
            token: "account-secret".into(),
        };
        let (output, task, resources) = agent_client::serve_coordinator_connection(
            Arc::new(agent_client::AgentEnvironmentClient::from_connection(
                self.0.clone(),
                NodeAuth {
                    node_id: "node-1".into(),
                    token: "lease-secret".into(),
                },
            )),
            auth,
            uuid::Uuid::new_v4().to_string(),
            first.peer_id,
            input,
            self.1.child_token(),
        )
        .await;
        self.2.send(resources).await.unwrap();
        tokio::spawn(async move {
            let _ = task.await.unwrap();
        });
        Ok(Response::new(output))
    }
}

struct Connected {
    client: RemoteAgentClient,
    connection: Arc<AgentExecutionConnection>,
    adapter: AgentHarnessRpcAdapter,
    stop: CancellationToken,
    worker: tokio::task::JoinHandle<agent_protocol::AgentResult<()>>,
    server: tokio::task::JoinHandle<()>,
    relay_server: Option<tokio::task::JoinHandle<()>>,
    relay_connection: Option<Arc<AgentExecutionConnection>>,
    callback_connection: Option<Arc<AgentExecutionConnection>>,
    callbacks: Arc<CallbackProbe>,
    caller_scope: Arc<std::sync::RwLock<Option<pb::CoordinatorCallContext>>>,
}
impl Connected {
    async fn new(harness: Arc<dyn AgentHarness>) -> Self {
        Self::connect(vec![(TEST_AGENT_ID.into(), harness)], false).await
    }
    async fn through_coordinator(harness: Arc<dyn AgentHarness>) -> Self {
        Self::connect(vec![(TEST_AGENT_ID.into(), harness)], true).await
    }
    async fn with_harnesses(harnesses: Vec<(String, Arc<dyn AgentHarness>)>) -> Self {
        Self::connect(harnesses, true).await
    }
    async fn connect(harnesses: Vec<(String, Arc<dyn AgentHarness>)>, relay: bool) -> Self {
        let certificate = rcgen::generate_simple_self_signed(vec!["127.0.0.1".into()]).unwrap();
        let pem = certificate.cert.pem().into_bytes();
        let identity = Identity::from_pem(&pem, certificate.signing_key.serialize_pem());
        let relay_identity = identity.clone();
        let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = socket.local_addr().unwrap();
        drop(socket);
        let stop = CancellationToken::new();
        let server_stop = stop.clone();
        let (accepted, mut ready) = mpsc::channel(1);
        let server = tokio::spawn(async move {
            Server::builder()
                .tls_config(ServerTlsConfig::new().identity(identity))
                .unwrap()
                .add_service(pb::agent_node_control_server::AgentNodeControlServer::new(
                    Control(accepted),
                ))
                .serve_with_shutdown(address, server_stop.cancelled())
                .await
                .unwrap();
        });
        let home = tempfile::tempdir().unwrap();
        let adapter = AgentHarnessRpcAdapter::new(
            "node-1".into(),
            Arc::new(std::sync::RwLock::new(
                talos_config::Config::default()
                    .with_runtime_paths(talos_config::TalosPaths::from_home(home.path().to_owned()))
                    .with_workspace_definition(talos_config::WorkspaceDefinition::new(
                        "workspace-1",
                        "/tmp",
                    )),
            )),
            harnesses,
        );
        let worker_home = home;
        let caller_scope = Arc::new(std::sync::RwLock::new(None));
        let scope = caller_scope.clone();
        let adapter = adapter.with_resource_binder(Arc::new(move |_, _, caller| {
            *scope.write().unwrap() = caller.cloned();
            Ok(Box::new(()))
        }));
        adapter
            .install_registration(
                Registration {
                    lease_token: "lease-secret".into(),
                    owner_user_id: "user-1".into(),
                    validity: LeaseValidity {
                        expires_at: 90,
                        server_at: 0,
                    },
                },
                Instant::now() + Duration::from_secs(90),
            )
            .unwrap();
        let auth = NodeAuth {
            node_id: "node-1".into(),
            token: "lease-secret".into(),
        };
        let node = AgentNodeClient::connect(
            &format!("https://{address}"),
            AgentTlsTrust::Certificate(pem.clone()),
        )
        .await
        .unwrap();
        let worker_adapter = adapter.clone();
        let worker_auth = auth.clone();
        let worker_stop = stop.child_token();
        let worker = tokio::spawn(async move {
            let _home = worker_home;
            node.run_execution_connection(worker_auth, worker_adapter, worker_stop)
                .await
        });
        let connection = tokio::time::timeout(Duration::from_secs(5), ready.recv())
            .await
            .unwrap()
            .unwrap();
        let callbacks = Arc::new(CallbackProbe::default());
        let (relay_server, relay_connection, callback_connection, client) = if relay {
            let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let address = socket.local_addr().unwrap();
            drop(socket);
            let server_stop = stop.clone();
            let (callback_sender, mut callback_ready) = mpsc::channel(1);
            let service = RelayControl(connection.clone(), stop.clone(), callback_sender);
            let server = tokio::spawn(async move {
                Server::builder()
                    .tls_config(ServerTlsConfig::new().identity(relay_identity))
                    .unwrap()
                    .add_service(
                        pb::agent_coordinator_rpc_server::AgentCoordinatorRpcServer::new(service),
                    )
                    .serve_with_shutdown(address, server_stop.cancelled())
                    .await
                    .unwrap();
            });
            let caller = AgentNodeClient::connect(
                &format!("https://{address}"),
                AgentTlsTrust::Certificate(pem),
            )
            .await
            .unwrap();
            let (peer, auth) = caller
                .connect_coordinator_with_resources(
                    "account-secret",
                    &uuid::Uuid::new_v4().to_string(),
                    "node-1",
                    callbacks.clone(),
                )
                .await
                .unwrap();
            let client =
                RemoteAgentClient::from_connection(peer.clone(), TEST_AGENT_ID.into(), auth);
            (
                Some(server),
                Some(peer),
                callback_ready.recv().await,
                client,
            )
        } else {
            (
                None,
                None,
                None,
                RemoteAgentClient::from_connection(connection.clone(), TEST_AGENT_ID.into(), auth),
            )
        };
        Self {
            client,
            connection,
            adapter,
            stop,
            worker,
            server,
            relay_server,
            relay_connection,
            callback_connection,
            callbacks,
            caller_scope,
        }
    }
    async fn close(self) {
        if let Some(peer) = self.relay_connection {
            peer.close();
        }
        self.connection.close();
        // Stop and drain the carrier before revoking its lease; buffered receipt
        // acknowledgements still belong to this live connection until it exits.
        self.stop.cancel();
        let result = self.worker.await.unwrap();
        self.adapter.clear_registration();
        assert!(
            result.is_ok()
                || result
                    .as_ref()
                    .err()
                    .is_some_and(|error| error.is_code(talos_error::ErrorCode::ServiceUnavailable)),
            "{result:?}"
        );
        self.adapter.drain_disconnected().await.unwrap();
        self.stop.cancel();
        self.server.await.unwrap();
        if let Some(server) = self.relay_server {
            server.await.unwrap();
        }
    }
}

#[tokio::test]
async fn coordinator_relay_preserves_admission_input_settlement_and_management() {
    let provider = FakeProvider::new(FakeMode::Input);
    let fixture = Connected::through_coordinator(Arc::new(Harness::new(
        TEST_AGENT_ID.into(),
        provider.clone(),
    )))
    .await;
    let identity = talos_agent_api::AgentTurnRef::new("session-1", "outbound-input");
    let mut execution = fixture
        .client
        .start(run_params(identity.clone(), "message-1"))
        .await
        .unwrap();
    assert!(!provider.started.load(Ordering::Acquire));
    execution.admission.permit.consume().await.unwrap();
    provider.wait_started().await;
    let input = fixture
        .client
        .submit_input(AgentInputParams {
            agent_id: TEST_AGENT_ID.into(),
            identity: identity.clone(),
            input: user_message(&identity, "message-2"),
        })
        .await
        .unwrap();
    assert!(!provider.input_received.load(Ordering::Acquire));
    input.permit.consume().await.unwrap();
    while execution.events.recv().await.is_some() {}
    let history_cursor = super::rpc_tests::commit_received_settlement(
        &fixture.client,
        execution.settlement.recv().await.unwrap(),
    )
    .await;
    let mut compact_context = context(talos_agent_api::AgentTurnRef::new(
        "session-1",
        "outbound-compact",
    ));
    compact_context.history_cursor = Some(history_cursor);
    let compact = fixture
        .client
        .compact_session(compact_context)
        .await
        .unwrap();
    compact.admission.permit.consume().await.unwrap();
    let history_cursor = super::rpc_tests::commit_received_settlement(
        &fixture.client,
        compact.settlement.recv().await.unwrap(),
    )
    .await;
    // A host without Workspace memory preserves its domain failure across both unary RPCs.
    assert!(fixture
        .client
        .read_workspace_memory(session_params())
        .await
        .unwrap_err()
        .is_code(talos_error::ErrorCode::InvalidRequest));
    assert!(fixture
        .client
        .update_workspace_memory(
            session_params(),
            talos_memory::UserWorkspaceMemoryUpdate::RemoveEntry {
                key: "preference".into(),
            }
        )
        .await
        .unwrap_err()
        .is_code(talos_error::ErrorCode::InvalidRequest));
    fixture
        .client
        .clear_session(session_params(), history_cursor.clone())
        .await
        .unwrap();
    fixture
        .client
        .close_session(session_params(), history_cursor)
        .await
        .unwrap();
    fixture
        .client
        .shutdown(Duration::from_secs(5))
        .await
        .unwrap();
    assert!(
        provider.clear_called.load(Ordering::Acquire)
            && provider.close_called.load(Ordering::Acquire)
            && provider.shutdown_called.load(Ordering::Acquire)
    );
    fixture.close().await;
}

#[tokio::test]
async fn outbound_cancel_before_commit_and_disconnect_after_commit_release_execution() {
    let provider = FakeProvider::new(FakeMode::Release);
    let fixture = Connected::new(Arc::new(Harness::new(
        TEST_AGENT_ID.into(),
        provider.clone(),
    )))
    .await;
    let first = fixture
        .client
        .start(run_params(
            talos_agent_api::AgentTurnRef::new("session-1", "not-committed"),
            "message-1",
        ))
        .await
        .unwrap();
    drop(first);
    assert!(!provider.started.load(Ordering::Acquire));
    let identity = talos_agent_api::AgentTurnRef::new("session-2", "committed");
    let run = fixture
        .client
        .start(run_params(identity.clone(), "message-2"))
        .await
        .unwrap();
    run.admission.permit.consume().await.unwrap();
    provider.wait_started().await;
    fixture.connection.close();
    let error = run.settlement.recv().await.unwrap_err();
    assert!(error.is_code(talos_error::ErrorCode::ServiceUnavailable));
    // A network-disconnect scenario waits for the Worker carrier to observe EOF
    // before withdrawing the lease. Receipts buffered before EOF are still valid.
    tokio::time::timeout(Duration::from_secs(5), async {
        while !fixture.worker.is_finished() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    fixture.adapter.clear_registration();
    fixture.adapter.drain_disconnected().await.unwrap();
    let error = fixture
        .client
        .interrupt(AgentInterruptParams {
            agent_id: TEST_AGENT_ID.into(),
            identity,
        })
        .await
        .unwrap_err();
    assert!(error.is_code(talos_error::ErrorCode::ServiceUnavailable));
    fixture.close().await;
}

#[tokio::test]
async fn stalled_outbound_consumer_fails_only_its_call_while_other_calls_progress() {
    let provider = FakeProvider::new(FakeMode::FloodProgress);
    let independent = FakeProvider::new(FakeMode::Release);
    let fixture = Connected::with_harnesses(vec![
        (
            TEST_AGENT_ID.into(),
            Arc::new(Harness::new(TEST_AGENT_ID.into(), provider.clone())),
        ),
        (
            "agent-2".into(),
            Arc::new(DreamHarness(Arc::new(Harness::new(
                "agent-2".into(),
                independent.clone(),
            )))),
        ),
    ])
    .await;
    let independent_client = RemoteAgentClient::from_connection(
        fixture.connection.clone(),
        "agent-2".into(),
        NodeAuth {
            node_id: "node-1".into(),
            token: "lease-secret".into(),
        },
    );
    let mut blocked = fixture
        .client
        .start(run_params(
            talos_agent_api::AgentTurnRef::new("session-1", "blocked"),
            "message-1",
        ))
        .await
        .unwrap();
    blocked.admission.permit.consume().await.unwrap();
    provider.wait_started().await;
    // Keep progress unread while unrelated admission, interruption and Dream callbacks run.
    let other_calls = async {
        let compact = fixture
            .client
            .compact_session(context(talos_agent_api::AgentTurnRef::new(
                "session-2",
                "independent",
            )))
            .await
            .unwrap();
        compact.admission.permit.consume().await.unwrap();
        assert!(matches!(
            compact.settlement.recv().await.unwrap(),
            AgentRunSettlement::Finished { .. }
        ));
        let identity = talos_agent_api::AgentTurnRef::new("session-3", "interrupt-independent");
        let mut params = run_params(identity.clone(), "message-3");
        params.context.session.agent_id = "agent-2".into();
        let run = independent_client.start(params).await.unwrap();
        run.admission.permit.consume().await.unwrap();
        independent.wait_started().await;
        assert_eq!(
            independent_client
                .interrupt(AgentInterruptParams {
                    agent_id: "agent-2".into(),
                    identity
                })
                .await
                .unwrap(),
            talos_agent_api::AgentInterruptOutcome::Cancelled
        );
        assert!(matches!(
            run.settlement.recv().await.unwrap(),
            AgentRunSettlement::Finished { .. }
        ));
        let model_call: agent_protocol::DreamMemoryModelCall = Arc::new(|messages| {
            assert_eq!(messages[0].content, "Form memory");
            assert_eq!(messages[1].content, "Session facts");
            Box::pin(async {
                agent_protocol::dream_pb::decode_reply(pb::DreamMemoryModelReply {
                    content: "formed memory".into(),
                    usage: Some(pb::TokenUsage {
                        input_tokens: 2,
                        output_tokens: 1,
                        total_tokens: 3,
                        ..Default::default()
                    }),
                })
            })
        });
        let mut session = session_params();
        session.agent_id = "agent-2".into();
        session.session_id = "session-4".into();
        assert_eq!(
            independent_client
                .form_dream_memory(session, model_call)
                .await
                .unwrap(),
            Some(17)
        );
    };
    let (failed, ()) = tokio::time::timeout(Duration::from_secs(8), async {
        tokio::join!(blocked.settlement.recv(), other_calls)
    })
    .await
    .unwrap();
    assert!(failed
        .unwrap_err()
        .is_code(talos_error::ErrorCode::ServiceUnavailable));
    while blocked.events.recv().await.is_some() {}
    // Transport loss alone cannot confirm the missing business terminal fact.
    assert!(fixture
        .client
        .clear_session(session_params(), Default::default())
        .await
        .unwrap_err()
        .is_code(talos_error::ErrorCode::ActiveTurnRunning));
    assert!(
        !fixture
            .client
            .read_session_execution_state(session_params())
            .await
            .unwrap()
            .unwrap()
            .handoff_ready
    );
    fixture.close().await;
}

#[tokio::test]
async fn coordinator_relay_backpressures_a_consuming_progress_stream() {
    let provider = FakeProvider::new(FakeMode::BurstProgress);
    let fixture =
        Connected::through_coordinator(Arc::new(Harness::new(TEST_AGENT_ID.into(), provider)))
            .await;
    let mut run = fixture
        .client
        .start(run_params(
            talos_agent_api::AgentTurnRef::new("session-burst", "burst"),
            "message-burst",
        ))
        .await
        .unwrap();
    run.admission.permit.consume().await.unwrap();
    let mut events = Vec::new();
    while let Some(event) = run.events.recv().await {
        events.push(event);
        // An ordinary journaling consumer is slower than a buffered token burst.
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(events.len(), 128);
    assert!(matches!(
        run.settlement.recv().await.unwrap(),
        AgentRunSettlement::Finished { .. }
    ));
    fixture
        .client
        .clear_session(session_params(), Default::default())
        .await
        .unwrap();
    fixture.close().await;
}

#[derive(Default)]
struct CallbackProbe(std::sync::Mutex<Vec<String>>);
impl agent_client::AgentResourceRpcHandler for CallbackProbe {
    fn dispatch(
        &self,
        scope: pb::CoordinatorCallContext,
        mut requests: agent_client::AgentResourceRequests,
    ) -> Pin<
        Box<
            dyn std::future::Future<Output = Result<agent_client::AgentResourceResponses, Status>>
                + Send
                + '_,
        >,
    > {
        Box::pin(async move {
            let request = requests.next().await.transpose()?.unwrap();
            let pb::worker_rpc_call::Request::ReadSessionHistory(request) = request else {
                unreachable!()
            };
            assert_eq!(request.caller.as_ref(), Some(&scope));
            assert_eq!(request.auth.unwrap().token, "account-secret");
            self.0
                .lock()
                .unwrap()
                .push(request.params.unwrap().session_id);
            Ok(Box::pin(tokio_stream::once(Ok(
                pb::worker_rpc_result::Response::ReadSessionHistory(Box::new(
                    pb::ReadSessionHistoryResponse {
                        result: Some(pb::read_session_history_response::Result::Turns(
                            pb::SessionHistoryTurns { data: vec![] },
                        )),
                    },
                )),
            ))) as agent_client::AgentResourceResponses)
        })
    }
}

#[tokio::test]
async fn reverse_callback_matches_the_live_invocation_and_rejects_other_session_scope() {
    let provider = FakeProvider::new(FakeMode::Input);
    let fixture = Connected::through_coordinator(Arc::new(Harness::new(
        TEST_AGENT_ID.into(),
        provider.clone(),
    )))
    .await;
    let identity = talos_agent_api::AgentTurnRef::new("session-1", "callback-turn");
    let run = fixture
        .client
        .start(run_params(identity.clone(), "message-callback"))
        .await
        .unwrap();
    run.admission.permit.consume().await.unwrap();
    provider.wait_started().await;
    let caller = fixture.caller_scope.read().unwrap().clone().unwrap();
    let call = |caller: &pb::CoordinatorCallContext| {
        pb::worker_rpc_call::Request::ReadSessionHistory(Box::new(pb::ReadSessionHistoryRequest {
            caller: Some(caller.clone()),
            auth: None,
            params: Some(pb::ReadSessionHistoryParams {
                session_id: "session-1".into(),
                completed_event_seq: None,
                limit: 1,
            }),
        }))
    };
    let connection = fixture.callback_connection.as_ref().unwrap();
    let mut response = connection
        .invoke_resource(tokio_stream::once(Ok(call(&caller))), caller.clone(), 5000)
        .await
        .unwrap();
    assert!(matches!(
        response.next().await.unwrap().unwrap(),
        pb::worker_rpc_result::Response::ReadSessionHistory(_)
    ));
    assert_eq!(*fixture.callbacks.0.lock().unwrap(), ["session-1"]);
    let mut wrong = caller;
    wrong.session.as_mut().unwrap().session_id = "other-session".into();
    let mut response = connection
        .invoke_resource(tokio_stream::once(Ok(call(&wrong))), wrong, 5000)
        .await
        .unwrap();
    let error =
        agent_protocol::error_pb::decode_status(response.next().await.unwrap().unwrap_err());
    assert!(error.is_code(talos_error::ErrorCode::PermissionDenied));
    assert_eq!(fixture.callbacks.0.lock().unwrap().len(), 1);
    fixture
        .client
        .interrupt(AgentInterruptParams {
            agent_id: TEST_AGENT_ID.into(),
            identity,
        })
        .await
        .unwrap();
    drop(run.events);
    drop(run.settlement);
    fixture.close().await;
}
