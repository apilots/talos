use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use agent_client::RemoteAgentClient;
use agent_pb::agent_harness_rpc_server::AgentHarnessRpcServer;
use agent_protocol::{
    AgentHarness, AgentInputParams, AgentInterruptParams, LeaseValidity, NodeAuth, Registration,
};
use talos_agent_api::{AgentInterruptOutcome, AgentTurnRef};
use talos_config::{Config, WorkspaceDefinition};
use talos_error::ErrorCode;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use tonic::transport::{Identity, Server, ServerTlsConfig};

use super::test_support::{
    context, run_params, session_params, user_message, FakeMode, FakeProvider, TEST_AGENT_ID,
};
use super::Harness;
use crate::{AgentHarnessRpcAdapter, AgentRunSettlement};

async fn server(
    harness: Arc<dyn AgentHarness>,
) -> (
    RemoteAgentClient,
    AgentHarnessRpcAdapter,
    Vec<u8>,
    String,
    CancellationToken,
    tokio::task::JoinHandle<()>,
) {
    let home = tempfile::tempdir().unwrap();
    let certificate = rcgen::generate_simple_self_signed(vec!["127.0.0.1".to_string()]).unwrap();
    let pem = certificate.cert.pem().into_bytes();
    let identity = Identity::from_pem(&pem, certificate.signing_key.serialize_pem());
    let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = socket.local_addr().unwrap();
    drop(socket);
    let endpoint = format!("https://{address}");
    let adapter = AgentHarnessRpcAdapter::new(
        "node-1".into(),
        Arc::new(std::sync::RwLock::new(
            Config::default()
                .with_runtime_paths(talos_config::TalosPaths::from_home(home.path().to_owned()))
                .with_workspace_definition(WorkspaceDefinition::new("workspace-1", "/tmp")),
        )),
        vec![(TEST_AGENT_ID.into(), harness)],
    );
    adapter
        .install_registration(
            Registration {
                lease_token: "lease-secret".into(),
                validity: LeaseValidity {
                    expires_at: 90,
                    server_at: 0,
                },
                owner_user_id: "user-1".into(),
            },
            Instant::now() + Duration::from_secs(90),
        )
        .unwrap();
    let stop = CancellationToken::new();
    let server_stop = stop.clone();
    let adapter_for_server = adapter.clone();
    let task = tokio::spawn(async move {
        let _home = home;
        Server::builder()
            .tls_config(ServerTlsConfig::new().identity(identity))
            .unwrap()
            .add_service(AgentHarnessRpcServer::new(adapter_for_server))
            .serve_with_shutdown(address, server_stop.cancelled())
            .await
            .unwrap();
    });
    let client = RemoteAgentClient::connect(
        &endpoint,
        agent_client::AgentTlsTrust::Certificate(pem.clone()),
        TEST_AGENT_ID.into(),
        NodeAuth {
            node_id: "node-1".into(),
            token: "lease-secret".into(),
        },
    )
    .await
    .unwrap();
    (client, adapter, pem, endpoint, stop, task)
}

pub(super) async fn commit_received_settlement(
    client: &RemoteAgentClient,
    settlement: AgentRunSettlement,
) -> agent_protocol::SessionHistoryCursor {
    let AgentRunSettlement::Finished { event, .. } = settlement else {
        panic!("expected terminal settlement");
    };
    let history_cursor = agent_protocol::SessionHistoryCursor {
        history_generation: 0,
        completed_turn_id: Some(event.identity.turn_id.clone()),
        completed_event_sequence: event.sequence,
    };
    client
        .commit_session_execution_state(agent_protocol::CommitSessionExecutionStateParams {
            session: session_params(),
            identity: Some(event.identity),
            history_cursor: history_cursor.clone(),
            visibility: agent_protocol::SessionStateVisibility::Local,
        })
        .await
        .unwrap();
    history_cursor
}

#[tokio::test]
async fn direct_and_tls_rpc_use_the_same_paused_harness_execution() {
    let provider = FakeProvider::new(FakeMode::Release);
    let harness: Arc<dyn AgentHarness> =
        Arc::new(Harness::new(TEST_AGENT_ID.into(), provider.clone()));
    let identity = AgentTurnRef::new("session-1", "turn-1");
    let direct = harness
        .start(run_params(identity.clone(), "message-1"))
        .await
        .unwrap();
    assert!(!provider.started.load(Ordering::Acquire));
    let direct_sequences = direct
        .admission
        .events
        .iter()
        .map(|event| event.sequence)
        .collect::<Vec<_>>();
    direct.admission.permit.consume().await.unwrap();
    provider.wait_started().await;
    provider.release.notify_waiters();
    assert!(matches!(
        direct.settlement.recv().await.unwrap(),
        AgentRunSettlement::Finished { .. }
    ));

    let (client, _, _, _, stop, task) = server(harness).await;
    let mut remote = client
        .start(run_params(identity, "message-1"))
        .await
        .unwrap();
    assert_eq!(
        remote
            .admission
            .events
            .iter()
            .map(|event| event.sequence)
            .collect::<Vec<_>>(),
        direct_sequences
    );
    remote.admission.permit.consume().await.unwrap();
    provider.wait_started().await;
    provider.release.notify_waiters();
    while remote.events.recv().await.is_some() {}
    assert!(matches!(
        remote.settlement.recv().await.unwrap(),
        AgentRunSettlement::Finished { .. }
    ));
    stop.cancel();
    task.await.unwrap();
}

#[tokio::test]
async fn rpc_input_commit_and_new_client_interrupt_keep_cleanup_conservative() {
    let provider = FakeProvider::new(FakeMode::Input);
    let harness: Arc<dyn AgentHarness> =
        Arc::new(Harness::new(TEST_AGENT_ID.into(), provider.clone()));
    let (client, _, pem, endpoint, stop, task) = server(harness).await;
    let identity = AgentTurnRef::new("session-1", "turn-input");
    let mut run = client
        .start(run_params(identity.clone(), "message-1"))
        .await
        .unwrap();
    assert!(!provider.started.load(Ordering::Acquire));
    run.admission.permit.consume().await.unwrap();
    provider.wait_started().await;
    let new_client = RemoteAgentClient::connect(
        &endpoint,
        agent_client::AgentTlsTrust::Certificate(pem),
        TEST_AGENT_ID.into(),
        NodeAuth {
            node_id: "node-1".into(),
            token: "lease-secret".into(),
        },
    )
    .await
    .unwrap();
    let error = new_client
        .interrupt(AgentInterruptParams {
            agent_id: TEST_AGENT_ID.into(),
            identity: identity.clone(),
        })
        .await
        .unwrap_err();
    assert!(error.is_code(ErrorCode::Conflict));

    let admission = client
        .submit_input(AgentInputParams {
            agent_id: TEST_AGENT_ID.into(),
            identity: identity.clone(),
            input: user_message(&identity, "message-2"),
        })
        .await
        .unwrap();
    assert!(!provider.input_received.load(Ordering::Acquire));
    admission.permit.consume().await.unwrap();
    assert!(provider.input_received.load(Ordering::Acquire));
    while run.events.recv().await.is_some() {}
    assert!(matches!(
        run.settlement.recv().await.unwrap(),
        AgentRunSettlement::Finished { .. }
    ));
    stop.cancel();
    task.await.unwrap();
}

#[tokio::test]
async fn waiting_rpc_cancellation_and_failed_continuation_keep_the_original_boundary() {
    let provider = FakeProvider::new(FakeMode::Waiting);
    let harness: Arc<dyn AgentHarness> = Arc::new(Harness::new(TEST_AGENT_ID.into(), provider));
    let (client, _, certificate, endpoint, stop, task) = server(harness).await;
    let peer = uuid::Uuid::new_v4().to_string();
    let client = client.with_coordinator_id(peer.clone()).unwrap();
    let identity = AgentTurnRef::new("session-1", "waiting-turn");
    let run = client
        .start(run_params(identity.clone(), "question"))
        .await
        .unwrap();
    run.admission.permit.consume().await.unwrap();
    let AgentRunSettlement::WaitingForInput {
        continuation,
        event,
        metrics,
    } = run.settlement.recv().await.unwrap()
    else {
        panic!("expected waiting settlement");
    };
    let before = client
        .read_session_execution_state(session_params())
        .await
        .unwrap()
        .unwrap();
    let mut invalid = run_params(identity.clone(), "answer");
    invalid.continuation = Some(continuation.clone());
    assert!(
        matches!(client.start(invalid).await, Err(error) if error.is_code(ErrorCode::InvalidRequest))
    );
    assert_eq!(
        client
            .read_session_execution_state(session_params())
            .await
            .unwrap()
            .unwrap(),
        before
    );
    assert!(client
        .clear_session(session_params(), Default::default())
        .await
        .unwrap_err()
        .is_code(ErrorCode::ActiveTurnRunning));
    let mut resume = run_params(identity.clone(), "answer");
    resume.continuation = Some(continuation);
    resume.checkpoint = Some(agent_protocol::AgentRunCheckpoint {
        last_sequence: event.sequence,
        messages: vec![],
        metrics,
    });
    // Close the actual request stream after admission without a commit frame.
    let channel = tonic::transport::Endpoint::from_shared(endpoint)
        .unwrap()
        .tls_config(
            tonic::transport::ClientTlsConfig::new()
                .ca_certificate(tonic::transport::Certificate::from_pem(certificate)),
        )
        .unwrap()
        .connect_lazy();
    let first = agent_pb::StartRequest {
        auth: Some(agent_protocol::node_pb::encode_auth(NodeAuth {
            node_id: "node-1".into(),
            token: "lease-secret".into(),
        })),
        caller: Some(agent_pb::CoordinatorCallContext {
            coordinator_id: peer,
            session: Some(agent_protocol::harness_pb::encode_session_context(
                session_params(),
            )),
            turn_id: Some(identity.turn_id.clone()),
            ..Default::default()
        }),
        params: Some(agent_pb::StartParams {
            body: Some(agent_pb::start_params::Body::Run(
                agent_protocol::harness_pb::encode_run_params(resume).unwrap(),
            )),
        }),
    };
    let mut unfinished = agent_pb::agent_harness_rpc_client::AgentHarnessRpcClient::new(channel)
        .start(tokio_stream::iter([first]))
        .await
        .unwrap()
        .into_inner();
    assert!(matches!(
        unfinished.message().await.unwrap().unwrap().result,
        Some(agent_pb::run_response::Result::Admission(_))
    ));
    assert!(matches!(
        unfinished.message().await.unwrap().unwrap().result,
        Some(agent_pb::run_response::Result::Failure(_))
    ));
    drop(unfinished);
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            match client
                .interrupt(AgentInterruptParams {
                    agent_id: TEST_AGENT_ID.into(),
                    identity: identity.clone(),
                })
                .await
            {
                Ok(AgentInterruptOutcome::Cancelled) => break,
                Err(error) if error.is_code(ErrorCode::RunCleanupUnconfirmed) => {
                    tokio::task::yield_now().await
                }
                result => panic!("waiting cleanup changed: {result:?}"),
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(
        client
            .read_session_execution_state(session_params())
            .await
            .unwrap()
            .unwrap(),
        before
    );
    assert!(client
        .clear_session(session_params(), Default::default())
        .await
        .unwrap_err()
        .is_code(ErrorCode::ActiveTurnRunning));
    assert_eq!(
        client
            .interrupt(AgentInterruptParams {
                agent_id: TEST_AGENT_ID.into(),
                identity
            })
            .await
            .unwrap(),
        AgentInterruptOutcome::Cancelled
    );
    stop.cancel();
    task.await.unwrap();
}

#[tokio::test]
async fn tls_rpc_management_and_interrupt_preserve_harness_outcomes() {
    let provider = FakeProvider::new(FakeMode::Release);
    let harness: Arc<dyn AgentHarness> =
        Arc::new(Harness::new(TEST_AGENT_ID.into(), provider.clone()));
    let (client, _, _, _, stop, task) = server(harness).await;
    let identity = AgentTurnRef::new("session-1", "turn-management");
    let run = client
        .start(run_params(identity.clone(), "message-1"))
        .await
        .unwrap();
    run.admission.permit.consume().await.unwrap();
    provider.wait_started().await;
    assert_eq!(
        client
            .interrupt(AgentInterruptParams {
                agent_id: TEST_AGENT_ID.into(),
                identity,
            })
            .await
            .unwrap(),
        AgentInterruptOutcome::Cancelled
    );
    let history_cursor =
        commit_received_settlement(&client, run.settlement.recv().await.unwrap()).await;
    let mut compact_context = context(AgentTurnRef::new("session-1", "compact-1"));
    compact_context.history_cursor = Some(history_cursor);
    let compact = client.compact_session(compact_context).await.unwrap();
    compact.admission.permit.consume().await.unwrap();
    let history_cursor =
        commit_received_settlement(&client, compact.settlement.recv().await.unwrap()).await;
    assert!(client
        .clear_session(session_params(), Default::default())
        .await
        .unwrap_err()
        .is_code(ErrorCode::Conflict));
    assert!(client
        .close_session(session_params(), Default::default())
        .await
        .unwrap_err()
        .is_code(ErrorCode::Conflict));
    assert!(!provider.clear_called.load(Ordering::Acquire));
    assert!(!provider.close_called.load(Ordering::Acquire));
    client
        .clear_session(session_params(), history_cursor.clone())
        .await
        .unwrap();
    assert!(provider.clear_called.load(Ordering::Acquire));
    client
        .close_session(session_params(), history_cursor)
        .await
        .unwrap();
    assert!(provider.close_called.load(Ordering::Acquire));
    client.shutdown(Duration::from_secs(5)).await.unwrap();
    assert!(provider.shutdown_called.load(Ordering::Acquire));
    stop.cancel();
    task.await.unwrap();
}

#[tokio::test]
async fn revoked_lease_stops_a_live_rpc_without_reporting_completion() {
    let provider = FakeProvider::new(FakeMode::Release);
    let harness: Arc<dyn AgentHarness> =
        Arc::new(Harness::new(TEST_AGENT_ID.into(), provider.clone()));
    let (client, adapter, _, _, stop, task) = server(harness).await;
    let identity = AgentTurnRef::new("session-1", "turn-revoked");
    let run = client
        .start(run_params(identity, "message-1"))
        .await
        .unwrap();
    run.admission.permit.consume().await.unwrap();
    provider.wait_started().await;
    adapter.clear_registration();
    let error = run.settlement.recv().await.unwrap_err();
    assert!(error.is_code(ErrorCode::ServiceUnavailable));
    stop.cancel();
    task.await.unwrap();
}

#[tokio::test]
async fn local_host_execution_survives_cloud_lease_revocation_and_reconnect_drain() {
    let provider = FakeProvider::new(FakeMode::Release);
    let harness: Arc<dyn AgentHarness> =
        Arc::new(Harness::new(TEST_AGENT_ID.into(), provider.clone()));
    let (_, adapter, pem, endpoint, stop, task) = server(harness).await;
    let local_auth = NodeAuth {
        node_id: "node-1".into(),
        token: "private-host-credential".into(),
    };
    adapter.install_local_auth(&local_auth).unwrap();
    let cloud_auth = NodeAuth {
        node_id: uuid::Uuid::new_v4().to_string(),
        token: "cloud-lease-secret".into(),
    };
    adapter
        .install_node_registration(
            &cloud_auth.node_id,
            Registration {
                lease_token: cloud_auth.token.clone(),
                owner_user_id: "user-1".into(),
                validity: LeaseValidity {
                    expires_at: 120,
                    server_at: 0,
                },
            },
            Instant::now() + Duration::from_secs(120),
        )
        .unwrap();
    for auth in [
        NodeAuth {
            node_id: cloud_auth.node_id.clone(),
            token: local_auth.token.clone(),
        },
        NodeAuth {
            node_id: local_auth.node_id.clone(),
            token: cloud_auth.token.clone(),
        },
    ] {
        assert!(adapter
            .validate_connection_auth(&auth)
            .unwrap_err()
            .is_code(ErrorCode::Unauthenticated));
    }
    let remote = RemoteAgentClient::connect(
        &endpoint,
        agent_client::AgentTlsTrust::Certificate(pem.clone()),
        TEST_AGENT_ID.into(),
        cloud_auth,
    )
    .await
    .unwrap();
    let local = RemoteAgentClient::connect(
        &endpoint,
        agent_client::AgentTlsTrust::Certificate(pem),
        TEST_AGENT_ID.into(),
        local_auth,
    )
    .await
    .unwrap();
    let run = local
        .start(run_params(
            AgentTurnRef::new("session-1", "local-turn"),
            "message-1",
        ))
        .await
        .unwrap();
    run.admission.permit.consume().await.unwrap();
    provider.wait_started().await;
    adapter.clear_registration();
    adapter.drain_disconnected().await.unwrap();
    let error = match remote
        .start(run_params(
            AgentTurnRef::new("session-2", "remote-turn"),
            "message-2",
        ))
        .await
    {
        Err(error) => error,
        Ok(_) => panic!("revoked cloud credential was accepted"),
    };
    assert!(error.is_code(ErrorCode::Unauthenticated));
    provider.release.notify_waiters();
    let AgentRunSettlement::Finished { result, .. } = run.settlement.recv().await.unwrap() else {
        panic!("local Turn did not finish");
    };
    assert!(result.is_ok());
    adapter.clear_local_auth();
    let error = match local
        .start(run_params(
            AgentTurnRef::new("session-3", "closed-host"),
            "message-3",
        ))
        .await
    {
        Err(error) => error,
        Ok(_) => panic!("closed host credential was accepted"),
    };
    assert!(error.is_code(ErrorCode::Unauthenticated));
    stop.cancel();
    task.await.unwrap();
}

#[tokio::test]
async fn progress_crosses_tls_and_lease_renewal_preserves_the_live_turn() {
    let provider = FakeProvider::new(FakeMode::ProgressThenRelease);
    let harness: Arc<dyn AgentHarness> =
        Arc::new(Harness::new(TEST_AGENT_ID.into(), provider.clone()));
    let (client, adapter, _, _, stop, task) = server(harness).await;
    let identity = AgentTurnRef::new("session-1", "turn-progress");
    let mut run = client
        .start(run_params(identity, "message-1"))
        .await
        .unwrap();
    run.admission.permit.consume().await.unwrap();
    provider.wait_started().await;
    let progress = tokio::time::timeout(Duration::from_secs(2), run.events.recv())
        .await
        .unwrap();
    let mut last_sequence = progress.expect("live progress").sequence;
    adapter
        .install_registration(
            Registration {
                lease_token: "lease-secret".into(),
                validity: LeaseValidity {
                    expires_at: 180,
                    server_at: 0,
                },
                owner_user_id: "user-1".into(),
            },
            Instant::now() + Duration::from_secs(180),
        )
        .unwrap();
    provider.release.notify_waiters();
    while let Some(event) = run.events.recv().await {
        assert_eq!(event.sequence, last_sequence + 1);
        last_sequence = event.sequence;
    }
    let AgentRunSettlement::Finished { event, result, .. } = run.settlement.recv().await.unwrap()
    else {
        panic!("expected a finished Turn");
    };
    assert!(result.is_ok());
    assert_eq!(event.sequence, last_sequence + 1);
    stop.cancel();
    task.await.unwrap();
}

#[tokio::test]
async fn rpc_rejects_wrong_lease_and_dropped_admission_has_no_provider_effect() {
    let provider = FakeProvider::new(FakeMode::Release);
    let harness: Arc<dyn AgentHarness> =
        Arc::new(Harness::new(TEST_AGENT_ID.into(), provider.clone()));
    let (client, _, pem, endpoint, stop, task) = server(harness).await;
    let wrong = RemoteAgentClient::connect(
        &endpoint,
        agent_client::AgentTlsTrust::Certificate(pem),
        TEST_AGENT_ID.into(),
        NodeAuth {
            node_id: "node-1".into(),
            token: "wrong-token".into(),
        },
    )
    .await
    .unwrap();
    let identity = AgentTurnRef::new("session-1", "turn-drop");
    let error = wrong
        .start(run_params(identity.clone(), "message-1"))
        .await
        .err()
        .unwrap();
    assert!(error.is_code(ErrorCode::Unauthenticated));
    let execution = client
        .start(run_params(identity, "message-1"))
        .await
        .unwrap();
    drop(execution);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(!provider.started.load(Ordering::Acquire));
    stop.cancel();
    task.await.unwrap();
}

#[tokio::test]
async fn rpc_input_rejection_is_the_original_harness_failure() {
    let provider = FakeProvider::new(FakeMode::RejectInput);
    let harness: Arc<dyn AgentHarness> =
        Arc::new(Harness::new(TEST_AGENT_ID.into(), provider.clone()));
    let (client, _, _, _, stop, task) = server(harness).await;
    let identity = AgentTurnRef::new("session-1", "turn-reject-input");
    let run = client
        .start(run_params(identity.clone(), "message-1"))
        .await
        .unwrap();
    run.admission.permit.consume().await.unwrap();
    provider.wait_started().await;
    let input = client
        .submit_input(AgentInputParams {
            agent_id: TEST_AGENT_ID.into(),
            identity: identity.clone(),
            input: user_message(&identity, "message-2"),
        })
        .await
        .unwrap();
    let error = input.permit.consume().await.unwrap_err();
    assert!(error.is_code(ErrorCode::InvalidRequest));
    assert!(matches!(
        run.settlement.recv().await.unwrap(),
        AgentRunSettlement::Finished { result: Err(_), .. }
    ));
    stop.cancel();
    task.await.unwrap();
}
