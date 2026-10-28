use super::*;
use serde_json::json;
use std::fs;
use talos_types::*;

pub(super) struct HttpPeer(pub(super) std::process::Child);
impl Drop for HttpPeer {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[tokio::test]
async fn manual_http_credentials_are_private_replaceable_and_bound_to_their_owner() {
    let mut config = Config::default();
    let workspace = isolate_runtime_workspace(&mut config, "mcp-credentials");
    config.gateway.auth = false;
    config.storage.connection_string = ":memory:".into();
    config.llm.anthropic.models = vec![provider_model(&config.llm.model)];
    config.llm.anthropic.api_key = None;
    config.agent.kernel.plugins.sources.clear();
    config.agent.kernel.plugins.roots.clear();
    super::mcp_tests::save(&config);
    fs::write(workspace.join("expected"), "first-private-value").unwrap();
    let _peer = HttpPeer(
        std::process::Command::new("/usr/bin/python3")
            .arg(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../action/mcp/tests/fixtures/http_server.py"),
            )
            .arg(&workspace)
            .spawn()
            .unwrap(),
    );
    tokio::time::timeout(Duration::from_secs(5), async {
        while !workspace.join("port").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let url = format!(
        "http://127.0.0.1:{}/bearer",
        fs::read_to_string(workspace.join("port")).unwrap()
    );
    let runtime =
        build_application_runtime_with_migration_dir(config.clone(), &test_migrations_dir())
            .await
            .unwrap();
    let plugins = &runtime.worker.plugin_manager();
    for invalid in [
        json!({"mcpServers": {"one": {"command": "npx"}, "two": {"command": "npx"}}}),
        json!({"name": "old", "type": "sse", "url": url}),
        json!({"name": "mixed", "url": url, "command": "npx"}),
        json!({"name": "relative", "command": "npx", "cwd": "./work"}),
    ] {
        let error = plugins
            .import_mcp_server(McpServerImportRequest {
                node_id: None,
                json: invalid.to_string(),
                name: None,
                enable: Some(false),
                conflict_policy: PluginInstallConflictPolicy::Reject,
            })
            .await
            .unwrap_err();
        assert_eq!(error.error_code(), talos_error::ErrorCode::InvalidRequest);
    }

    let imported = plugins.import_mcp_server(McpServerImportRequest { node_id: None, json: json!({"mcpServers":{"private_http":{"url":url,"headers":{"Authorization":"Bearer first-private-value"}}}}).to_string(), name: None, enable: Some(true), conflict_policy: PluginInstallConflictPolicy::Reject }).await.unwrap();
    super::mcp_tests::available(&runtime, "private_http").await;
    let owner = McpDefinitionRef::Plugin {
        install_id: imported.install.id.clone(),
    };
    let read = plugins
        .read(PluginReadRequest {
            node_id: None,
            plugin_id: imported.plugin.id.clone(),
        })
        .unwrap();
    let public = serde_json::to_string(&read).unwrap();
    assert!(!public.contains("first-private-value"));
    let manifest = imported.install.installed_path.join("talos-mcp.json");
    assert!(!fs::read_to_string(&manifest)
        .unwrap()
        .contains("first-private-value"));
    let binding = match &read.plugin.definition {
        PluginDefinition::McpServer(plugin) => plugin.server.credentials.clone().unwrap(),
        _ => panic!("MCP installation"),
    };
    let credential_file = config
        .talos_paths()
        .talos_home
        .join("mcp-credentials")
        .join(format!("{}.json", binding.reference));
    assert!(fs::read_to_string(&credential_file)
        .unwrap()
        .contains("first-private-value"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&credential_file).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    // Marketplace manifests reach the same HTTP boundary without manual-input validation.
    let header_market = workspace.join("header-market");
    for (name, headers) in [
        (
            "duplicate_headers",
            json!({"X-API-Key": "wrong-value", "x-api-key": "first-private-value"}),
        ),
        (
            "normal_headers",
            json!({"x-api-key": "first-private-value"}),
        ),
    ] {
        let directory = header_market.join(name);
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join("talos-mcp.json"),
            serde_json::to_vec(&json!({
                "name": name,
                "transport": {
                    "type": "streamable_http",
                    "url": format!("{}/key", url.trim_end_matches("/bearer")),
                    "headers": headers
                }
            }))
            .unwrap(),
        )
        .unwrap();
    }
    let header_source = plugins
        .add_source(PluginSourceAddRequest {
            node_id: None,
            source: MarketplaceSourceInput::LocalFolder {
                path: header_market.clone(),
                sparse_paths: vec![],
            },
            refresh: true,
        })
        .unwrap();
    let mut normal_headers = None;
    for plugin in header_source.plugins {
        let name = plugin.metadata.name.clone();
        let installed = plugins
            .install(PluginInstallRequest {
                node_id: None,
                target: PluginInstallTarget::Plugin {
                    plugin_id: plugin.id,
                    expected_source_commit: None,
                },
                enable: Some(true),
                conflict_policy: PluginInstallConflictPolicy::Reject,
            })
            .await
            .unwrap();
        if name == "normal_headers" {
            normal_headers = Some(installed);
        }
    }
    let normal_headers = normal_headers.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let page = plugins
                .list_mcp_servers(McpServerListRequest::default())
                .unwrap();
            let server = page
                .data
                .iter()
                .find(|server| server.name == "duplicate_headers")
                .unwrap();
            match &server.status {
                McpServerStatus::Failed { error } => {
                    assert_eq!(error.code, talos_error::ErrorCode::InvalidRequest);
                    break;
                }
                McpServerStatus::Ready => {
                    panic!("case-insensitive duplicate headers were accepted")
                }
                _ => tokio::time::sleep(Duration::from_millis(10)).await,
            }
        }
    })
    .await
    .unwrap();
    assert!(!runtime
        .tool_registry
        .is_tool_available("mcp__duplicate_headers__inspect"));
    let normal_owner = McpDefinitionRef::Plugin {
        install_id: normal_headers.install.id.clone(),
    };
    runtime
        .worker
        .plugin_manager()
        .update_mcp_credentials(McpCredentialsUpdateRequest {
            node_id: None,
            server: normal_owner.clone(),
            change: McpCredentialsChange::Replace {
                credentials: McpCredentialValues {
                    authentication: McpAuthentication::Header {
                        name: "X-API-Key".into(),
                    },
                    token: Some("first-private-value".into()),
                    env: None,
                    headers: None,
                },
            },
        })
        .await
        .unwrap();
    fs::write(
        header_market.join("normal_headers").join("talos-mcp.json"),
        serde_json::to_vec(&json!({
            "name": "normal_headers",
            "description": "Updated without changing the execution target.",
            "transport": {
                "type": "streamable_http",
                "url": format!("{}/key", url.trim_end_matches("/bearer"))
            }
        }))
        .unwrap(),
    )
    .unwrap();
    plugins
        .update(PluginUpdateRequest {
            node_id: None,
            install_id: normal_headers.install.id,
            expected_source_commit: None,
            conflict_policy: PluginUpdateConflictPolicy::RejectLocalChanges,
        })
        .await
        .unwrap();
    assert!(matches!(
        plugins
            .read_mcp_authentication(McpAuthenticationReadRequest {
                node_id: None,
                server: normal_owner
            })
            .unwrap()
            .state,
        McpAuthenticationState::CredentialsStored
    ));
    for server in ["normal_headers", "private_http"] {
        super::mcp_tests::available(&runtime, server).await;
        let invocation = runtime
            .tool_registry
            .build_function_invocation("call", format!("mcp__{server}__inspect"), json!({}))
            .unwrap();
        runtime
            .tool_registry
            .execute_invocation_unchecked_with_context(
                invocation,
                talos_tools::ToolContext::new("credentials", "user"),
            )
            .await
            .unwrap();
    }
    assert_eq!(
        fs::read_to_string(workspace.join("called")).unwrap(),
        "bearer"
    );
    plugins
        .disable(PluginDisableRequest {
            node_id: None,
            install_id: imported.install.id.clone(),
        })
        .await
        .unwrap();
    plugins.recover_credentials();
    assert!(
        credential_file.exists(),
        "disabled installations retain their credentials"
    );
    plugins
        .enable(PluginEnableRequest {
            node_id: None,
            install_id: imported.install.id.clone(),
        })
        .await
        .unwrap();
    super::mcp_tests::available(&runtime, "private_http").await;
    fs::write(workspace.join("expected"), "replacement-private-value").unwrap();
    runtime
        .worker
        .plugin_manager()
        .update_mcp_credentials(McpCredentialsUpdateRequest {
            node_id: None,
            server: owner.clone(),
            change: McpCredentialsChange::Replace {
                credentials: McpCredentialValues {
                    authentication: McpAuthentication::Bearer,
                    token: Some("replacement-private-value".into()),
                    env: None,
                    headers: None,
                },
            },
        })
        .await
        .unwrap();
    super::mcp_tests::available(&runtime, "private_http").await;
    assert!(!credential_file.exists(), "replaced bundles are collected");
    let updated = plugins
        .read(PluginReadRequest {
            node_id: None,
            plugin_id: imported.plugin.id.clone(),
        })
        .unwrap();
    let mut foreign: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&manifest).unwrap()).unwrap();
    foreign["name"] = "foreign_ref".into();
    let market = workspace.join("market");
    fs::create_dir_all(&market).unwrap();
    fs::write(
        market.join("talos-mcp.json"),
        serde_json::to_vec(&foreign).unwrap(),
    )
    .unwrap();
    let source = plugins
        .add_source(PluginSourceAddRequest {
            node_id: None,
            source: MarketplaceSourceInput::LocalFolder {
                path: market,
                sparse_paths: vec![],
            },
            refresh: true,
        })
        .unwrap();
    let other = plugins
        .install(PluginInstallRequest {
            node_id: None,
            target: PluginInstallTarget::Plugin {
                plugin_id: source.plugins[0].id.clone(),
                expected_source_commit: None,
            },
            enable: Some(false),
            conflict_policy: PluginInstallConflictPolicy::Reject,
        })
        .await
        .unwrap();
    assert!(matches!(
        plugins
            .read_mcp_authentication(McpAuthenticationReadRequest {
                node_id: None,
                server: McpDefinitionRef::Plugin {
                    install_id: other.install.id
                }
            })
            .unwrap()
            .state,
        McpAuthenticationState::Unconfigured
    ));
    assert!(matches!(
        plugins
            .read_mcp_authentication(McpAuthenticationReadRequest {
                node_id: None,
                server: owner.clone()
            })
            .unwrap()
            .state,
        McpAuthenticationState::CredentialsStored
    ));
    assert!(!serde_json::to_string(&updated)
        .unwrap()
        .contains("replacement-private-value"));
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
    assert!(!runtime
        .tool_registry
        .is_tool_available("mcp__private_http__inspect"));
    assert!(matches!(
        plugins
            .read_mcp_authentication(McpAuthenticationReadRequest {
                node_id: None,
                server: owner
            })
            .unwrap()
            .state,
        McpAuthenticationState::Missing
    ));
    runtime.shutdown().await.unwrap();
    fs::remove_dir_all(workspace).unwrap();
}
