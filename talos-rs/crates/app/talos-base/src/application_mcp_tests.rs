use super::*;
use serde_json::json;
use std::fs;
use talos_types::{
    ConfigReloadRequest, MarketplaceSourceInput, McpAuthentication, McpCredentialBinding,
    McpServerListRequest, McpServerStatus, PluginDisableRequest, PluginEnableRequest,
    PluginInstallConflictPolicy, PluginInstallRequest, PluginInstallTarget, PluginKind,
    PluginListRequest, PluginSourceAddRequest, PluginUninstallRequest, PluginUpdateConflictPolicy,
    PluginUpdateRequest, PluginWarningCode,
};

pub(super) async fn available(runtime: &crate::TalosApplicationRuntime, server: &str) {
    tokio::time::timeout(Duration::from_secs(8), async {
        while !runtime
            .tool_registry
            .is_tool_available(&format!("mcp__{server}__inspect"))
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

pub(super) fn save(config: &Config) {
    fs::create_dir_all(config.talos_paths().conf_dir).unwrap();
    let mut document = serde_json::to_value(config).unwrap();
    document["logging"].as_object_mut().unwrap().remove("file");
    document["storage"]
        .as_object_mut()
        .unwrap()
        .remove("connection_string");
    fs::write(
        config.source_path(),
        serde_json::to_vec_pretty(&document).unwrap(),
    )
    .unwrap();
}

#[test]
fn unreadable_credentials_publish_a_failed_server_status() {
    let temp = tempfile::tempdir().unwrap();
    let credentials_root = temp.path().join("credentials");
    fs::create_dir_all(&credentials_root).unwrap();
    let reference = "11111111-1111-1111-1111-111111111111";
    fs::write(
        credentials_root.join(format!("{reference}.json")),
        "invalid credential record",
    )
    .unwrap();
    let mut config = talos_config::AgentKernelConfig::default();
    config.mcp.servers.push(talos_config::McpServerConfig {
        name: "broken_credentials".into(),
        credentials: Some(McpCredentialBinding {
            reference: reference.into(),
            authentication: McpAuthentication::Bearer,
        }),
        transport: talos_config::McpTransportConfig::Http {
            url: "https://example.com/mcp".into(),
            bearer_token: None,
        },
    });
    let runtime = talos_mcp::TalosMcpRuntime::new();
    let credentials = talos_service::McpCredentialStore::new(credentials_root);

    talos_worker::prepare_mcp(&runtime, &config, vec![], &credentials)
        .unwrap()
        .publish();

    let listed = runtime
        .list_servers(McpServerListRequest::default())
        .unwrap();
    assert!(matches!(
        listed.data.as_slice(),
        [server] if server.name == "broken_credentials"
            && matches!(server.status, McpServerStatus::Failed { .. })
    ));
}

#[tokio::test]
async fn application_plugin_changes_and_config_reload_publish_after_commit() {
    let mut config = Config::default();
    let workspace = isolate_runtime_workspace(&mut config, "mcp-management");
    config.gateway.auth = false;
    config.storage.connection_string = ":memory:".into();
    config.llm.anthropic.models = vec![provider_model(&config.llm.model)];
    config.llm.anthropic.api_key = None;
    config.agent.kernel.plugins.sources.clear();
    config.agent.kernel.plugins.roots.clear();
    save(&config);
    let runtime =
        build_application_runtime_with_migration_dir(config.clone(), &test_migrations_dir())
            .await
            .unwrap();
    assert!(!runtime
        .tool_registry
        .list_tool_names()
        .iter()
        .any(|name| name.starts_with("mcp__")));
    let repo = workspace.join("market");
    let source =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../action/mcp/tests/fixtures/server.py");
    for name in ["healthy", "damaged"] {
        let plugin = repo.join(name);
        fs::create_dir_all(&plugin).unwrap();
        fs::copy(&source, plugin.join("server.py")).unwrap();
        fs::write(plugin.join("talos-mcp.json"), serde_json::to_vec(&json!({
            "name": name, "transport": {"type":"stdio", "command":"/usr/bin/python3", "args":["server.py", workspace.join(name), "normal"]}
        })).unwrap()).unwrap();
    }
    let plugins = &runtime.service.plugins;
    let added = plugins
        .add_source(PluginSourceAddRequest {
            source: MarketplaceSourceInput::LocalFolder {
                path: repo.clone(),
                sparse_paths: vec![],
            },
            refresh: true,
        })
        .unwrap();
    let install = |name: &str| PluginInstallRequest {
        target: PluginInstallTarget::Plugin {
            plugin_id: added
                .plugins
                .iter()
                .find(|plugin| plugin.metadata.name == name)
                .unwrap()
                .id
                .clone(),
            expected_source_commit: None,
        },
        enable: Some(true),
        conflict_policy: PluginInstallConflictPolicy::Reject,
    };
    let healthy = plugins.install(install("healthy")).await.unwrap();
    let damaged = plugins.install(install("damaged")).await.unwrap();
    available(&runtime, "healthy").await;
    available(&runtime, "damaged").await;
    let first_pid = fs::read_to_string(workspace.join("healthy.pid")).unwrap();
    // Only installed program bytes change. The manifest and installation ID stay stable.
    let script = fs::read_to_string(repo.join("healthy/server.py")).unwrap();
    fs::write(
        repo.join("healthy/server.py"),
        script.replace("changed = False", "changed = True"),
    )
    .unwrap();
    plugins
        .update(PluginUpdateRequest {
            install_id: healthy.install.id.clone(),
            expected_source_commit: None,
            conflict_policy: PluginUpdateConflictPolicy::RejectLocalChanges,
        })
        .await
        .unwrap();
    available(&runtime, "healthy").await;
    assert_ne!(
        fs::read_to_string(workspace.join("healthy.pid")).unwrap(),
        first_pid
    );
    assert!(runtime
        .tool_registry
        .is_tool_available("mcp__healthy__changed"));
    // Disk edits are not active configuration until reload succeeds.
    config.agent.kernel.mcp.servers = vec![talos_config::McpServerConfig {
        credentials: None,
        name: "healthy".into(),
        transport: talos_config::McpTransportConfig::Stdio {
            command: "/usr/bin/python3".into(),
            args: vec![
                source.to_str().unwrap().into(),
                workspace.join("static").to_str().unwrap().into(),
                "normal".into(),
            ],
            env: Default::default(),
        },
    }];
    save(&config);
    plugins
        .disable(PluginDisableRequest {
            install_id: healthy.install.id.clone(),
        })
        .await
        .unwrap();
    assert!(!runtime
        .tool_registry
        .is_tool_available("mcp__healthy__inspect"));
    plugins
        .enable(PluginEnableRequest {
            install_id: healthy.install.id.clone(),
        })
        .await
        .unwrap();
    available(&runtime, "healthy").await;
    assert_eq!(
        runtime
            .service
            .config
            .reload_config(ConfigReloadRequest {})
            .await
            .unwrap_err()
            .error_code(),
        talos_error::ErrorCode::Conflict
    );
    assert!(runtime
        .tool_registry
        .is_tool_available("mcp__healthy__inspect"));
    config.agent.kernel.mcp.servers[0].name = "static".into();
    save(&config);
    fs::write(
        damaged.install.installed_path.join("talos-mcp.json"),
        "broken manifest",
    )
    .unwrap();
    runtime
        .service
        .config
        .reload_config(ConfigReloadRequest {})
        .await
        .unwrap();
    available(&runtime, "static").await;
    assert!(runtime
        .tool_registry
        .is_tool_available("mcp__healthy__inspect"));
    assert!(!runtime
        .tool_registry
        .is_tool_available("mcp__damaged__inspect"));
    let listed = plugins
        .list_plugins(PluginListRequest {
            kind: Some(PluginKind::McpServer),
            status: None,
            source_id: None,
            include_disabled: true,
        })
        .unwrap();
    assert!(listed
        .warnings
        .iter()
        .any(|warning| warning.code == PluginWarningCode::InvalidManifest));
    plugins
        .disable(PluginDisableRequest {
            install_id: damaged.install.id.clone(),
        })
        .await
        .unwrap();
    assert!(plugins
        .enable(PluginEnableRequest {
            install_id: damaged.install.id.clone()
        })
        .await
        .is_err());
    plugins
        .uninstall(PluginUninstallRequest {
            install_id: damaged.install.id,
        })
        .await
        .unwrap();
    plugins
        .uninstall(PluginUninstallRequest {
            install_id: healthy.install.id,
        })
        .await
        .unwrap();
    assert!(!runtime
        .tool_registry
        .is_tool_available("mcp__healthy__inspect"));
    runtime.shutdown().await.unwrap();
    assert!(!runtime
        .tool_registry
        .is_tool_available("mcp__static__inspect"));
    // A late assembly error occurs before the first MCP publication.
    fs::remove_file(workspace.join("static.pid")).unwrap();
    config.automation.profile_usage_retention.cron = "invalid cron".into();
    assert!(
        build_application_runtime_with_migration_dir(config, &test_migrations_dir())
            .await
            .is_err()
    );
    assert!(!workspace.join("static.pid").exists());
    fs::remove_dir_all(workspace).unwrap();
}
