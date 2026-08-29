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
    let event_store = Arc::new(RecordingEventStore::default());
    let event_service = event_service_from_store(event_store.clone());
    let config_store: Arc<dyn ConfigStore> =
        Arc::new(InMemoryLlmConfigStore::new(config.llm.clone()));
    let session_service: Arc<dyn SessionService> = Arc::new(MockSessionService::default());
    let memory = talos_service::MemoryService::new(
        config.clone(),
        Arc::new(talos_store::memory::MemoryStore::in_memory().await.unwrap()),
    )
    .unwrap();
    let project_store = Arc::new(talos_store::project::MockProjectStore::new());
    let files = test_file_service(project_store.clone(), config.storage.files.clone()).await;
    let app = TalosService::new_with_session_store(
        Arc::new(MockGateway),
        Arc::new(SessionServiceRuntimeAdapter::new(session_service.clone())),
        Arc::new(SessionStoreAdapter::new(session_service)),
        Arc::new(talos_store::session::FileSessionArtifactStore::default()),
        project_store,
        Arc::new(EmptyProfileStore),
        files,
        event_service.clone(),
        event_store,
        config.clone(),
        memory,
        config_store,
        vec![test_model_info()],
        auth_service.clone(),
        user_service,
    );
    let mut app = app;
    app.plugins = plugin_manager.clone();
    app.skills = talos_service::SkillsService::new(
        config.agent.kernel.skills.clone(),
        config.agent.kernel.plugins.clone(),
    )
    .with_plugin_manager(plugin_manager);
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
async fn test_config_handler_rejects_regular_user_updates() {
    let state = test_state(false).await;
    let error = put_config_handler(
        State(state),
        Some(axum::extract::Extension(test_auth())),
        Json(talos_types::ConfigValue::WebSearch(Box::default())),
    )
    .await
    .expect_err("regular users should not update global config");

    assert_eq!(error.into_response().status(), StatusCode::FORBIDDEN);
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
    assert_eq!(data.session_count, 0);
    assert_eq!(data.message_count, 0);
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
    assert!(data.sessions.is_empty());
}

#[tokio::test]
async fn test_get_session_handler_returns_not_found_for_unknown_session() {
    let state = test_state(false).await;

    let err = get_session_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Path("unknown-session".to_string()),
    )
    .await
    .expect_err("unknown session should return error");

    assert_eq!(err.into_response().status(), StatusCode::NOT_FOUND);
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
    let event_store = Arc::new(RecordingEventStore::default());
    event_store
        .upsert_active_turn(ActiveTurnUpdate {
            session_id: "mock-session".to_string(),
            turn_id: "turn-running".to_string(),
            message_id: Some("msg-running".to_string()),
            status: ActiveTurnStatus::Running,
            last_event_id: None,
            current_action_id: None,
            updated_at: 1,
        })
        .await
        .expect("active turn should be inserted");
    let state = test_state_with_event_store(false, event_store).await;

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
                    date: "2026-04-04".to_string(),
                }),
            },
        )
        .await
        .expect("app-server memory read should use shared service");
    assert!(app_server_memory["file"]["content"]
        .as_str()
        .unwrap()
        .contains("shared service fact"));

    let app_server_workspace = adapter
        .dispatch(
            &actor,
            ClientRequest {
                method: ClientMethod::WorkspaceDocRead,
                params: ClientRequestParams::WorkspaceDocRead(WorkspaceDocReadRequest {
                    kind: WorkspaceDocKind::Soul,
                }),
            },
        )
        .await
        .expect("app-server workspace read should use shared service");
    assert_eq!(
        app_server_workspace["doc"]["exists"],
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
    assert_eq!(models["models"][0]["id"], "qwen-plus");

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
    assert!(config["config"]["provider"].is_string());

    let invalid_workspace_kind = ClientRequest::decode(
        "workspace.doc.read",
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
    let state = test_state_with_config_and_plugin_manager(config, plugin_manager, false).await;

    let added = add_plugin_source_handler(
        State(state.clone()),
        axum::extract::Extension(test_auth()),
        Json(PluginSourceAddRequest {
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
        app_server_search["plugins"][0]["metadata"]["name"],
        "contract-skill"
    );
    assert_eq!(
        app_server_search["warnings"].as_array().map(Vec::len),
        Some(0)
    );

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
