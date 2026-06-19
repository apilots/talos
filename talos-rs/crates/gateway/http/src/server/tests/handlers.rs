use super::*;

fn test_state_with_config_and_plugin_manager(
    config: talos_types::TalosConfig,
    plugin_manager: talos_application_service::PluginManagerService,
    auth_disabled: bool,
) -> Arc<HttpApiState> {
    let (auth_service, user_service) = test_auth_services();
    let event_store = Arc::new(RecordingEventStore::default());
    let event_service = event_service_from_store(event_store.clone());
    let llm_config_service: Arc<dyn LlmConfigService> = Arc::new(LlmConfigManager::new(Arc::new(
        InMemoryLlmConfigStore::new(config.llm.clone()),
    )));
    let app = TalosApplicationService::new_with_session_store(
        Arc::new(MockGateway),
        Arc::new(SessionServiceRuntimeAdapter::new(Arc::new(
            MockSessionService,
        ))),
        Arc::new(SessionStoreAdapter::new(Arc::new(MockSessionService))),
        event_service.clone(),
        event_store,
        config.clone(),
        Arc::new(MockLlmProvider),
        llm_config_service,
        vec![test_model_info()],
        auth_service.clone(),
        user_service,
    );
    let mut app = app;
    app.plugins = plugin_manager.clone();
    app.skills = talos_application_service::SkillsService::new(
        config.agent.kernel.skills.clone(),
        config.agent.kernel.plugins.clone(),
    )
    .with_plugin_manager(plugin_manager);
    Arc::new(HttpApiState::from_application_service(
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
    let state = test_state(false);
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
    let state = test_state(true);
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
    let state = test_state(false);
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
    assert_eq!(payload["data"][0]["id"], "qwen-plus");
    assert_eq!(payload["data"][0]["provider"], "bailian");
    assert_eq!(payload["data"][0]["context_window"], 32_768);
    assert_eq!(payload["data"][0]["reasoning"], true);
}

#[tokio::test]
async fn test_list_sessions_handler_returns_data_array() {
    let state = test_state(false);

    let response = list_sessions_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Query(SessionQuery::default()),
    )
    .await
    .expect("list sessions should succeed");

    assert!(response.0["data"].is_array());
}

#[tokio::test]
async fn test_get_session_handler_returns_not_found_for_unknown_session() {
    let state = test_state(false);

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
async fn test_patch_session_handler_returns_success_ack() {
    let state = test_state(false);

    let response = patch_session_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Path("mock-session".to_string()),
        Json(
            serde_json::from_value(serde_json::json!({
                "conf": {"agent.model": "qwen-plus"}
            }))
            .expect("patch request should deserialize"),
        ),
    )
    .await
    .expect("patch session should succeed");

    assert_eq!(response.0["success"], serde_json::Value::Bool(true));
}

#[tokio::test]
async fn test_close_session_handler_returns_success() {
    let state = test_state(false);

    let response = close_session_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Path("mock-session".to_string()),
    )
    .await
    .expect("close session should succeed");

    assert_eq!(response.0["success"], serde_json::Value::Bool(true));
}

#[tokio::test]
async fn test_delete_session_handler_returns_success() {
    let state = test_state(false);

    let response = delete_session_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Path("mock-session".to_string()),
    )
    .await
    .expect("delete session should succeed");

    assert_eq!(response.0["success"], serde_json::Value::Bool(true));
}

#[tokio::test]
async fn test_compact_session_handler_returns_session_payload() {
    let state = test_state(false);

    let response = compact_session_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Path("mock-session".to_string()),
    )
    .await
    .expect("compact session should succeed");

    assert_eq!(response.0["context"]["session_id"], "mock-session");
    assert_eq!(response.0["state"]["message_count"], 1);
}

#[tokio::test]
async fn test_clear_session_handler_returns_reset_session_payload() {
    let state = test_state(false);

    let response = clear_session_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Path("mock-session".to_string()),
    )
    .await
    .expect("clear session should succeed");

    assert_eq!(response.0["context"]["session_id"], "mock-session");
    assert_eq!(response.0["state"]["message_count"], 0);
    assert_eq!(response.0["state"]["usage"]["total_tokens"], 0);
}

#[tokio::test]
async fn test_get_workspace_doc_handler_returns_exists_false_for_missing_file() {
    let (config, workspace_dir) = isolated_test_config();
    let state = test_state_with_config(config, false);

    let response = get_workspace_doc_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Path("soul".to_string()),
    )
    .await
    .expect("get workspace doc should succeed");

    assert_eq!(response.0["kind"], "soul");
    assert_eq!(response.0["exists"], serde_json::Value::Bool(false));
    assert_eq!(response.0["content"], "");
    let _ = std::fs::remove_dir_all(workspace_dir);
}

#[tokio::test]
async fn test_get_workspace_info_handler_returns_effective_paths() {
    let (config, workspace_dir) = isolated_test_config();
    let state = test_state_with_config(config, false);

    let response = get_workspace_info_handler(State(state), axum::extract::Extension(test_auth()))
        .await
        .expect("get workspace info should succeed");

    assert_eq!(
        response.0["base_workspace_dir"],
        workspace_dir.to_string_lossy().to_string()
    );
    assert_eq!(response.0["user_id"], "static-token");
    assert_eq!(
        response.0["user_workspace_dir"],
        workspace_dir
            .join("users")
            .join("static-token")
            .to_string_lossy()
            .to_string()
    );
    let _ = std::fs::remove_dir_all(workspace_dir);
}

#[tokio::test]
async fn test_put_and_get_memory_file_handlers_roundtrip() {
    let (config, workspace_dir) = isolated_test_config();
    let state = test_state_with_config(config, false);

    let put_response = put_memory_file_handler(
        State(state.clone()),
        axum::extract::Extension(test_auth()),
        Path("2026-04-04".to_string()),
        Json(talos_types::DocUpdateRequest {
            content: "## [10:00] Example\nsession: session-1\n\n**Facts:**\n- stable fact\n"
                .to_string(),
            base_version: talos_types::ExpectedFileVersion::MustNotExist,
        }),
    )
    .await
    .expect("put memory file should succeed");

    assert_eq!(put_response.0["date"], "2026-04-04");

    let get_response = get_memory_file_handler(
        State(state.clone()),
        axum::extract::Extension(test_auth()),
        Path("2026-04-04".to_string()),
    )
    .await
    .expect("get memory file should succeed");

    assert!(get_response.0["content"]
        .as_str()
        .unwrap()
        .contains("stable fact"));

    let list_response = list_memory_files_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Query(serde_json::from_value(serde_json::json!({ "limit": 10 })).unwrap()),
    )
    .await
    .expect("list memory files should succeed");

    assert_eq!(list_response.0["data"][0]["date"], "2026-04-04");
    let _ = std::fs::remove_dir_all(workspace_dir);
}

#[tokio::test]
async fn test_http_and_app_server_adapters_share_application_service_contract() {
    use talos_app_protocol::errors::VALIDATION_FAILED;
    use talos_app_protocol::requests::{
        ClientMethod, ClientRequest, ClientRequestParams, ConfigReadParams, MemoryFileReadParams,
        ModelListParams, WorkspaceDocReadParams,
    };
    use talos_app_server::{AppServerApplication, TalosApplicationAdapter};

    let (config, workspace_dir) = isolated_test_config();
    let state = test_state_with_config(config, false);
    let adapter = TalosApplicationAdapter::new(state.app.clone(), 32);
    let actor = talos_application_service::Actor::regular("static-token");

    let put_response = put_memory_file_handler(
        State(state.clone()),
        axum::extract::Extension(test_auth()),
        Path("2026-04-04".to_string()),
        Json(talos_types::DocUpdateRequest {
            content:
                "## [10:00] Example\nsession: session-1\n\n**Facts:**\n- shared service fact\n"
                    .to_string(),
            base_version: talos_types::ExpectedFileVersion::MustNotExist,
        }),
    )
    .await
    .expect("HTTP memory write should succeed");
    assert_eq!(put_response.0["date"], "2026-04-04");

    let app_server_memory = adapter
        .dispatch(
            &actor,
            ClientRequest {
                method: ClientMethod::MemoryFileRead,
                params: ClientRequestParams::MemoryFileRead(MemoryFileReadParams {
                    path: "2026-04-04.md".into(),
                }),
            },
        )
        .await
        .expect("app-server memory read should use shared service");
    assert!(app_server_memory["content"]
        .as_str()
        .unwrap()
        .contains("shared service fact"));

    let app_server_workspace = adapter
        .dispatch(
            &actor,
            ClientRequest {
                method: ClientMethod::WorkspaceDocRead,
                params: ClientRequestParams::WorkspaceDocRead(WorkspaceDocReadParams {
                    kind: "soul".to_string(),
                }),
            },
        )
        .await
        .expect("app-server workspace read should use shared service");
    assert_eq!(
        app_server_workspace["exists"],
        serde_json::Value::Bool(false)
    );

    let models = adapter
        .dispatch(
            &actor,
            ClientRequest {
                method: ClientMethod::ModelList,
                params: ClientRequestParams::ModelList(ModelListParams {}),
            },
        )
        .await
        .expect("app-server model list should use shared config service");
    assert_eq!(models["data"][0]["id"], "qwen-plus");

    let config = adapter
        .dispatch(
            &actor,
            ClientRequest {
                method: ClientMethod::ConfigRead,
                params: ClientRequestParams::ConfigRead(ConfigReadParams {}),
            },
        )
        .await
        .expect("app-server config read should use shared config service");
    assert!(config["config"]["provider"].is_string());

    let invalid_workspace_kind = adapter
        .dispatch(
            &actor,
            ClientRequest {
                method: ClientMethod::WorkspaceDocRead,
                params: ClientRequestParams::WorkspaceDocRead(WorkspaceDocReadParams {
                    kind: "unknown".to_string(),
                }),
            },
        )
        .await
        .expect_err("app-server invalid workspace kind should fail");
    assert_eq!(invalid_workspace_kind.code, VALIDATION_FAILED);

    let _ = std::fs::remove_dir_all(workspace_dir);
}

#[tokio::test]
async fn test_plugin_http_and_app_server_adapters_share_application_service_contract() {
    use talos_app_protocol::requests::{ClientMethod, ClientRequest, ClientRequestParams};
    use talos_app_server::{AppServerApplication, TalosApplicationAdapter};
    use talos_application_service::PluginManagerService;
    use talos_config::PluginsRuntimeConfig;
    use talos_plugin::{PluginManager, PluginStateStore};
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
    let state = test_state_with_config_and_plugin_manager(config, plugin_manager, false);

    let added = add_plugin_source_handler(
        State(state.clone()),
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
    assert_eq!(added.0.plugins[0].name, "contract-skill");
    assert!(added.0.warnings.is_empty());

    let adapter = TalosApplicationAdapter::new(state.app.clone(), 32);
    let actor = talos_application_service::Actor::regular("static-token");
    let app_server_search = adapter
        .dispatch(
            &actor,
            ClientRequest {
                method: ClientMethod::PluginSearch,
                params: ClientRequestParams::PluginSearch(PluginSearchRequest {
                    query: Some("contract".to_string()),
                    source_id: Some(added.0.source.id),
                    kind: Some(PluginKind::Skill),
                    status: Some(PluginSearchStatusFilter::Any),
                    refresh: PluginSearchRefreshPolicy::CacheOnly,
                    limit: Some(10),
                }),
            },
        )
        .await
        .expect("app-server plugin search should use shared service");

    assert_eq!(app_server_search["plugins"][0]["name"], "contract-skill");
    assert_eq!(
        app_server_search["warnings"].as_array().map(Vec::len),
        Some(0)
    );

    let _ = std::fs::remove_dir_all(workspace_dir);
}

#[tokio::test]
async fn test_search_memory_files_handler_returns_matching_data() {
    let (config, workspace_dir) = isolated_test_config();
    let memory_dir = workspace_dir
        .join("users")
        .join("static-token")
        .join("memory");
    std::fs::create_dir_all(&memory_dir).expect("memory dir should exist");
    std::fs::write(
        memory_dir.join("2026-04-04.md"),
        "## [10:00] Example\nsession: session-1\n\n**Facts:**\n- stable fact\n",
    )
    .expect("memory file should write");
    let state = test_state_with_config(config, false);

    let response = search_memory_files_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Query(
            serde_json::from_value(serde_json::json!({
                "q": "stable",
                "limit": 5,
                "date_from": "2026-04-01",
                "date_to": "2026-04-30"
            }))
            .unwrap(),
        ),
    )
    .await
    .expect("search memory should succeed");

    assert_eq!(response.0["data"][0]["path"], "memory/2026-04-04.md");
    let _ = std::fs::remove_dir_all(workspace_dir);
}

#[test]
fn test_http_server_listen_addr_uses_configured_host() {
    let state = test_state(false);
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
