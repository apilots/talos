//! Real registered Worker/Automation callback acceptance; requires the controlled upstream.
use super::*;
use std::{path::PathBuf, time::Duration};
use talos_config::{
    AgentNodeRunnerConfig, AgentNodeTransport, Config, TalosPaths, WorkerRpcTlsConfig,
};
use talos_types::{
    AgentUserContent, AgentUserInput, AutomationTurnOrigin, GoalStatus, SessionActiveTurnRequest,
    SessionAutomationReadRequest, SessionHistoryRequest, SkillScope, SkillsListRequest,
    StartTurnRequest, UserPermissionMode,
};

fn fixture_directory() -> PathBuf {
    std::env::var_os("TALOS_AUTOMATION_FIXTURE_DIR")
        .map(PathBuf::from)
        .expect("Run through 09_app_server/automation_e2e with its controlled model")
}

fn config(home: &Path) -> Config {
    let source = std::env::var_os("TALOS_AUTOMATION_FIXTURE_CONFIG")
        .expect("controlled fixture config is required");
    let config: Config = serde_json::from_slice(&std::fs::read(source).unwrap()).unwrap();
    let paths = TalosPaths::from_home(home.into());
    paths.ensure_base_dirs().unwrap();
    let mut config = config.with_runtime_paths(paths);
    config.apply_derived_paths();
    config.gateway.auth = true;
    config.agent_node.control_plane = None;
    config.agent_node.runner = None;
    persist(&config);
    config
}

fn persist(config: &Config) {
    let mut persisted = serde_json::to_value(config).unwrap();
    persisted["logging"].as_object_mut().unwrap().remove("file");
    persisted["storage"]
        .as_object_mut()
        .unwrap()
        .remove("connection_string");
    std::fs::write(
        config.source_path(),
        serde_json::to_vec(&persisted).unwrap(),
    )
    .unwrap();
}

fn address() -> std::net::SocketAddr {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
}

#[tokio::test]
#[ignore = "requires controlled automation model; run 09_app_server/automation_e2e"]
async fn automation_worker_business_direct() {
    run(AgentNodeTransport::Direct).await;
}

#[tokio::test]
#[ignore = "requires controlled automation model; run 09_app_server/automation_e2e"]
async fn automation_worker_business_outbound() {
    run(AgentNodeTransport::Outbound).await;
}

async fn run(transport: AgentNodeTransport) {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let home = tempfile::tempdir().unwrap();
    let certificate = rcgen::generate_simple_self_signed(vec!["127.0.0.1".into()]).unwrap();
    let cert = home.path().join("rpc.crt");
    let key = home.path().join("rpc.key");
    std::fs::write(&cert, certificate.cert.pem()).unwrap();
    std::fs::write(&key, certificate.signing_key.serialize_pem()).unwrap();
    let ingress = address();
    let control_config = talos_config::AgentNodeControlConfig {
        bind_address: ingress.to_string(),
        tls_certificate_file: cert.clone(),
        tls_private_key_file: key.clone(),
        trusted_node_ca_file: Some(cert.clone()),
    };
    let mut owner_config = config(&home.path().join("owner"));
    owner_config.agent_node.control_plane = Some(control_config.clone());
    owner_config.cloud.agent_base_url = format!("https://{ingress}");
    owner_config.cloud.agent_trusted_ca_file = Some(cert.clone());
    persist(&owner_config);
    let runtime = crate::build_application_runtime_with_migration_dir(
        owner_config,
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../conf/migrations/sqlite"),
    )
    .await
    .unwrap();
    let server = NodeRpcServer::new(
        &control_config,
        runtime.node_service.clone(),
        Arc::new(TestAuth),
        runtime.coordinator.clone(),
        NodeResourceServices {
            session_data: NodeSessionDataRpc::new(
                runtime.node_service.clone(),
                runtime.session_store.clone(),
                runtime.session_history_reader.clone(),
                runtime.attachment_reader.clone(),
                runtime.memory_progress_store.clone(),
            ),
            agent_control: runtime.agent_control.clone(),
            automation_control: runtime.automation_control.clone(),
            tool_images: NodeToolImageRpc::new(
                runtime.node_service.clone(),
                runtime.tool_image_store.clone(),
                1024 * 1024,
            ),
        },
    )
    .unwrap();
    let stop = CancellationToken::new();
    let serving = tokio::spawn(server.run(stop.clone()));
    let endpoint = format!("https://{ingress}");
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if AgentNodeClient::connect(
                &endpoint,
                agent_client::AgentTlsTrust::Certificate(certificate.cert.pem().into_bytes()),
            )
            .await
            .is_ok()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let actor = Actor::regular("user-1").with_access_token("user-1");
    let worker_address = address();
    let node = runtime
        .node_service
        .create_node(
            &actor,
            agent_protocol::CreateNodeRequest {
                display_name: format!("Automation {transport:?}"),
                endpoint: (transport == AgentNodeTransport::Direct)
                    .then(|| format!("https://{worker_address}")),
                owner_user_id: None,
                credential_expires_at: chrono::Utc::now().timestamp() + 3600,
            },
        )
        .await
        .unwrap();
    let node_id = node.node.node_id;
    let mut worker_config = config(&home.path().join("worker"));
    let credential = worker_config
        .talos_paths()
        .conf_dir
        .join("worker.credential");
    talos_fs::write_bytes_with(
        &credential,
        node.credential.token.as_bytes(),
        talos_fs::WriteOptions::default().with_permissions(talos_fs::FilePermissions::unix(0o600)),
    )
    .unwrap();
    worker_config.agent_node.runner = Some(AgentNodeRunnerConfig {
        transport: Some(transport),
        node_id: node_id.clone(),
        control_plane_endpoint: endpoint,
        credential_file: credential,
        trusted_ca_file: Some(cert.clone()),
    });
    if transport == AgentNodeTransport::Direct {
        worker_config.agent_node.worker_rpc.bind_address = worker_address.to_string();
        worker_config.agent_node.worker_rpc.tls = Some(WorkerRpcTlsConfig {
            certificate_file: cert,
            private_key_file: key,
        });
    }
    persist(&worker_config);
    let worker = talos_worker::RegisteredWorker::start(worker_config)
        .await
        .unwrap();
    let skill = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            if let Ok(skills) = runtime
                .gateway
                .list_skills(
                    &actor,
                    SkillsListRequest {
                        node_id: Some(node_id.clone()),
                        include_disabled: false,
                    },
                )
                .await
            {
                if let Some(skill) = skills.data.into_iter().find(|skill| {
                    skill.scope == SkillScope::System
                        && skill.name == "goal"
                        && skill.enabled
                        && skill.read_only
                }) {
                    break skill;
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("real Worker environment was not published");
    let case = format!("goal_{}", uuid::Uuid::new_v4().simple());
    let session = uuid::Uuid::new_v4().to_string();
    runtime
        .gateway
        .start_turn(
            &actor,
            talos_types::talos::SessionSource::http(),
            StartTurnRequest {
                node_id: Some(node_id.clone()),
                session_id: session.clone(),
                input: AgentUserInput {
                    content: vec![
                        AgentUserContent::Text {
                            text: format!("AUTO_CASE={case} controlled automation work"),
                        },
                        AgentUserContent::SkillSelection {
                            name: skill.name,
                            path: skill.path.to_string_lossy().into(),
                        },
                    ],
                },
                model_selection: None,
                user_permission_mode: Some(UserPermissionMode::FullAccess),
                project_id: None,
                current_directory: None,
                client_request_id: Some(uuid::Uuid::new_v4().to_string()),
            },
            None,
        )
        .await
        .unwrap();
    let gate = fixture_directory().join(format!("{case}.work-1"));
    tokio::time::timeout(Duration::from_secs(30), async {
        while !gate.exists() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("registered Worker did not execute the automatic Goal Turn");
    let active = runtime
        .gateway
        .get_active_turn(
            &actor,
            SessionActiveTurnRequest {
                session_id: session.clone(),
            },
        )
        .await
        .unwrap()
        .active_turn
        .unwrap();
    let origin = active
        .active_turn
        .automation_input
        .clone()
        .expect("remote automatic Input provenance missing");
    assert!(matches!(origin.origin, AutomationTurnOrigin::Goal { .. }));
    let record = runtime
        .session_store
        .get_session_record(&session)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.execution_node_id, node_id);
    assert_eq!(record.user_id, actor.user_id);
    std::fs::write(
        fixture_directory().join(format!("{case}.release-work-1")),
        "released",
    )
    .unwrap();
    let completed = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let state = runtime
                .gateway
                .session_automation(
                    &actor,
                    SessionAutomationReadRequest {
                        session_id: session.clone(),
                    },
                )
                .await
                .unwrap();
            if state
                .goal
                .as_ref()
                .is_some_and(|goal| goal.status == GoalStatus::Complete)
            {
                let history = runtime
                    .gateway
                    .get_session_history(
                        &actor,
                        SessionHistoryRequest {
                            session_id: session.clone(),
                            cursor: None,
                            limit: Some(100),
                        },
                    )
                    .await
                    .unwrap();
                if let Some(turn) = history
                    .data
                    .into_iter()
                    .find(|turn| turn.identity == active.active_turn.identity)
                {
                    assert_eq!(turn.automation_input.as_ref(), Some(&origin));
                    break turn;
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("remote Goal Tool/Service/settlement did not complete");
    assert!(matches!(
        completed.result,
        talos_agent_api::AgentRunResult::Completed(_)
    ));
    std::fs::write(
        fixture_directory().join(format!("remote-{transport:?}-business.json")),
        serde_json::to_vec_pretty(&completed).unwrap(),
    )
    .unwrap();
    worker.shutdown().await.unwrap();
    stop.cancel();
    serving.await.unwrap().unwrap();
    runtime.shutdown_sync().await.unwrap();
    runtime.shutdown().await.unwrap();
}
