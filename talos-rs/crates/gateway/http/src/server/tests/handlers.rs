use super::*;

fn canonical_memory(body: &str) -> String {
    let timezone = talos_config::parse_timezone("Asia/Shanghai").expect("timezone");
    let context = talos_memory::DreamMemoryDocumentContext::new(
        talos_memory::MemoryAgentId::parse("default").expect("workspace id"),
        talos_memory::MemoryUserId::parse("static-token").expect("user id"),
        talos_memory::DreamMemoryDate::parse("2026-04-04").expect("date"),
        talos_memory::MemoryTimezone::new(timezone),
    );
    let mut document = talos_memory::DreamMemoryDocument::new(context, 1_775_268_000)
        .expect("Dream Memory document");
    document
        .create_entry(
            talos_memory::DreamMemoryEntry::builder()
                .memory_id(
                    talos_memory::MemoryId::parse("mem_0123456789abcdef0123456789abcdef")
                        .expect("memory id"),
                )
                .title("Example")
                .body(body)
                .occurred_at(1_775_268_000)
                .kind(talos_types::MemoryKind::Fact)
                .importance(talos_types::MemoryImportance::Normal)
                .provenance(talos_memory::DreamMemoryProvenance::UserAuthored)
                .build()
                .expect("memory entry"),
        )
        .expect("create memory entry");
    document.render().expect("render Dream Memory")
}

async fn test_state_with_config_and_plugin_manager(
    config: talos_types::TalosConfig,
    plugin_manager: talos_service::PluginManagerService,
    auth_disabled: bool,
) -> Arc<HttpApiState> {
    let (auth_service, user_service) = test_auth_services();
    let agent_execution_store = test_agent_execution_store().await;
    let event_service = event_service_from_store(agent_execution_store.clone());
    let config_store: Arc<dyn ConfigStore> =
        Arc::new(InMemoryLlmConfigStore::new(config.llm.clone()));
    let (session_store, nodes) = test_session_store().await;
    let memory = talos_service::MemoryService::new(
        config.clone(),
        Arc::new(talos_store::memory::MemoryStore::in_memory().await.unwrap()),
    )
    .unwrap();
    let project_store = Arc::new(talos_store::project::MockProjectStore::new());
    let files = test_file_service(project_store.clone(), config.storage.files.clone()).await;
    let reload_hook = test_config_reload_hook(config.clone());
    let config_service = talos_service::ConfigService::new(
        config_store,
        plugin_manager.execution_gate(),
        reload_hook,
    );
    let app = TalosService::new(
        Arc::new(TestAgentCoordinator(
            config.clone(),
            memory,
            plugin_manager.clone(),
            config_service.clone(),
        )),
        nodes,
        session_store,
        agent_execution_store,
        project_store,
        Arc::new(EmptyProfileStore),
        files,
        event_service.clone(),
        config.clone(),
        auth_service.clone(),
        user_service,
    );
    Arc::new(HttpApiState::from_service(
        Arc::new(app),
        event_service,
        config,
        auth_service,
        auth_disabled,
        Arc::new(ServerLifecycle::new()),
    ))
}

#[tokio::test]
async fn test_health() {
    let state = test_state(false).await;
    let app = Router::new()
        .route("/health", get(health_handler))
        .with_state(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("body should be readable");
    let payload: Value = serde_json::from_slice(&body).expect("payload should be valid json");
    assert_eq!(payload["auth_enabled"], true);
    assert_eq!(payload["cloud_auth_url"], "http://127.0.0.1:3000");
}

#[tokio::test]
async fn test_health_reports_auth_disabled() {
    let state = test_state(true).await;
    let app = Router::new()
        .route("/health", get(health_handler))
        .with_state(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("body should be readable");
    let payload: Value = serde_json::from_slice(&body).expect("payload should be valid json");
    assert_eq!(payload["auth_enabled"], false);
}

#[tokio::test]
async fn test_models() {
    let state = test_state(false).await;
    let app = Router::new()
        .route("/v1/models", get(list_models_handler))
        .with_state(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("body should be readable");
    let payload: Value = serde_json::from_slice(&body).expect("payload should be valid json");
    assert_eq!(payload["status"], "success");
    assert_eq!(payload["data"]["models"][0]["id"], "qwen-plus");
    assert_eq!(payload["data"]["models"][0]["provider"], "bailian");
    assert_eq!(payload["data"]["models"][0]["context_window"], 32_768);
    assert_eq!(payload["data"]["models"][0]["reasoning"], true);
    assert_eq!(
        payload["data"]["defaultModelSelection"],
        serde_json::json!({
            "modelRef": {"provider": "bailian", "id": "qwen-plus"},
            "reasoningEffort": "medium"
        })
    );
}

#[tokio::test]
async fn test_get_and_put_web_search_config_handlers() {
    let state = test_state(false).await;
    let web_search = talos_config::WebSearchToolConfig {
        provider: talos_config::WebSearchProviderConfig::Brave,
        brave: talos_config::WebSearchProviderRuntimeConfig {
            base_url: Some("https://brave.example/search".to_string()),
            timeout_ms: 12_000,
            ..Default::default()
        },
        ..Default::default()
    };

    let update_response = put_config_handler(
        State(state.clone()),
        Some(axum::extract::Extension(admin_auth())),
        axum::extract::Query(talos_types::WorkspaceInfoRequest::default()),
        Json(talos_types::ConfigValue::WebSearch(Box::new(
            web_search.clone(),
        ))),
    )
    .await
    .expect("web search config update should succeed");
    let update_data = expect_success(update_response.0);
    assert_eq!(
        update_data,
        talos_types::ConfigResponse::WebSearch {
            config: Box::new(web_search.clone())
        }
    );

    let get_response = get_config_handler(
        State(state),
        Some(axum::extract::Extension(admin_auth())),
        axum::extract::Query(talos_types::WorkspaceInfoRequest::default()),
        Path("webSearch".to_string()),
    )
    .await
    .expect("web search config read should succeed");
    let get_data = expect_success(get_response.0);

    assert_eq!(
        get_data,
        talos_types::ConfigResponse::WebSearch {
            config: Box::new(web_search)
        }
    );
}

#[tokio::test]
async fn test_config_handler_allows_authorized_regular_worker_user() {
    let config = test_config();
    let state = test_state_with_config(config, false).await;
    let response = put_config_handler(
        State(state),
        Some(axum::extract::Extension(test_auth())),
        axum::extract::Query(talos_types::WorkspaceInfoRequest::default()),
        Json(talos_types::ConfigValue::WebSearch(Box::default())),
    )
    .await
    .expect("authorized Worker users can configure their environment");
    assert_eq!(
        expect_success(response.0),
        talos_types::ConfigResponse::WebSearch {
            config: Box::default()
        }
    );
}

fn admin_auth() -> RequestIdentity {
    RequestIdentity {
        role: talos_auth::UserRole::Admin,
        ..test_auth()
    }
}

#[tokio::test]
async fn test_profile_handler_returns_shared_profile_response() {
    let state = test_state(false).await;

    let response = read_profile_handler(State(state), axum::extract::Extension(test_auth()))
        .await
        .expect("profile read should succeed");

    let data = expect_success(response.0);
    assert_eq!(data.session_count, 1);
    assert_eq!(data.message_count, 1);
    assert_eq!(data.project_count, 0);
    assert!(!data.usage_available);
}

#[tokio::test]
async fn test_list_sessions_handler_returns_data_array() {
    let state = test_state(false).await;

    let response = list_sessions_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Query(SessionListRequest::default()),
    )
    .await
    .expect("list sessions should succeed");

    let data = expect_success(response.0);
    assert_eq!(data.sessions.len(), 1);
    assert_eq!(data.sessions[0].id, "mock-session");
}

#[tokio::test]
async fn test_get_session_handler_returns_not_found_for_unknown_session() {
    let state = test_state(false).await;
    let expected = state
        .gateway
        .get_session(
            &crate::server::access::shared_actor(&test_auth()),
            talos_types::SessionGetRequest {
                session_id: "unknown-session".to_string(),
            },
        )
        .await
        .expect_err("unknown session should fail in the service")
        .to_api_error();

    let err = get_session_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Path("unknown-session".to_string()),
    )
    .await
    .expect_err("unknown session should return error");

    let response = err.into_response();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = axum::body::to_bytes(response.into_body(), 16_384)
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap(),
        serde_json::to_value(talos_types::ApiResponse::<()>::error(expected)).unwrap()
    );
}

#[tokio::test]
async fn test_get_session_handler_returns_session_usage() {
    let state = test_state(false).await;

    let response = get_session_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Path("mock-session".to_string()),
    )
    .await
    .expect("known session should be returned");

    let data = expect_success(response.0);
    assert_eq!(data.session.usage.tokens.total_tokens, 120);
    assert_eq!(
        data.session
            .usage
            .context
            .as_ref()
            .map(|context| context.used_tokens),
        Some(80)
    );
}

#[tokio::test]
async fn test_patch_session_handler_returns_success_ack() {
    let state = test_state(false).await;

    let response = patch_session_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Path("mock-session".to_string()),
        Json(
            serde_json::from_value(serde_json::json!({
                "conf": {
                    "modelSelection": {
                        "modelRef": {
                            "provider": "bailian",
                            "id": "qwen-plus"
                        },
                        "reasoningEffort": "medium"
                    }
                }
            }))
            .expect("patch request should deserialize"),
        ),
    )
    .await
    .expect("patch session should succeed");

    let data = expect_success(response.0);
    assert_eq!(data.session.id, "mock-session");
}

#[tokio::test]
async fn test_close_session_handler_returns_success() {
    let state = test_state(false).await;

    let response = close_session_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Path("mock-session".to_string()),
    )
    .await
    .expect("close session should succeed");

    let data = expect_success(response.0);
    assert!(data.success);
}

#[tokio::test]
async fn test_delete_session_handler_returns_success() {
    let state = test_state(false).await;

    let response = delete_session_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Path("mock-session".to_string()),
    )
    .await
    .expect("delete session should succeed");

    let data = expect_success(response.0);
    assert!(data.success);
}

#[tokio::test]
async fn test_compact_session_handler_returns_session_payload() {
    let state = test_state(false).await;

    let response = compact_session_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Path("mock-session".to_string()),
    )
    .await
    .expect("compact session should succeed");

    let data = expect_success(response.0);
    assert_eq!(data.session.id, "mock-session");
    assert_eq!(data.session.message_count, 1);
}

#[tokio::test]
async fn test_compact_session_handler_maps_active_turn_to_conflict() {
    let execution_store = test_agent_execution_store().await;
    seed_running_agent_execution(execution_store.as_ref(), "mock-session", "turn-running").await;
    let state = test_state_with_event_store_and_agent_execution_store(
        false,
        execution_store.clone(),
        execution_store,
    )
    .await;

    let error = compact_session_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Path("mock-session".to_string()),
    )
    .await
    .expect_err("active session compaction should be rejected");

    assert_eq!(error.into_response().status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn test_clear_session_handler_returns_reset_session_payload() {
    let state = test_state(false).await;

    let response = clear_session_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Path("mock-session".to_string()),
    )
    .await
    .expect("clear session should succeed");

    let data = expect_success(response.0);
    assert_eq!(data.session.id, "mock-session");
    assert_eq!(data.session.message_count, 0);
    assert_eq!(data.session.usage.tokens.total_tokens, 0);
}

#[tokio::test]
async fn test_get_workspace_doc_handler_returns_exists_false_for_missing_file() {
    let (config, workspace_dir) = isolated_test_config();
    let state = test_state_with_config(config, false).await;

    let response = get_workspace_doc_handler(
        Query(Default::default()),
        State(state),
        axum::extract::Extension(test_auth()),
        Path("soul".to_string()),
    )
    .await
    .expect("get workspace doc should succeed");

    let data = expect_success(response.0);
    assert_eq!(data.doc.kind, talos_types::WorkspaceDocKind::Soul);
    assert!(!data.doc.exists);
    assert_eq!(data.doc.content, "");
    let _ = std::fs::remove_dir_all(workspace_dir);
}

#[tokio::test]
async fn test_put_and_get_memory_file_handlers_roundtrip() {
    let (config, workspace_dir) = isolated_test_config();
    let state = test_state_with_config(config, false).await;

    let put_response = put_memory_file_handler(
        State(state.clone()),
        axum::extract::Extension(test_auth()),
        Path("2026-04-04".to_string()),
        Json(talos_types::MemoryFileWriteRequest {
            node_id: None,
            date: "2026-04-04".to_string(),
            content: canonical_memory("stable fact"),
            expected_version: None,
        }),
    )
    .await
    .expect("put memory file should succeed");

    let put_data = expect_success(put_response.0);
    assert_eq!(put_data.file.date, "2026-04-04");

    let get_response = get_memory_file_handler(
        Query(Default::default()),
        State(state.clone()),
        axum::extract::Extension(test_auth()),
        Path("2026-04-04".to_string()),
    )
    .await
    .expect("get memory file should succeed");

    let get_data = expect_success(get_response.0);
    assert!(get_data.file.content.contains("stable fact"));

    let list_response = list_memory_files_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Query(serde_json::from_value(serde_json::json!({ "limit": 10 })).unwrap()),
    )
    .await
    .expect("list memory files should succeed");

    let list_data = expect_success(list_response.0);
    assert_eq!(list_data.data[0].date, "2026-04-04");
    let _ = std::fs::remove_dir_all(workspace_dir);
}

#[tokio::test]
async fn test_http_and_app_server_adapters_share_service_contract() {
    use talos_app_protocol::errors::INVALID_PARAMS;
    use talos_app_protocol::requests::{ClientMethod, ClientRequest, ClientRequestParams};
    use talos_app_server::{AppServerApplication, TalosApplicationAdapter};
    use talos_types::{
        ConfigGetRequest, MemoryFileReadRequest, ModelListRequest, WorkspaceDocKind,
        WorkspaceDocReadRequest,
    };

    let (config, workspace_dir) = isolated_test_config();
    let state = test_state_with_config(config, false).await;
    let adapter = TalosApplicationAdapter::new(state.app.clone(), 32);
    let actor = talos_service::Actor::regular("static-token");
    let admin_actor =
        talos_service::Actor::regular("admin-token").with_role(talos_auth::UserRole::Admin);

    let put_response = put_memory_file_handler(
        State(state.clone()),
        axum::extract::Extension(test_auth()),
        Path("2026-04-04".to_string()),
        Json(talos_types::MemoryFileWriteRequest {
            node_id: None,
            date: "2026-04-04".to_string(),
            content: canonical_memory("shared service fact"),
            expected_version: None,
        }),
    )
    .await
    .expect("HTTP memory write should succeed");
    let put_data = expect_success(put_response.0);
    assert_eq!(put_data.file.date, "2026-04-04");

    let app_server_memory = adapter
        .dispatch(
            &actor,
            ClientRequest {
                method: ClientMethod::MemoryFileRead,
                params: ClientRequestParams::MemoryFileRead(MemoryFileReadRequest {
                    node_id: None,
                    date: "2026-04-04".to_string(),
                }),
            },
        )
        .await
        .expect("app-server memory read should use shared service");
    assert!(app_server_memory.body["file"]["content"]
        .as_str()
        .unwrap()
        .contains("shared service fact"));

    let app_server_workspace = adapter
        .dispatch(
            &actor,
            ClientRequest {
                method: ClientMethod::WorkspaceDocRead,
                params: ClientRequestParams::WorkspaceDocRead(WorkspaceDocReadRequest {
                    node_id: None,
                    kind: WorkspaceDocKind::Soul,
                }),
            },
        )
        .await
        .expect("app-server workspace read should use shared service");
    assert_eq!(
        app_server_workspace.body["doc"]["exists"],
        serde_json::Value::Bool(false)
    );

    let models = adapter
        .dispatch(
            &actor,
            ClientRequest {
                method: ClientMethod::ModelList,
                params: ClientRequestParams::ModelList(ModelListRequest::default()),
            },
        )
        .await
        .expect("app-server model list should use shared config service");
    assert_eq!(models.body["models"][0]["id"], "qwen-plus");

    let config = adapter
        .dispatch(
            &admin_actor,
            ClientRequest {
                method: ClientMethod::ConfigGet,
                params: ClientRequestParams::ConfigGet(ConfigGetRequest::default()),
            },
        )
        .await
        .expect("app-server config get should use shared config service");
    assert!(config.body["config"]["provider"].is_string());

    let invalid_workspace_kind = ClientRequest::decode(
        "workspace/doc/read",
        Some(serde_json::json!({ "kind": "unknown" })),
    )
    .expect_err("invalid workspace kind should fail at protocol decode");
    assert_eq!(invalid_workspace_kind.code, INVALID_PARAMS);

    let _ = std::fs::remove_dir_all(workspace_dir);
}

#[tokio::test]
async fn test_plugin_http_and_app_server_adapters_share_service_contract() {
    use talos_app_protocol::requests::{ClientMethod, ClientRequest, ClientRequestParams};
    use talos_app_server::{AppServerApplication, TalosApplicationAdapter};
    use talos_config::PluginsRuntimeConfig;
    use talos_plugin::{PluginManager, PluginStateStore};
    use talos_service::PluginManagerService;
    use talos_types::{
        MarketplaceSourceInput, PluginKind, PluginSearchRefreshPolicy, PluginSearchRequest,
        PluginSearchStatusFilter, PluginSourceAddRequest,
    };

    let temp = tempfile::tempdir().expect("temp dir should exist");
    let repo = temp.path().join("repo");
    let skill_dir = repo.join("skills").join("contract-skill");
    std::fs::create_dir_all(&skill_dir).expect("skill dir should be created");
    std::fs::write(
        skill_dir.join("SKILL.md"),
        r#"---
name: contract-skill
description: Verify shared plugin service contract.
---

Use this skill for protocol contract verification.
"#,
    )
    .expect("skill should be written");

    let (config, workspace_dir) = isolated_test_config();
    let plugin_manager = PluginManagerService::with_manager(PluginManager::with_state_store(
        PluginsRuntimeConfig {
            sources: Vec::new(),
            roots: Vec::new(),
        },
        PluginStateStore::new(temp.path().join("plugin-state")),
    ));
    let mcp = Arc::new(talos_mcp::TalosMcpRuntime::new());
    let owner = mcp.clone();
    let plugin_manager = plugin_manager.with_mcp(
        mcp.clone(),
        Arc::new(move |servers| {
            let prepared = owner.prepare(servers.into_iter().map(Into::into).collect())?;
            Ok(Box::new(move || prepared.publish()))
        }),
        talos_service::McpCredentialStore::new(temp.path().join("credentials")),
    );
    let state = test_state_with_config_and_plugin_manager(config, plugin_manager, false).await;

    let added = add_plugin_source_handler(
        State(state.clone()),
        axum::extract::Extension(test_auth()),
        Json(PluginSourceAddRequest {
            node_id: None,
            source: MarketplaceSourceInput::LocalFolder {
                path: repo,
                sparse_paths: vec!["skills".to_string()],
            },
            refresh: true,
        }),
    )
    .await
    .expect("HTTP plugin source add should succeed");
    let added = expect_success(added.0);
    assert_eq!(added.plugins[0].metadata.name, "contract-skill");
    assert!(added.warnings.is_empty());

    let adapter = TalosApplicationAdapter::new(state.app.clone(), 32);
    let actor = talos_service::Actor::regular("static-token");
    let app_server_search = adapter
        .dispatch(
            &actor,
            ClientRequest {
                method: ClientMethod::PluginSearch,
                params: ClientRequestParams::PluginSearch(PluginSearchRequest {
                    node_id: None,
                    query: Some("contract".to_string()),
                    source_id: Some(added.source.id),
                    kind: Some(PluginKind::Skill),
                    status: Some(PluginSearchStatusFilter::Any),
                    refresh: PluginSearchRefreshPolicy::CacheOnly,
                    limit: Some(10),
                }),
            },
        )
        .await
        .expect("app-server plugin search should use shared service");

    assert_eq!(
        app_server_search.body["plugins"][0]["metadata"]["name"],
        "contract-skill"
    );
    assert_eq!(
        app_server_search.body["warnings"].as_array().map(Vec::len),
        Some(0)
    );

    // A real JSON HTTP request imports a write-only token; App Server reads and clears the same state.
    let router = Router::new()
        .route("/v1/mcp/servers/import", post(import_mcp_server_handler))
        .layer(axum::Extension(test_auth()))
        .with_state(state.clone());
    let response = router.oneshot(Request::builder().method("POST").uri("/v1/mcp/servers/import").header("content-type", "application/json").body(Body::from(serde_json::json!({
        "json": "{\"mcpServers\":{\"contract_mcp\":{\"url\":\"https://example.com/mcp\",\"headers\":{\"Authorization\":\"Bearer contract-private-token\"}}}}",
        "enable": false, "conflictPolicy": "reject"
    }).to_string())).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    assert!(!String::from_utf8_lossy(&body).contains("contract-private-token"));
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let owner = talos_types::McpDefinitionRef::Plugin {
        install_id: serde_json::from_value(body["data"]["install"]["id"].clone()).unwrap(),
    };
    let authentication = adapter
        .dispatch(
            &actor,
            ClientRequest {
                method: ClientMethod::McpAuthenticationRead,
                params: ClientRequestParams::McpAuthenticationRead(
                    talos_types::McpAuthenticationReadRequest {
                        node_id: None,
                        server: owner.clone(),
                    },
                ),
            },
        )
        .await
        .unwrap();
    assert_eq!(authentication.body["state"]["type"], "credentialsStored");
    let cleared = adapter
        .dispatch(
            &actor,
            ClientRequest {
                method: ClientMethod::McpCredentialsUpdate,
                params: ClientRequestParams::McpCredentialsUpdate(
                    talos_types::McpCredentialsUpdateRequest {
                        node_id: None,
                        server: owner,
                        change: talos_types::McpCredentialsChange::Clear,
                    },
                ),
            },
        )
        .await
        .unwrap();
    assert_eq!(cleared.body["state"]["type"], "missing");

    // Both adapters page the same live connection owner and expose safe errors.
    mcp.prepare(
        ["alpha", "beta"]
            .into_iter()
            .map(|name| talos_types::McpServerRuntimeDescriptor {
                content_revision: None,
                source: talos_types::McpServerSource::Plugin {
                    plugin_id: talos_types::PluginId("fixture".into()),
                    install_id: talos_types::PluginInstallId("fixture".into()),
                },
                server: talos_types::McpServerDefinition {
                    credentials: None,
                    name: name.into(),
                    display_name: None,
                    description: None,
                    transport: talos_types::McpTransportDefinition::Stdio {
                        command: "/missing/private-mcp-command".into(),
                        args: vec![],
                        cwd: None,
                        env: [("API_KEY".into(), "private-mcp-secret".into())].into(),
                    },
                },
            })
            .map(Into::into)
            .collect(),
    )
    .unwrap()
    .publish();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while !mcp
            .list_servers(Default::default())
            .unwrap()
            .data
            .iter()
            .all(|server| matches!(server.status, talos_types::McpServerStatus::Failed { .. }))
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let first = expect_success(
        list_mcp_servers_handler(
            State(state.clone()),
            axum::extract::Extension(test_auth()),
            Query(talos_types::McpServerListRequest {
                node_id: None,
                cursor: None,
                limit: Some(1),
            }),
        )
        .await
        .unwrap()
        .0,
    );
    assert_eq!(first.data[0].name, "alpha");
    assert_eq!(first.next_cursor.as_deref(), Some("alpha"));
    let second = adapter
        .dispatch(
            &actor,
            ClientRequest {
                method: ClientMethod::McpServerList,
                params: ClientRequestParams::McpServerList(talos_types::McpServerListRequest {
                    node_id: None,
                    cursor: first.next_cursor,
                    limit: Some(1),
                }),
            },
        )
        .await
        .unwrap()
        .body;
    assert_eq!(second["data"][0]["name"], "beta");
    assert_eq!(second["data"][0]["status"]["status"], "failed");
    assert_eq!(second["data"][0]["toolCount"], 0);
    assert!(second["nextCursor"].is_null());
    assert!(!second.to_string().contains("private-mcp"));
    assert!(list_mcp_servers_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Query(talos_types::McpServerListRequest {
            node_id: None,
            cursor: None,
            limit: Some(0)
        })
    )
    .await
    .is_err());
    mcp.shutdown().await.unwrap();
    let _ = std::fs::remove_dir_all(workspace_dir);
}

#[tokio::test]
async fn test_search_memory_files_handler_returns_matching_data() {
    let (config, workspace_dir) = isolated_test_config();
    let memory_dir = workspace_dir.join("static-token").join("memory");
    std::fs::create_dir_all(&memory_dir).expect("memory dir should exist");
    std::fs::write(
        memory_dir.join("2026-04-04.md"),
        canonical_memory("stable fact"),
    )
    .expect("memory file should write");
    let state = test_state_with_config(config, false).await;

    let response = search_memory_files_handler(
        State(state.clone()),
        axum::extract::Extension(test_auth()),
        Query(
            serde_json::from_value(serde_json::json!({
                "query": "stable",
                "limit": 5
            }))
            .unwrap(),
        ),
    )
    .await
    .expect("search memory should succeed");

    let search_data = expect_success(response.0);
    assert_eq!(search_data.data[0].source.date, "2026-04-04");
    let memory_id = search_data.data[0].memory_id.clone();
    let detail = get_memory_handler(
        Query(Default::default()),
        State(state),
        axum::extract::Extension(test_auth()),
        Path(memory_id.clone()),
    )
    .await
    .expect("get memory should succeed");
    let detail = expect_success(detail.0);
    assert_eq!(detail.memory.memory_id, memory_id);
    assert!(detail.memory.body.contains("stable fact"));
    let _ = std::fs::remove_dir_all(workspace_dir);
}

#[tokio::test]
async fn test_http_server_listen_addr_uses_configured_host() {
    let state = test_state(false).await;
    let server = HttpServer::new(
        state.app.clone(),
        state.event_service.clone(),
        state.auth_service.clone(),
        test_config(),
        "127.0.0.1",
        8080,
    );

    assert_eq!(server.listen_addr(), "127.0.0.1:8080");
}

#[tokio::test]
async fn workspace_info_http_and_rpc_share_actor_scoped_storage_locations() {
    use talos_app_protocol::requests::{ClientMethod, ClientRequest, ClientRequestParams};
    use talos_app_server::{AppServerApplication, TalosApplicationAdapter};
    let (config, workspace_dir) = isolated_test_config();
    let expected = talos_types::WorkspaceInfoResponse {
        workspace_id: config.default_workspace_id().into(),
        root_prefix: config
            .workspace_root_prefix(config.default_workspace_id())
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        user_workspace_dir: config
            .resolve_workspace(config.default_workspace_id(), "static-token")
            .unwrap()
            .root
            .to_string_lossy()
            .into_owned(),
    };
    let state = test_state_with_config(config, false).await;
    let http = workspace_info_handler(
        axum::extract::Query(talos_types::WorkspaceInfoRequest::default()),
        State(state.clone()),
        axum::extract::Extension(test_auth()),
    )
    .await
    .unwrap();
    assert_eq!(expect_success(http.0), expected);
    let adapter = TalosApplicationAdapter::new(state.app.clone(), 32);
    let rpc = adapter
        .dispatch(
            &talos_service::Actor::regular("static-token"),
            ClientRequest {
                method: ClientMethod::WorkspaceInfo,
                params: ClientRequestParams::WorkspaceInfo(talos_types::WorkspaceInfoRequest {
                    node_id: None,
                }),
            },
        )
        .await
        .unwrap();
    assert_eq!(rpc.body, serde_json::to_value(expected).unwrap());
    assert!(!workspace_dir.join("static-token/profile.json").exists());
    std::fs::remove_dir_all(workspace_dir).unwrap();
}
