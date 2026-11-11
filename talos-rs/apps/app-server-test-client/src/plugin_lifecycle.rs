use std::fs;
use std::path::PathBuf;

use anyhow::{ensure, Context, Result};
use talos_sdk::{
    AppServerClient, GatewayClient, MarketplaceSourceInput, PluginDisableRequest,
    PluginEnableRequest, PluginInstallConflictPolicy, PluginInstallRequest, PluginInstallTarget,
    PluginKind, PluginListRequest, PluginReadRequest, PluginSearchRefreshPolicy,
    PluginSearchRequest, PluginSourceAddRequest, PluginSourceListRequest,
    PluginSourceRefreshRequest, PluginSourceRemoveRequest, PluginStatus, PluginUninstallRequest,
    PluginUpdateConflictPolicy, PluginUpdateRequest,
};

pub(super) async fn run(client: &mut AppServerClient) -> Result<()> {
    let catalog = PathBuf::from(
        std::env::var_os("TALOS_E2E_PLUGIN_CATALOG").context("Plugin catalog fixture path")?,
    );
    let added = client
        .add_plugin_source(PluginSourceAddRequest {
            node_id: None,
            source: MarketplaceSourceInput::LocalFolder {
                path: catalog.clone(),
                sparse_paths: Vec::new(),
            },
            refresh: true,
        })
        .await?;
    let source_id = added.source.id;
    ensure!(
        added.plugins.len() == 2,
        "Expected one Skill and one MCP plugin"
    );
    ensure!(
        client
            .list_plugin_sources(PluginSourceListRequest { node_id: None })
            .await?
            .sources
            .iter()
            .any(|source| source.id == source_id),
        "Added catalog source is missing from the source list"
    );
    let mut installed = Vec::new();
    for (kind, name, file) in [
        (PluginKind::Skill, "cli-skill", "SKILL.md"),
        (PluginKind::McpServer, "cli-mcp", "README.md"),
    ] {
        let found = client
            .search_plugins(PluginSearchRequest {
                node_id: None,
                query: Some(name.to_string()),
                source_id: Some(source_id.clone()),
                kind: Some(kind),
                status: None,
                refresh: PluginSearchRefreshPolicy::CacheOnly,
                limit: None,
            })
            .await?;
        let plugin = found.plugins.first().context("Expected a catalog entry")?;
        ensure!(
            plugin.metadata.name == name,
            "Search returned a different plugin"
        );
        let document = client
            .read_plugin(PluginReadRequest {
                node_id: None,
                plugin_id: plugin.id.clone(),
            })
            .await?;
        ensure!(
            document
                .documents
                .iter()
                .any(|document| document.contents.contains("VERSION_ONE")),
            "Plugin documents did not include the catalog contents for {name}"
        );
        let response = client
            .install_plugin(PluginInstallRequest {
                node_id: None,
                target: PluginInstallTarget::Plugin {
                    plugin_id: plugin.id.clone(),
                    expected_source_commit: plugin.source_commit.clone(),
                },
                enable: Some(true),
                conflict_policy: PluginInstallConflictPolicy::Reject,
            })
            .await?;
        ensure!(response.install.enabled, "Install did not enable {name}");
        let disabled = client
            .disable_plugin(PluginDisableRequest {
                node_id: None,
                install_id: response.install.id.clone(),
            })
            .await?;
        ensure!(
            matches!(
                disabled.plugin.status,
                PluginStatus::Installed { enabled: false, .. }
            ),
            "Disable did not change {name}"
        );
        let source_file = catalog
            .join(
                plugin
                    .path_in_source
                    .as_deref()
                    .context("Plugin catalog path")?,
            )
            .join(file);
        let contents = fs::read_to_string(&source_file)?.replace("VERSION_ONE", "VERSION_TWO");
        fs::write(source_file, contents)?;
        installed.push((response.install, file));
    }
    eprintln!("Plugin API catalog search, document read, install, and disable passed");

    let refreshed = client
        .refresh_plugin_source(PluginSourceRefreshRequest {
            node_id: None,
            source_id: source_id.clone(),
        })
        .await?;
    ensure!(
        refreshed.plugins.len() == 2,
        "Refresh changed the catalog size"
    );
    let list = PluginListRequest {
        node_id: None,
        kind: None,
        status: None,
        source_id: Some(source_id.clone()),
        include_disabled: true,
    };
    ensure!(
        client.list_plugins(list.clone()).await?.plugins.len() == 2,
        "Disabled plugins are missing from the managed list"
    );
    for (install, file) in installed {
        let updated = client
            .update_plugin(PluginUpdateRequest {
                node_id: None,
                install_id: install.id.clone(),
                expected_source_commit: None,
                conflict_policy: PluginUpdateConflictPolicy::RejectLocalChanges,
            })
            .await?;
        ensure!(
            updated.install.id == install.id && !updated.install.enabled,
            "Update changed the install identity or enablement for {}",
            install.name
        );
        ensure!(
            fs::read_to_string(updated.install.installed_path.join(file))?.contains("VERSION_TWO"),
            "Update did not copy the new catalog contents for {}",
            install.name
        );
        let enabled = client
            .enable_plugin(PluginEnableRequest {
                node_id: None,
                install_id: install.id.clone(),
            })
            .await?;
        ensure!(
            matches!(
                enabled.plugin.status,
                PluginStatus::Installed { enabled: true, .. }
            ),
            "Enable did not change {}",
            install.name
        );
        ensure!(
            client
                .uninstall_plugin(PluginUninstallRequest {
                    node_id: None,
                    install_id: install.id,
                })
                .await?
                .uninstalled
                && !install.installed_path.exists(),
            "Uninstall did not remove {}",
            install.name
        );
    }
    ensure!(
        client.list_plugins(list).await?.plugins.is_empty(),
        "Uninstalled plugins remained in the managed list"
    );
    ensure!(
        client
            .remove_plugin_source(PluginSourceRemoveRequest {
                node_id: None,
                source_id: source_id.clone(),
            })
            .await?
            .removed,
        "The catalog source was not removed"
    );
    ensure!(
        !client
            .list_plugin_sources(PluginSourceListRequest { node_id: None })
            .await?
            .sources
            .iter()
            .any(|source| source.id == source_id),
        "Removed catalog source is still listed"
    );
    eprintln!("Plugin API refresh, update, enable, uninstall, and source removal passed");
    Ok(())
}
