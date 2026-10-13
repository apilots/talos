//! Real TLS direct/Connect calls and embedded calls must use the same filesystem owner.
use super::*;
use agent_protocol::{WorkerEnvironment, WorkerEnvironmentContext};
use talos_types::{FsDirectoryCreateRequest, FsDirectoryReadRequest};

#[tokio::test]
async fn embedded_direct_and_outbound_environment_share_paths_under_computer_authorization() {
    let fixture = Fixture::new(None).await;
    let response = fixture
        .control
        .register(fixture.registration(&["talos"]))
        .await
        .unwrap()
        .into_inner();
    let Some(pb::register_response::Result::Registered(registration)) = response.result else {
        panic!("register failed")
    };
    let registration = node_pb::decode_registration(registration).unwrap();
    fixture.heartbeat(registration.lease_token.clone()).await;
    let auth = NodeAuth {
        node_id: fixture.credential.node_id.clone(),
        token: registration.lease_token.clone(),
    };
    let home = tempfile::tempdir().unwrap();
    let mut config = talos_config::Config::default()
        .with_runtime_paths(talos_config::TalosPaths::from_home(home.path().into()));
    config.workspaces = talos_config::WorkspaceConfig {
        default_workspace_id: "pc".into(),
        definitions: vec![talos_config::WorkspaceDefinition::new("pc", "worker-files")],
    };
    config.llm.anthropic.models = vec![talos_config::ProviderModelConfig {
        id: config.llm.model.clone(),
        name: "Worker model".into(),
        context_window: 32_768,
        reasoning: false,
        reasoning_effort: Default::default(),
        speed: Default::default(),
        input: Vec::new(),
        cost: Default::default(),
    }];
    let skill_root = home.path().join("skills");
    std::fs::create_dir_all(skill_root.join("review")).unwrap();
    std::fs::write(
        skill_root.join("review/SKILL.md"),
        "---\nname: review\ndescription: Review node changes.\n---\n\nUse node evidence.",
    )
    .unwrap();
    config.agent.kernel.plugins.roots = vec![talos_config::PluginRootConfig {
        kind: talos_config::PluginRootKind::Skill,
        path: skill_root.display().to_string(),
        read_only: true,
    }];
    let memory = talos_service::MemoryService::new(
        config.clone(),
        Arc::new(talos_store::memory::MemoryStore::in_memory().await.unwrap()),
    )
    .unwrap();
    let plugins = talos_service::PluginManagerService::with_manager(
        talos_plugin::PluginManager::with_state_store(
            config.agent.kernel.plugins.clone(),
            talos_plugin::PluginStateStore::new(home.path().join("plugins")),
        ),
    );
    let environment: Arc<dyn WorkerEnvironment> = Arc::new(
        talos_service::WorkerEnvironmentService::new(
            Arc::new(std::sync::RwLock::new(config.clone())),
            Some(home.path().into()),
        )
        .with_memory_service(memory)
        .with_plugin_manager(plugins),
    );
    let adapter = agent_client::AgentHarnessRpcAdapter::new(
        auth.node_id.clone(),
        Arc::new(std::sync::RwLock::new(config)),
        Vec::new(),
    )
    .with_environment(environment.clone());
    adapter
        .install_registration(
            registration,
            tokio::time::Instant::now() + std::time::Duration::from_secs(60),
        )
        .unwrap();
    let ingress = Ingress::new(fixture.control.clone()).await;
    let node = agent_client::AgentNodeClient::connect(&ingress.endpoint, ingress.trust.clone())
        .await
        .unwrap();
    let stop = CancellationToken::new();
    let worker_auth = auth.clone();
    let worker_adapter = adapter.clone();
    let worker_stop = stop.clone();
    let worker = tokio::spawn(async move {
        node.run_execution_connection(worker_auth, worker_adapter, worker_stop)
            .await
    });
    // Publication, rather than an arbitrary sleep, is the synchronization point.
    let outbound = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            if let Ok(environment) = fixture
                .publisher
                .coordinator
                .worker_environment(&auth.node_id, "user-1")
                .await
            {
                break environment;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let certificate = rcgen::generate_simple_self_signed(vec!["127.0.0.1".into()]).unwrap();
    let pem = certificate.cert.pem();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let incoming = futures_util::stream::unfold(listener, |listener| async {
        Some((listener.accept().await.map(|(stream, _)| stream), listener))
    });
    let direct_stop = CancellationToken::new();
    let stopped = direct_stop.clone();
    let server = tokio::spawn(async move {
        Server::builder()
            .tls_config(ServerTlsConfig::new().identity(Identity::from_pem(
                certificate.cert.pem(),
                certificate.signing_key.serialize_pem(),
            )))
            .unwrap()
            .add_service(
                pb::worker_environment_rpc_server::WorkerEnvironmentRpcServer::new(adapter),
            )
            .serve_with_incoming_shutdown(incoming, stopped.cancelled_owned())
            .await
            .unwrap();
    });
    let direct = agent_client::AgentEnvironmentClient::connect(
        &format!("https://{address}"),
        agent_client::AgentTlsTrust::Certificate(pem.into_bytes()),
        auth.clone(),
    )
    .await
    .unwrap();
    let context = WorkerEnvironmentContext {
        user_id: "user-1".into(),
        username: None,
        workspace_id: None,
    };
    let expected = environment.workspace_info(context.clone()).await.unwrap();
    assert_eq!(expected.workspace_id, "pc");
    assert_eq!(
        direct.workspace_info(context.clone()).await.unwrap(),
        expected
    );
    assert_eq!(
        outbound.workspace_info(context.clone()).await.unwrap(),
        expected
    );
    // A remote write is immediately visible through embedded/direct reads; a stale
    // version cannot overwrite it. MEMORY.md uses the same node-local writer.
    for kind in [
        talos_types::WorkspaceDocKind::Soul,
        talos_types::WorkspaceDocKind::Memory,
    ] {
        let content = if kind == talos_types::WorkspaceDocKind::Memory {
            talos_memory::UserWorkspaceMemoryDocument::template(
                talos_memory::UserWorkspaceMemoryProducer::new(
                    talos_memory::MemoryAgentId::parse("pc").unwrap(),
                    1_791_720_000,
                ),
            )
            .unwrap()
            .render()
            .unwrap()
        } else {
            "node-local document".into()
        };
        let written = outbound
            .write_workspace_doc(
                context.clone(),
                talos_types::WorkspaceDocWriteRequest {
                    node_id: None,
                    kind,
                    content,
                    expected_version: None,
                },
            )
            .await
            .unwrap();
        let read = talos_types::WorkspaceDocReadRequest {
            node_id: None,
            kind,
        };
        assert_eq!(
            direct
                .read_workspace_doc(context.clone(), read.clone())
                .await
                .unwrap()
                .doc,
            written.doc
        );
        assert_eq!(
            environment
                .read_workspace_doc(context.clone(), read)
                .await
                .unwrap()
                .doc,
            written.doc
        );
        let stale = direct
            .write_workspace_doc(
                context.clone(),
                talos_types::WorkspaceDocWriteRequest {
                    node_id: None,
                    kind,
                    content: "stale overwrite".into(),
                    expected_version: Some("stale".into()),
                },
            )
            .await
            .unwrap_err();
        assert!(stale.is_code(ErrorCode::Conflict));
    }
    assert_eq!(
        outbound
            .list_workspace_docs(context.clone(), Default::default())
            .await
            .unwrap(),
        environment
            .list_workspace_docs(context.clone(), Default::default())
            .await
            .unwrap()
    );
    verify_memory_and_skills(&context, [&*environment, &direct, &*outbound]).await;
    let catalog = environment.list_models(context.clone()).await.unwrap();
    assert_eq!(direct.list_models(context.clone()).await.unwrap(), catalog);
    assert_eq!(
        outbound.list_models(context.clone()).await.unwrap(),
        catalog
    );
    let created = outbound
        .create_directory(
            context.clone(),
            FsDirectoryCreateRequest {
                node_id: None,
                parent: home.path().into(),
                name: "project".into(),
            },
        )
        .await
        .unwrap();
    let params = FsDirectoryReadRequest {
        node_id: None,
        path: Some(home.path().into()),
    };
    assert_eq!(
        direct
            .read_directory(context.clone(), params.clone())
            .await
            .unwrap(),
        environment
            .read_directory(context.clone(), params.clone())
            .await
            .unwrap()
    );
    assert_eq!(
        outbound
            .read_directory(context.clone(), params)
            .await
            .unwrap(),
        direct
            .read_directory(
                context.clone(),
                FsDirectoryReadRequest {
                    node_id: None,
                    path: Some(home.path().into())
                }
            )
            .await
            .unwrap()
    );
    assert_eq!(
        outbound
            .validate_project_directory(context.clone(), created.path.clone())
            .await
            .unwrap(),
        created.path
    );
    assert_eq!(
        direct
            .project_status(context.clone(), created.path.clone())
            .await
            .unwrap(),
        outbound
            .project_status(context.clone(), created.path)
            .await
            .unwrap()
    );
    // The credential authorizes the whole personal Computer. A claimed Session
    // keeps its original filesystem namespace without treating it as an account.
    let original_namespace = WorkerEnvironmentContext {
        user_id: "anonymous".into(),
        ..context.clone()
    };
    assert_eq!(
        outbound
            .workspace_info(original_namespace.clone())
            .await
            .unwrap(),
        direct
            .workspace_info(original_namespace.clone())
            .await
            .unwrap()
    );
    assert!(outbound
        .workspace_info(original_namespace)
        .await
        .unwrap()
        .user_workspace_dir
        .contains("anonymous"));
    assert!(fixture
        .publisher
        .coordinator
        .worker_environment(&auth.node_id, "user-2")
        .await
        .err()
        .unwrap()
        .is_code(ErrorCode::PermissionDenied));
    let saved_missing = WorkerEnvironmentContext {
        workspace_id: Some("removed".into()),
        ..context
    };
    assert!(outbound
        .workspace_info(saved_missing)
        .await
        .unwrap_err()
        .is_code(ErrorCode::InvalidRequest));
    direct_stop.cancel();
    server.await.unwrap();
    stop.cancel();
    worker.await.unwrap().unwrap();
    ingress.close().await;
    assert!(fixture
        .publisher
        .coordinator
        .worker_environment(&auth.node_id, "user-1")
        .await
        .is_err());
}

/// A versioned write updates the existing index; every transport observes that
/// same file/search result and the configured Skills root, including warnings.
async fn verify_memory_and_skills(
    context: &WorkerEnvironmentContext,
    ports: [&dyn WorkerEnvironment; 3],
) {
    use talos_types::*;
    let timezone = talos_config::parse_timezone("UTC").unwrap();
    let document_context = talos_memory::DreamMemoryDocumentContext::new(
        talos_memory::MemoryAgentId::parse("pc").unwrap(),
        talos_memory::MemoryUserId::parse("user-1").unwrap(),
        talos_memory::DreamMemoryDate::parse("2026-10-11").unwrap(),
        talos_memory::MemoryTimezone::new(timezone),
    );
    let mut document =
        talos_memory::DreamMemoryDocument::new(document_context, 1_791_720_000).unwrap();
    document
        .create_entry(
            talos_memory::DreamMemoryEntry::builder()
                .memory_id(
                    talos_memory::MemoryId::parse("mem_0123456789abcdef0123456789abcdef").unwrap(),
                )
                .title("Node fact")
                .body("Use node evidence")
                .occurred_at(1_791_720_000)
                .kind(MemoryKind::Fact)
                .importance(MemoryImportance::Normal)
                .provenance(talos_memory::DreamMemoryProvenance::UserAuthored)
                .build()
                .unwrap(),
        )
        .unwrap();
    let written = ports[2]
        .write_memory_file(
            context.clone(),
            MemoryFileWriteRequest {
                node_id: None,
                date: "2026-10-11".into(),
                content: document.render().unwrap(),
                expected_version: None,
            },
        )
        .await
        .unwrap();
    let mut expected_search = None;
    for port in ports {
        assert_eq!(
            port.read_memory_file(
                context.clone(),
                MemoryFileReadRequest {
                    node_id: None,
                    date: "2026-10-11".into()
                }
            )
            .await
            .unwrap(),
            MemoryFileReadResponse {
                file: written.file.clone()
            }
        );
        assert_eq!(
            port.list_memory_files(context.clone(), Default::default())
                .await
                .unwrap()
                .data[0]
                .version_token,
            written.file.version_token
        );
        let search = port
            .search_memory(
                context.clone(),
                MemorySearchRequest {
                    node_id: None,
                    query: "evidence".into(),
                    limit: None,
                    kinds: Some(vec![MemoryKind::Fact]),
                    since_at: None,
                    until_at: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(search.data.len(), 1);
        let got = port
            .get_memory(
                context.clone(),
                MemoryGetRequest {
                    node_id: None,
                    memory_id: search.data[0].memory_id.clone(),
                },
            )
            .await
            .unwrap();
        assert_eq!(got.memory.body, "Use node evidence");
        if let Some(expected) = &expected_search {
            assert_eq!(&search, expected);
        } else {
            expected_search = Some(search);
        }
        assert!(port
            .write_memory_file(
                context.clone(),
                MemoryFileWriteRequest {
                    node_id: None,
                    date: "2026-10-11".into(),
                    content: written.file.content.clone() + "\n",
                    expected_version: Some("stale".into())
                }
            )
            .await
            .unwrap_err()
            .is_code(ErrorCode::Conflict));
        let list = port
            .list_skills(context.clone(), Default::default())
            .await
            .unwrap();
        assert_eq!(list.data.len(), 1);
        assert_eq!(
            port.refresh_skills(context.clone(), Default::default())
                .await
                .unwrap(),
            list
        );
        let read = port
            .read_skill(
                context.clone(),
                SkillsReadRequest {
                    node_id: None,
                    selector: SkillSelector::by_name("review"),
                },
            )
            .await
            .unwrap();
        assert!(read.document.contents.contains("Use node evidence."));
        let suggestions = port
            .suggest_skills(
                context.clone(),
                SkillsSuggestRequest {
                    node_id: None,
                    query: "rev".into(),
                    limit: Some(1),
                },
            )
            .await
            .unwrap();
        assert_eq!(suggestions.data[0].name, "review");
        let resolved = port
            .resolve_skills(
                context.clone(),
                SkillsResolveRequest {
                    node_id: None,
                    text: "Use $review".into(),
                    selected: Vec::new(),
                },
            )
            .await
            .unwrap();
        assert_eq!(resolved.data, list.data);
        assert!(resolved.warnings.is_empty());
    }
}
