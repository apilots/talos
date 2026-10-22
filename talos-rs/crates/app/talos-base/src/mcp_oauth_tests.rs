use super::*;
use serde_json::json;
use std::fs;
use talos_types::*;

#[tokio::test]
async fn oauth_browser_grant_refresh_disconnect_and_reauthorize_use_the_installed_service() {
    let mut config = Config::default();
    let workspace = isolate_runtime_workspace(&mut config, "mcp-oauth");
    let log_file = workspace.join("oauth-diagnostics.jsonl");
    let log_guard = talos_logging::init_with_config(
        talos_logging::LoggingConfig {
            file: Some(log_file.to_string_lossy().into_owned()),
            stdout: false,
            format: "json".into(),
            rotation: None, // This fixture verifies diagnostic contents in one file.
            ..Default::default()
        },
        "UTC",
    )
    .unwrap();
    config.gateway.auth = false;
    config.storage.connection_string = ":memory:".into();
    config.llm.anthropic.models = vec![provider_model(&config.llm.model)];
    config.llm.anthropic.api_key = None;
    config.agent.kernel.plugins.sources.clear();
    config.agent.kernel.plugins.roots.clear();
    super::mcp_tests::save(&config);
    let _peer = super::mcp_credentials_tests::HttpPeer(
        std::process::Command::new("/usr/bin/python3")
            .arg(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../action/mcp/tests/fixtures/oauth_server.py"),
            )
            .arg(&workspace)
            .spawn()
            .unwrap(),
    );
    wait_file(&workspace.join("port")).await;
    let url = format!(
        "http://127.0.0.1:{}/bearer",
        fs::read_to_string(workspace.join("port")).unwrap()
    );
    let runtime =
        build_application_runtime_with_migration_dir(config.clone(), &test_migrations_dir())
            .await
            .unwrap();
    let imported = runtime
        .worker
        .plugin_manager()
        .import_mcp_server(McpServerImportRequest {
            node_id: None,
            json: json!({"name":"oauth_http","url":url}).to_string(),
            name: None,
            enable: Some(true),
            conflict_policy: PluginInstallConflictPolicy::Reject,
        })
        .await
        .unwrap();
    let owner = McpDefinitionRef::Plugin {
        install_id: imported.install.id.clone(),
    };
    let callback_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let callback_port = callback_listener.local_addr().unwrap().port();
    drop(callback_listener);
    let request = || McpAuthenticationStartRequest {
        node_id: None,
        server: owner.clone(),
        config: McpOAuthConfig {
            scopes: vec!["read".into()],
            callback_port: Some(callback_port),
            ..Default::default()
        },
        client_secret: None,
    };
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let plugins = runtime.worker.plugin_manager();
    let replacements = tokio::join!(
        plugins.start_mcp_authentication(request()),
        plugins.start_mcp_authentication(request()),
        plugins.start_mcp_authentication(request())
    );
    let mut started = None;
    for result in [replacements.0, replacements.1, replacements.2] {
        match result {
            Ok(value) => {
                assert!(started.is_none());
                started = Some(value);
            }
            Err(error) => assert_eq!(error.error_code(), talos_error::ErrorCode::Cancelled),
        }
    }
    let started =
        started.expect("the current authorization attempt starts on the shared callback port");
    let authorization_url = reqwest::Url::parse(&started.authorization_url).unwrap();
    let redirect = authorization_url
        .query_pairs()
        .find(|(key, _)| key == "redirect_uri")
        .unwrap()
        .1
        .to_string();
    assert_eq!(
        http.get(format!("{redirect}?state=unrelated&code=invalid"))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    let grant = http.get(&started.authorization_url).send().await.unwrap();
    assert_eq!(grant.status(), 302);
    let callback = grant.headers()["location"].to_str().unwrap();
    let mut wrong_issuer = reqwest::Url::parse(callback).unwrap();
    let query = wrong_issuer
        .query_pairs()
        .map(|(key, value)| {
            (
                key.to_string(),
                if key == "iss" {
                    "https://wrong.invalid".to_string()
                } else {
                    value.to_string()
                },
            )
        })
        .collect::<Vec<_>>();
    wrong_issuer.query_pairs_mut().clear().extend_pairs(query);
    http.get(wrong_issuer).send().await.unwrap();
    http.get(callback).send().await.unwrap();
    super::mcp_tests::available(&runtime, "oauth_http").await;
    invoke(&runtime).await;
    let read = runtime
        .worker
        .plugin_manager()
        .read(PluginReadRequest {
            node_id: None,
            plugin_id: imported.plugin.id.clone(),
        })
        .unwrap();
    assert!(!serde_json::to_string(&read)
        .unwrap()
        .contains("private-access"));
    let server = match read.plugin.definition {
        PluginDefinition::McpServer(plugin) => plugin.server,
        _ => panic!("MCP definition"),
    };
    let credential_file = config
        .talos_paths()
        .talos_home
        .join("mcp-credentials")
        .join(format!("{}.json", server.credentials.unwrap().reference));
    assert!(fs::read_to_string(&credential_file)
        .unwrap()
        .contains("private-refresh"));
    expire(&credential_file);
    invoke(&runtime).await;
    assert_eq!(
        fs::read_to_string(workspace.join("refresh-count")).unwrap(),
        "1"
    );
    assert!(fs::read_to_string(&credential_file)
        .unwrap()
        .contains("refreshed-private-access"));

    runtime
        .worker
        .plugin_manager()
        .disable(PluginDisableRequest {
            node_id: None,
            install_id: imported.install.id.clone(),
        })
        .await
        .unwrap();
    fs::write(workspace.join("changed-resource"), "").unwrap();
    runtime
        .worker
        .plugin_manager()
        .enable(PluginEnableRequest {
            node_id: None,
            install_id: imported.install.id.clone(),
        })
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let state = runtime
                .worker
                .plugin_manager()
                .list_mcp_servers(McpServerListRequest::default())
                .unwrap();
            if state.data.iter().any(|server| {
                server.name == "oauth_http"
                    && matches!(server.status, McpServerStatus::Failed { .. })
            }) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(!runtime
        .tool_registry
        .is_tool_available("mcp__oauth_http__inspect"));
    fs::remove_file(workspace.join("changed-resource")).unwrap();
    runtime
        .worker
        .plugin_manager()
        .disable(PluginDisableRequest {
            node_id: None,
            install_id: imported.install.id.clone(),
        })
        .await
        .unwrap();
    runtime
        .worker
        .plugin_manager()
        .enable(PluginEnableRequest {
            node_id: None,
            install_id: imported.install.id.clone(),
        })
        .await
        .unwrap();
    super::mcp_tests::available(&runtime, "oauth_http").await;
    invoke(&runtime).await;

    let stale = runtime
        .worker
        .plugin_manager()
        .start_mcp_authentication(request())
        .await
        .unwrap();
    let grant = http.get(&stale.authorization_url).send().await.unwrap();
    let stale_callback = grant.headers()["location"].to_str().unwrap().to_string();
    runtime
        .worker
        .plugin_manager()
        .update_mcp_credentials(McpCredentialsUpdateRequest {
            node_id: None,
            server: owner.clone(),
            change: McpCredentialsChange::Clear,
        })
        .await
        .unwrap();
    let _ = http.get(stale_callback).send().await;
    assert!(!credential_file.exists());
    assert!(matches!(
        runtime
            .worker
            .plugin_manager()
            .read_mcp_authentication(McpAuthenticationReadRequest {
                node_id: None,
                server: owner.clone()
            })
            .unwrap()
            .state,
        McpAuthenticationState::Missing
    ));

    let restarted = runtime
        .worker
        .plugin_manager()
        .start_mcp_authentication(request())
        .await
        .unwrap();
    let grant = http.get(&restarted.authorization_url).send().await.unwrap();
    http.get(grant.headers()["location"].to_str().unwrap())
        .send()
        .await
        .unwrap();
    super::mcp_tests::available(&runtime, "oauth_http").await;
    invoke(&runtime).await;
    let read = runtime
        .worker
        .plugin_manager()
        .read(PluginReadRequest {
            node_id: None,
            plugin_id: imported.plugin.id.clone(),
        })
        .unwrap();
    let current = match read.plugin.definition {
        PluginDefinition::McpServer(plugin) => plugin.server,
        _ => panic!("MCP definition"),
    };
    let store = talos_service::McpCredentialStore::new(
        config.talos_paths().talos_home.join("mcp-credentials"),
    );
    let source = match store.resolve(&owner, &current).unwrap().unwrap() {
        talos_mcp::McpConnectionCredentials::OAuth(source) => source,
        _ => panic!("OAuth connection"),
    };
    // A real refresh-lock failure must retain the storage error at the connection owner.
    let refresh_lock = config
        .talos_paths()
        .talos_home
        .join("mcp-credentials")
        .join(format!("{}.refresh.lock", source.reference));
    fs::create_dir(&refresh_lock).unwrap();
    let storage_error = source.store.refresh_guard().await.err().unwrap();
    expire(&refresh_lock.with_file_name(format!("{}.json", source.reference)));
    let invocation = runtime
        .tool_registry
        .build_function_invocation("storage-failure", "mcp__oauth_http__inspect", json!({}))
        .unwrap();
    assert!(runtime
        .tool_registry
        .execute_invocation_unchecked_with_context(
            invocation,
            talos_tools::ToolContext::new("oauth", "user")
        )
        .await
        .is_err());
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let page = runtime
                .worker
                .plugin_manager()
                .list_mcp_servers(McpServerListRequest::default())
                .unwrap();
            if let Some(server) = page.data.iter().find(|server| server.name == "oauth_http") {
                if let McpServerStatus::Failed { error } = &server.status {
                    assert_eq!(error, &storage_error.to_api_error());
                    break;
                }
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    fs::remove_dir(&refresh_lock).unwrap();
    runtime
        .worker
        .plugin_manager()
        .disable(PluginDisableRequest {
            node_id: None,
            install_id: imported.install.id.clone(),
        })
        .await
        .unwrap();
    runtime
        .worker
        .plugin_manager()
        .enable(PluginEnableRequest {
            node_id: None,
            install_id: imported.install.id.clone(),
        })
        .await
        .unwrap();
    super::mcp_tests::available(&runtime, "oauth_http").await;
    invoke(&runtime).await;

    // A second process can already hold a refresh result when this process disconnects.
    let pending_write = source.store.load().await.unwrap().unwrap();
    fs::write(workspace.join("hold-refresh"), "").unwrap();
    fs::remove_file(workspace.join("refresh-count")).unwrap();
    expire(
        &config
            .talos_paths()
            .talos_home
            .join("mcp-credentials")
            .join(format!("{}.json", source.reference)),
    );
    let registry = runtime.tool_registry.clone();
    let calling = tokio::spawn(async move {
        let invocation = registry
            .build_function_invocation("blocked-refresh", "mcp__oauth_http__inspect", json!({}))
            .unwrap();
        registry
            .execute_invocation_unchecked_with_context(
                invocation,
                talos_tools::ToolContext::new("oauth", "user"),
            )
            .await
    });
    wait_file(&workspace.join("refresh-count")).await;
    runtime
        .worker
        .plugin_manager()
        .update_mcp_credentials(McpCredentialsUpdateRequest {
            node_id: None,
            server: owner.clone(),
            change: McpCredentialsChange::Clear,
        })
        .await
        .unwrap();
    fs::remove_file(workspace.join("hold-refresh")).unwrap();
    assert!(calling.await.unwrap().is_err());
    assert!(
        source.store.save(pending_write).await.is_err(),
        "a late writer must not recreate disconnected credentials"
    );
    assert!(source.store.load().await.unwrap().is_none());
    fs::write(workspace.join("hold-discovery"), "").unwrap();
    let service = runtime.worker.plugin_manager();
    let request = request();
    let discovering = tokio::spawn(async move { service.start_mcp_authentication(request).await });
    wait_file(&workspace.join("discovery-started")).await;
    runtime.shutdown().await.unwrap();
    let _released_callback =
        std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, callback_port))
            .expect("shutdown releases callback listeners even during discovery");
    assert!(discovering.await.unwrap().is_err());
    fs::remove_file(workspace.join("hold-discovery")).unwrap();
    drop(log_guard);
    let diagnostics = fs::read_to_string(log_file)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .filter(|event| event["fields"]["public_message"] == storage_error.to_api_error().message)
        .collect::<Vec<_>>();
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0]["fields"]["context"]
        .as_str()
        .unwrap()
        .contains("mcp_server=oauth_http"));
    assert_eq!(
        diagnostics[0]["fields"]["sources"],
        format!(
            "0: {}\n",
            std::error::Error::source(&storage_error).unwrap()
        )
    );
    assert_eq!(
        diagnostics[0]["fields"]["location"],
        storage_error.location().to_string()
    );
}
async fn wait_file(path: &Path) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !path.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}
async fn invoke(runtime: &crate::TalosApplicationRuntime) {
    let invocation = runtime
        .tool_registry
        .build_function_invocation("call", "mcp__oauth_http__inspect", json!({}))
        .unwrap();
    runtime
        .tool_registry
        .execute_invocation_unchecked_with_context(
            invocation,
            talos_tools::ToolContext::new("oauth", "user"),
        )
        .await
        .unwrap();
}

fn expire(path: &Path) {
    let mut record: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    record["values"]["OAuth"]["stored"]["token_received_at"] = json!(0);
    fs::write(path, serde_json::to_vec(&record).unwrap()).unwrap();
}
