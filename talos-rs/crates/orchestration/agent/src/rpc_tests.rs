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
    harness: Arc<Harness<FakeProvider>>,
) -> (
    RemoteAgentClient,
    AgentHarnessRpcAdapter,
    Vec<u8>,
    String,
    CancellationToken,
    tokio::task::JoinHandle<()>,
) {
    server_with_binder(harness, None).await
}

async fn server_with_binder(
    harness: Arc<Harness<FakeProvider>>,
    binder: Option<crate::AgentInvocationResourceBinder>,
) -> (
    RemoteAgentClient,
    AgentHarnessRpcAdapter,
    Vec<u8>,
    String,
    CancellationToken,
    tokio::task::JoinHandle<()>,
) {
    let home = tempfile::tempdir().unwrap();
    let journal = harness.inner.execution_store.clone();
    let harness: Arc<dyn AgentHarness> =
        Arc::new(crate::LocalExecutionHarness::new(harness, journal));
    let certificate = rcgen::generate_simple_self_signed(vec!["127.0.0.1".to_string()]).unwrap();
    let pem = certificate.cert.pem().into_bytes();
    let identity = Identity::from_pem(&pem, certificate.signing_key.serialize_pem());
    let socket = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = socket.local_addr().unwrap();
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
    let adapter = match binder {
        Some(binder) => adapter.with_resource_binder(binder),
        None => adapter,
    };
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
            .serve_with_incoming_shutdown(
                tokio_stream::wrappers::TcpListenerStream::new(socket),
                server_stop.cancelled(),
            )
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
    let state = client
        .read_session_execution_state(session_params())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(state.history_cursor, history_cursor);
    history_cursor
}

#[tokio::test]
async fn rpc_delivers_terminal_settlement_after_large_progress_history() {
    let (_journal_directory, journal) = crate::harness::test_support::execution_store().await;
    let provider = FakeProvider::new(FakeMode::LargeHistory);
    let harness = Arc::new(Harness::new(
        TEST_AGENT_ID.into(),
        provider,
        journal.clone(),
    ));
    let (client, _, _, _, stop, server_task) = server(harness).await;
    tokio::time::timeout(Duration::from_secs(20), async {
        let mut run = client
            .start(run_params(
                AgentTurnRef::new("session-1", "turn-1"),
                "message-1",
            ))
            .await
            .unwrap();
        run.admission.permit.consume().await.unwrap();
        let mut completed = 0;
        while let Some(event) = run.events.recv().await {
            if matches!(
                event.payload,
                talos_agent_api::AgentEventPayload::MessageCompleted(_)
            ) {
                completed += 1;
            }
        }
        assert_eq!(completed, 12);
        assert!(matches!(
            run.settlement.recv().await.unwrap(),
            AgentRunSettlement::Finished { result: Ok(()), .. }
        ));
    })
    .await
    .unwrap();
    stop.cancel();
    server_task.await.unwrap();
}

#[tokio::test]
async fn direct_and_tls_rpc_use_the_same_paused_harness_execution() {
    let (_journal_directory, journal) = crate::harness::test_support::execution_store().await;
    let provider = FakeProvider::new(FakeMode::Release);
    let harness = Arc::new(Harness::new(
        TEST_AGENT_ID.into(),
        provider.clone(),
        journal.clone(),
    ));
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
        .start(run_params(identity.clone(), "message-1"))
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
    let title = client
        .generate_session_title(agent_protocol::AgentSessionTitleParams {
            session: session_params(),
            identity,
            mode: agent_protocol::AgentSessionTitleMode::Generate,
        })
        .await
        .unwrap()
        .unwrap();
    assert!(title.model_usage.is_none());
    assert!(
        matches!(title.outcome, agent_protocol::AgentSessionTitleOutcome::Failed { error } if error.code == talos_error::ErrorCode::ServiceUnavailable)
    );
    stop.cancel();
    task.await.unwrap();
}

#[tokio::test]
async fn rpc_input_commit_and_new_client_interrupt_keep_cleanup_conservative() {
    let (_journal_directory, journal) = crate::harness::test_support::execution_store().await;
    let provider = FakeProvider::new(FakeMode::Input);
    let harness = Arc::new(Harness::new(
        TEST_AGENT_ID.into(),
        provider.clone(),
        journal.clone(),
    ));
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
            command: None,
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
async fn tls_rpc_management_and_interrupt_preserve_harness_outcomes() {
    let (_journal_directory, journal) = crate::harness::test_support::execution_store().await;
    let provider = FakeProvider::new(FakeMode::Release);
    let harness = Arc::new(Harness::new(
        TEST_AGENT_ID.into(),
        provider.clone(),
        journal.clone(),
    ));
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
        .is_code(ErrorCode::EventCursorExpired));
    assert!(client
        .close_session(session_params(), Default::default())
        .await
        .unwrap_err()
        .is_code(ErrorCode::EventCursorExpired));
    assert!(!provider.clear_called.load(Ordering::Acquire));
    assert!(!provider.close_called.load(Ordering::Acquire));
    client
        .clear_session(session_params(), history_cursor.clone())
        .await
        .unwrap();
    assert!(provider.clear_called.load(Ordering::Acquire));
    let cleared = client
        .read_session_execution_state(session_params())
        .await
        .unwrap()
        .unwrap()
        .history_cursor;
    client
        .close_session(session_params(), cleared)
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
    let (_journal_directory, journal) = crate::harness::test_support::execution_store().await;
    let provider = FakeProvider::new(FakeMode::Release);
    let harness = Arc::new(Harness::new(
        TEST_AGENT_ID.into(),
        provider.clone(),
        journal.clone(),
    ));
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
    let (_journal_directory, journal) = crate::harness::test_support::execution_store().await;
    let provider = FakeProvider::new(FakeMode::Release);
    let harness = Arc::new(Harness::new(
        TEST_AGENT_ID.into(),
        provider.clone(),
        journal.clone(),
    ));
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
    let (_journal_directory, journal) = crate::harness::test_support::execution_store().await;
    let provider = FakeProvider::new(FakeMode::ProgressThenRelease);
    let harness = Arc::new(Harness::new(
        TEST_AGENT_ID.into(),
        provider.clone(),
        journal.clone(),
    ));
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
    let (_journal_directory, journal) = crate::harness::test_support::execution_store().await;
    let provider = FakeProvider::new(FakeMode::Release);
    let harness = Arc::new(Harness::new(
        TEST_AGENT_ID.into(),
        provider.clone(),
        journal.clone(),
    ));
    let (client, _, pem, endpoint, stop, task) = server(harness).await;
    let untrusted = rcgen::generate_simple_self_signed(vec!["127.0.0.1".into()])
        .unwrap()
        .cert
        .pem()
        .into_bytes();
    let auth = NodeAuth {
        node_id: "node-1".into(),
        token: "lease-secret".into(),
    };
    for result in [
        RemoteAgentClient::connect(
            &endpoint,
            agent_client::AgentTlsTrust::Certificate(untrusted.clone()),
            TEST_AGENT_ID.into(),
            auth.clone(),
        )
        .await
        .map(|_| ()),
        agent_client::AgentEnvironmentClient::connect(
            &endpoint,
            agent_client::AgentTlsTrust::Certificate(untrusted),
            auth.clone(),
        )
        .await
        .map(|_| ()),
    ] {
        assert!(result.unwrap_err().is_code(ErrorCode::ServiceUnavailable));
    }
    let wrong = RemoteAgentClient::connect(
        &endpoint,
        agent_client::AgentTlsTrust::Certificate(pem.clone()),
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
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let state = client
                .read_session_execution_state(session_params())
                .await
                .unwrap()
                .unwrap();
            if state.coordinator_id.is_none() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(!provider.started.load(Ordering::Acquire));

    let base = client
        .start(run_params(AgentTurnRef::new("session-1", "base"), "base"))
        .await
        .unwrap();
    base.admission.permit.consume().await.unwrap();
    provider.wait_started().await;
    provider.release.notify_one();
    let history_cursor =
        commit_received_settlement(&client, base.settlement.recv().await.unwrap()).await;

    let shared = client
        .read_session_execution_state(session_params())
        .await
        .unwrap()
        .unwrap();
    provider.started.store(false, Ordering::Release);
    let other = client
        .clone()
        .with_coordinator_id(uuid::Uuid::new_v4().to_string())
        .unwrap();
    let mut attempted = run_params(
        AgentTurnRef::new("session-1", "abandoned"),
        "must not execute",
    );
    attempted.context.history_cursor = Some(history_cursor.clone());
    drop(other.start(attempted).await.unwrap());
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if client
                .read_session_execution_state(session_params())
                .await
                .unwrap()
                .unwrap()
                == shared
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(!provider.started.load(Ordering::Acquire));
    drop(other);

    let mut retry = run_params(
        AgentTurnRef::new("session-1", "retry"),
        "original coordinator continues",
    );
    retry.context.history_cursor = Some(history_cursor);
    let run = client.start(retry).await.unwrap();
    run.admission.permit.consume().await.unwrap();
    provider.wait_started().await;
    provider.release.notify_one();
    assert!(matches!(
        run.settlement.recv().await.unwrap(),
        AgentRunSettlement::Finished { .. }
    ));
    stop.cancel();
    task.await.unwrap();
    for result in [
        RemoteAgentClient::connect(
            &endpoint,
            agent_client::AgentTlsTrust::Certificate(pem.clone()),
            TEST_AGENT_ID.into(),
            auth.clone(),
        )
        .await
        .map(|_| ()),
        agent_client::AgentEnvironmentClient::connect(
            &endpoint,
            agent_client::AgentTlsTrust::Certificate(pem),
            auth,
        )
        .await
        .map(|_| ()),
    ] {
        assert!(result.unwrap_err().is_code(ErrorCode::ServiceUnavailable));
    }
}

#[tokio::test]
async fn rpc_input_admission_survives_provider_delivery_rejection() {
    let (_journal_directory, journal) = crate::harness::test_support::execution_store().await;
    let provider = FakeProvider::new(FakeMode::RejectInput);
    let harness = Arc::new(Harness::new(
        TEST_AGENT_ID.into(),
        provider.clone(),
        journal.clone(),
    ));
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
            command: None,
            agent_id: TEST_AGENT_ID.into(),
            identity: identity.clone(),
            input: user_message(&identity, "message-2"),
        })
        .await
        .unwrap();
    input.permit.consume().await.unwrap();
    assert!(matches!(
        run.settlement.recv().await.unwrap(),
        AgentRunSettlement::Finished { result: Err(_), .. }
    ));
    stop.cancel();
    task.await.unwrap();
}

async fn library_client(
    mode: FakeMode,
) -> (
    tempfile::TempDir,
    FakeProvider,
    AgentHarnessRpcAdapter,
    Arc<dyn AgentHarness>,
) {
    library_client_with_binder(mode, None).await
}

async fn library_client_with_binder(
    mode: FakeMode,
    binder: Option<crate::AgentInvocationResourceBinder>,
) -> (
    tempfile::TempDir,
    FakeProvider,
    AgentHarnessRpcAdapter,
    Arc<dyn AgentHarness>,
) {
    let home = tempfile::tempdir().unwrap();
    let provider = FakeProvider::new(mode);
    let journal = Arc::new(
        talos_store::execution::WorkerExecutionStore::open(&home.path().join("worker.sqlite"))
            .await
            .unwrap(),
    );
    let durable: Arc<dyn AgentHarness> = Arc::new(crate::LocalExecutionHarness::new(
        Arc::new(Harness::new(
            TEST_AGENT_ID.into(),
            provider.clone(),
            journal.clone(),
        )),
        journal,
    ));
    let adapter = AgentHarnessRpcAdapter::new(
        "embedded".into(),
        Arc::new(std::sync::RwLock::new(
            Config::default()
                .with_runtime_paths(talos_config::TalosPaths::from_home(home.path().to_owned()))
                .with_workspace_definition(WorkspaceDefinition::new("workspace-1", "/tmp")),
        )),
        vec![(TEST_AGENT_ID.into(), durable)],
    );
    let adapter = match binder {
        Some(binder) => adapter.with_resource_binder(binder),
        None => adapter,
    };
    let auth = NodeAuth {
        node_id: "embedded".into(),
        token: "private-process-credential".into(),
    };
    adapter.install_local_auth(&auth).unwrap();
    let client = adapter
        .local_harness(TEST_AGENT_ID.into(), auth, uuid::Uuid::new_v4().to_string())
        .unwrap();
    (home, provider, adapter, client)
}

#[tokio::test]
async fn library_admission_preserves_the_gate_and_large_history_without_rpc() {
    let (_home, provider, adapter, client) = library_client(FakeMode::LargeHistory).await;
    let identity = AgentTurnRef::new("session-1", "large-local");
    let params = run_params(identity.clone(), "large-input");
    let mut execution = client.start(params).await.unwrap();
    assert!(!provider.started.load(Ordering::Acquire));
    assert!(
        matches!(client.start(run_params(identity, "overlap")).await, Err(error) if error.is_code(ErrorCode::ActiveTurnRunning))
    );
    execution.admission.permit.consume().await.unwrap();
    let mut completed = 0;
    while let Some(event) = execution.events.recv().await {
        if matches!(
            event.payload,
            talos_agent_api::AgentEventPayload::MessageCompleted(_)
        ) {
            completed += 1;
        }
    }
    assert_eq!(completed, 12);
    let AgentRunSettlement::Finished {
        event: _,
        result: Ok(()),
        ..
    } = execution.settlement.recv().await.unwrap()
    else {
        panic!("expected completion");
    };

    client.shutdown(Duration::from_secs(2)).await.unwrap();
    assert!(provider.shutdown_called.load(Ordering::Acquire));
    adapter.clear_local_auth();
    assert!(client
        .read_session_execution_state(session_params())
        .await
        .is_err());
}

#[tokio::test]
async fn library_live_input_stays_paused_until_its_permit_is_consumed() {
    let (_home, provider, _adapter, client) = library_client(FakeMode::Input).await;
    let identity = AgentTurnRef::new("session-1", "local-input");
    let mut execution = client
        .start(run_params(identity.clone(), "first"))
        .await
        .unwrap();
    execution.admission.permit.consume().await.unwrap();
    provider.wait_started().await;
    let input = client
        .submit_input(AgentInputParams {
            command: None,
            agent_id: TEST_AGENT_ID.into(),
            identity: identity.clone(),
            input: user_message(&identity, "second"),
        })
        .await
        .unwrap();
    assert!(!provider.input_received.load(Ordering::Acquire));
    input.permit.consume().await.unwrap();
    assert!(provider.input_received.load(Ordering::Acquire));
    while execution.events.recv().await.is_some() {}
    assert!(matches!(
        execution.settlement.recv().await.unwrap(),
        AgentRunSettlement::Finished { result: Ok(()), .. }
    ));
    client.shutdown(Duration::from_secs(2)).await.unwrap();
}

#[tokio::test]
async fn rpc_interrupt_coordinator_gate_remains_a_preflight_status() {
    use agent_pb::agent_harness_rpc_server::AgentHarnessRpc;
    let (_home, provider, adapter, client) = library_client(FakeMode::Release).await;
    let identity = AgentTurnRef::new("session-1", "turn-preflight");
    let mut run = client
        .start(run_params(identity.clone(), "message-1"))
        .await
        .unwrap();
    run.admission.permit.consume().await.unwrap();
    provider.wait_started().await;
    let result = AgentHarnessRpc::interrupt(
        &adapter,
        tonic::Request::new(agent_pb::InterruptRequest {
            auth: Some(agent_pb::NodeAuth {
                node_id: "embedded".into(),
                token: "private-process-credential".into(),
            }),
            params: Some(agent_protocol::harness_pb::encode_interrupt_params(
                AgentInterruptParams {
                    agent_id: TEST_AGENT_ID.into(),
                    identity,
                },
            )),
            caller: Some(agent_pb::CoordinatorCallContext {
                coordinator_id: uuid::Uuid::new_v4().to_string(),
                ..Default::default()
            }),
        }),
    )
    .await;
    adapter.clear_local_auth();
    while run.events.recv().await.is_some() {}
    let status = result.expect_err("coordinator gate must reject before returning an RPC payload");
    assert!(agent_protocol::error_pb::decode_status(status).is_code(ErrorCode::Conflict));
}

fn library_input_request(
    identity: AgentTurnRef,
    coordinator_id: String,
) -> agent_pb::SubmitInputRequest {
    agent_pb::SubmitInputRequest {
        auth: Some(agent_pb::NodeAuth {
            node_id: "embedded".into(),
            token: "private-process-credential".into(),
        }),
        params: Some(agent_pb::SubmitInputParams {
            body: Some(agent_pb::submit_input_params::Body::Input(Box::new(
                agent_protocol::harness_pb::encode_input_params(AgentInputParams {
                    command: None,
                    agent_id: TEST_AGENT_ID.into(),
                    input: user_message(&identity, "input-1"),
                    identity,
                })
                .unwrap(),
            ))),
        }),
        caller: Some(agent_pb::CoordinatorCallContext {
            coordinator_id,
            ..Default::default()
        }),
    }
}

#[tokio::test]
async fn rpc_input_gate_rejects_before_opening_a_connect_response() {
    let (_home, _provider, adapter, _client) = library_client(FakeMode::Release).await;
    let request = library_input_request(
        AgentTurnRef::new("session-1", "absent"),
        uuid::Uuid::new_v4().to_string(),
    );
    let result = adapter
        .dispatch_connect(Box::pin(tokio_stream::iter(vec![Ok(
            agent_pb::worker_rpc_call::Request::SubmitInput(Box::new(request)),
        )])))
        .await;
    let status = match result {
        Err(status) => status,
        Ok(_) => panic!("absent-Turn gate must reject before opening a response stream"),
    };
    assert!(
        agent_protocol::error_pb::decode_status(status).is_code(ErrorCode::RunCleanupUnconfirmed)
    );
}

#[tokio::test]
async fn rpc_input_harness_rejection_remains_a_failure_payload() {
    use tokio_stream::StreamExt;
    let (_home, provider, adapter, client) = library_client(FakeMode::Release).await;
    let identity = AgentTurnRef::new("session-1", "turn-input-failure");
    let mut run = client
        .start(run_params(identity.clone(), "message-1"))
        .await
        .unwrap();
    run.admission.permit.consume().await.unwrap();
    provider.wait_started().await;
    let peer = client
        .read_session_execution_state(session_params())
        .await
        .unwrap()
        .unwrap()
        .coordinator_id
        .unwrap();
    let mut request = library_input_request(identity.clone(), peer);
    let Some(agent_pb::submit_input_params::Body::Input(params)) = request
        .params
        .as_mut()
        .and_then(|params| params.body.as_mut())
    else {
        unreachable!()
    };
    // Valid wire message and active gate, but rejected by Harness input semantics.
    params.input.as_mut().unwrap().turn_id = "another-turn".into();
    let mut response = adapter
        .dispatch_connect(Box::pin(tokio_stream::iter(vec![Ok(
            agent_pb::worker_rpc_call::Request::SubmitInput(Box::new(request)),
        )])))
        .await
        .expect("Harness rejection is not a preflight Status");
    let agent_pb::worker_rpc_result::Response::Input(response) =
        response.next().await.unwrap().unwrap()
    else {
        panic!("expected an input response");
    };
    let Some(agent_pb::input_response::Result::Failure(failure)) = response.result else {
        panic!("Harness input validation must remain a failure payload");
    };
    assert!(agent_protocol::error_pb::decode_failure(failure)
        .unwrap()
        .is_code(ErrorCode::InvalidRequest));
    client
        .interrupt(AgentInterruptParams {
            agent_id: TEST_AGENT_ID.into(),
            identity,
        })
        .await
        .unwrap();
    while run.events.recv().await.is_some() {}
    assert!(matches!(
        run.settlement.recv().await.unwrap(),
        AgentRunSettlement::Finished { .. }
    ));
}

#[tokio::test]
async fn rpc_shutdown_keeps_lookup_status_distinct_from_operation_failure() {
    use agent_pb::agent_harness_rpc_server::AgentHarnessRpc;
    let (_home, provider, adapter, client) = library_client(FakeMode::Release).await;
    let request = |agent_id: &str, timeout_ms| {
        tonic::Request::new(agent_pb::ShutdownRequest {
            auth: Some(agent_pb::NodeAuth {
                node_id: "embedded".into(),
                token: "private-process-credential".into(),
            }),
            params: Some(agent_pb::ShutdownParams {
                agent_id: agent_id.into(),
                timeout_ms,
            }),
        })
    };
    let missing = AgentHarnessRpc::shutdown(&adapter, request("missing-agent", 50))
        .await
        .expect_err("Harness lookup is RPC preflight");
    assert!(agent_protocol::error_pb::decode_status(missing).is_code(ErrorCode::ResourceNotFound));

    let mut run = client
        .start(run_params(
            AgentTurnRef::new("session-1", "turn-shutdown"),
            "message-1",
        ))
        .await
        .unwrap();
    run.admission.permit.consume().await.unwrap();
    provider.wait_started().await;
    // A real active operation and zero deadline deterministically fail inside Harness.
    let response = AgentHarnessRpc::shutdown(&adapter, request(TEST_AGENT_ID, 0))
        .await
        .expect("actual shutdown errors remain payload-framed")
        .into_inner();
    let Some(agent_pb::empty_response::Result::Failure(failure)) = response.result else {
        panic!("expected shutdown Timeout payload")
    };
    assert!(agent_protocol::error_pb::decode_failure(failure)
        .unwrap()
        .is_code(ErrorCode::Timeout));

    // Failed shutdown must retain this instance so a later drain can finish it.
    let response = AgentHarnessRpc::shutdown(&adapter, request(TEST_AGENT_ID, 2_000))
        .await
        .unwrap()
        .into_inner();
    assert!(matches!(
        response.result,
        Some(agent_pb::empty_response::Result::Completed(_))
    ));
    assert!(provider.shutdown_called.load(Ordering::Acquire));
    while run.events.recv().await.is_some() {}
    assert!(matches!(
        run.settlement.recv().await.unwrap(),
        AgentRunSettlement::Finished { .. }
    ));
    let removed = AgentHarnessRpc::shutdown(&adapter, request(TEST_AGENT_ID, 50))
        .await
        .expect_err("successful shutdown removes the exact Harness instance");
    assert!(agent_protocol::error_pb::decode_status(removed).is_code(ErrorCode::ResourceNotFound));
}

#[path = "rpc_replay_tests.rs"]
mod replay;
