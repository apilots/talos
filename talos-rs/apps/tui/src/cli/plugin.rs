//! CLI request mapping only; discovery, authorization, state and installation belong to Plugin.
use super::types::{
    MarketplaceCommand, PluginCmd, PluginCommands, PluginInstallationCommand, TypedPluginCommand,
};
use std::path::PathBuf;
use talos_sdk::{
    ErrorCode, GatewayClient, MarketplaceSourceInput, McpServerImportRequest, PluginDisableRequest,
    PluginEnableRequest, PluginInstallConflictPolicy, PluginInstallId, PluginInstallRequest,
    PluginInstallTarget, PluginKind, PluginListRequest, PluginListResponse,
    PluginSearchRefreshPolicy, PluginSearchRequest, PluginSourceAddRequest,
    PluginSourceListRequest, PluginSourceRefreshRequest, PluginSourceRemoveRequest, PluginStatus,
    PluginUninstallRequest, PluginUpdateConflictPolicy, PluginUpdateRequest, TalosError,
};
use talos_types::{MarketplaceSourceId, PluginId};

pub(super) async fn execute(
    client: &mut impl GatewayClient,
    command: PluginCmd,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let node_id = command.node;
    Ok(match command.command {
        PluginCommands::Skill { command } => {
            typed(client, node_id, PluginKind::Skill, command).await?
        }
        PluginCommands::Mcp { command } => {
            typed(client, node_id, PluginKind::McpServer, command).await?
        }
        PluginCommands::Marketplace { command } => marketplace(client, node_id, command).await?,
        PluginCommands::List => serde_json::to_value(list_installed(client, node_id, None).await?)?,
        PluginCommands::ImportMcp { file } => serde_json::to_value(
            client
                .import_mcp_server(McpServerImportRequest {
                    node_id,
                    json: std::fs::read_to_string(file)?,
                    name: None,
                    enable: Some(true),
                    conflict_policy: PluginInstallConflictPolicy::Reject,
                })
                .await?,
        )?,
        PluginCommands::Installation(command) => installation(client, node_id, command).await?,
    })
}

async fn list_installed(
    client: &mut impl GatewayClient,
    node_id: Option<String>,
    kind: Option<PluginKind>,
) -> Result<PluginListResponse, Box<dyn std::error::Error>> {
    Ok(client
        .list_plugins(PluginListRequest {
            node_id,
            kind,
            status: None,
            source_id: None,
            include_disabled: true,
        })
        .await?)
}

async fn installation(
    client: &mut impl GatewayClient,
    node_id: Option<String>,
    command: PluginInstallationCommand,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    Ok(match command {
        PluginInstallationCommand::Install {
            plugin_id,
            commit,
            disabled,
            replace,
        } => serde_json::to_value(
            client
                .install_plugin(PluginInstallRequest {
                    node_id,
                    target: PluginInstallTarget::Plugin {
                        plugin_id: PluginId(plugin_id),
                        expected_source_commit: commit,
                    },
                    enable: Some(!disabled),
                    conflict_policy: if replace {
                        PluginInstallConflictPolicy::Replace
                    } else {
                        PluginInstallConflictPolicy::Reject
                    },
                })
                .await?,
        )?,
        PluginInstallationCommand::Update {
            install_id,
            commit,
            replace_local_changes,
        } => serde_json::to_value(
            client
                .update_plugin(PluginUpdateRequest {
                    node_id,
                    install_id: PluginInstallId(install_id),
                    expected_source_commit: commit,
                    conflict_policy: if replace_local_changes {
                        PluginUpdateConflictPolicy::ReplaceLocalChanges
                    } else {
                        PluginUpdateConflictPolicy::RejectLocalChanges
                    },
                })
                .await?,
        )?,
        PluginInstallationCommand::Enable { install_id } => serde_json::to_value(
            client
                .enable_plugin(PluginEnableRequest {
                    node_id,
                    install_id: PluginInstallId(install_id),
                })
                .await?,
        )?,
        PluginInstallationCommand::Disable { install_id } => serde_json::to_value(
            client
                .disable_plugin(PluginDisableRequest {
                    node_id,
                    install_id: PluginInstallId(install_id),
                })
                .await?,
        )?,
        PluginInstallationCommand::Uninstall { install_id } => serde_json::to_value(
            client
                .uninstall_plugin(PluginUninstallRequest {
                    node_id,
                    install_id: PluginInstallId(install_id),
                })
                .await?,
        )?,
    })
}

async fn typed(
    client: &mut impl GatewayClient,
    node_id: Option<String>,
    kind: PluginKind,
    command: TypedPluginCommand,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    if let TypedPluginCommand::Add {
        name,
        file,
        replace,
    } = command
    {
        let plugin = if let Some(path) = file {
            let path = worker_path(path, node_id.as_deref())?;
            let added = client
                .add_plugin_source(PluginSourceAddRequest {
                    node_id: node_id.clone(),
                    source: MarketplaceSourceInput::LocalPackage { path },
                    refresh: true,
                })
                .await?;
            unique(
                added
                    .plugins
                    .into_iter()
                    .filter(|plugin| plugin.kind == kind),
                "The package has no plugin of the selected type.",
                "The package has more than one plugin of the selected type.",
            )?
        } else {
            let selector = name.expect("CLI requires a name or file");
            let (name, source_id) = match selector.rsplit_once('@') {
                Some((name, marketplace)) => (
                    name,
                    Some(source_id(client, node_id.clone(), marketplace).await?),
                ),
                None => (selector.as_str(), None),
            };
            let discovered = client
                .search_plugins(PluginSearchRequest {
                    node_id: node_id.clone(),
                    query: Some(name.to_owned()),
                    source_id,
                    kind: Some(kind),
                    status: None,
                    refresh: PluginSearchRefreshPolicy::RefreshStale,
                    limit: None,
                })
                .await?;
            unique(
                discovered
                    .plugins
                    .into_iter()
                    .filter(|plugin| plugin.metadata.name == name),
                "The named plugin was not found in the marketplace.",
                "More than one plugin has this name. Select a unique name and marketplace.",
            )?
        };
        return installation(
            client,
            node_id,
            PluginInstallationCommand::Install {
                plugin_id: plugin.id.0,
                commit: plugin.source_commit,
                disabled: false,
                replace,
            },
        )
        .await;
    }

    let listed = list_installed(client, node_id.clone(), Some(kind)).await?;
    let name = match &command {
        TypedPluginCommand::List => return Ok(serde_json::to_value(listed)?),
        TypedPluginCommand::Remove { name }
        | TypedPluginCommand::Enable { name }
        | TypedPluginCommand::Disable { name }
        | TypedPluginCommand::Update { name } => name,
        TypedPluginCommand::Add { .. } => unreachable!(),
    };
    let install_id = unique(
        listed
            .plugins
            .into_iter()
            .filter(|plugin| plugin.metadata.name == *name)
            .filter_map(|plugin| match plugin.status {
                PluginStatus::Installed { install_id, .. }
                | PluginStatus::SourceMissing { install_id, .. }
                | PluginStatus::UpdateAvailable { install_id, .. } => Some(install_id),
                PluginStatus::Conflict { install_id, .. } => install_id,
                PluginStatus::NotInstalled => None,
            }),
        "The named plugin is not installed on this Worker.",
        "More than one installation has the selected name.",
    )?
    .0;

    let command = match command {
        TypedPluginCommand::Remove { .. } => PluginInstallationCommand::Uninstall { install_id },
        TypedPluginCommand::Enable { .. } => PluginInstallationCommand::Enable { install_id },
        TypedPluginCommand::Disable { .. } => PluginInstallationCommand::Disable { install_id },
        TypedPluginCommand::Update { .. } => PluginInstallationCommand::Update {
            install_id,
            commit: None,
            replace_local_changes: false,
        },
        TypedPluginCommand::Add { .. } | TypedPluginCommand::List => unreachable!(),
    };
    installation(client, node_id, command).await
}

async fn marketplace(
    client: &mut impl GatewayClient,
    node_id: Option<String>,
    command: MarketplaceCommand,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    Ok(match command {
        MarketplaceCommand::Add {
            location,
            reference,
        } => {
            let source = if location.starts_with("https://")
                || location.starts_with("git@")
                || location.starts_with("file://")
            {
                MarketplaceSourceInput::Git {
                    url: location,
                    reference: reference.unwrap_or_else(|| "main".into()),
                    sparse_paths: Vec::new(),
                }
            } else {
                if reference.is_some() {
                    return Err(TalosError::new(
                        ErrorCode::InvalidRequest,
                        "A Git reference requires a Git source.",
                    )
                    .into());
                }
                MarketplaceSourceInput::LocalFolder {
                    path: worker_path(location.into(), node_id.as_deref())?,
                    sparse_paths: Vec::new(),
                }
            };
            serde_json::to_value(
                client
                    .add_plugin_source(PluginSourceAddRequest {
                        node_id,
                        source,
                        refresh: true,
                    })
                    .await?,
            )?
        }
        MarketplaceCommand::List => serde_json::to_value(
            client
                .list_plugin_sources(PluginSourceListRequest { node_id })
                .await?,
        )?,
        MarketplaceCommand::Refresh { marketplace } => {
            let source_id = source_id(client, node_id.clone(), &marketplace).await?;
            serde_json::to_value(
                client
                    .refresh_plugin_source(PluginSourceRefreshRequest { node_id, source_id })
                    .await?,
            )?
        }
        MarketplaceCommand::Remove { marketplace } => {
            let source_id = source_id(client, node_id.clone(), &marketplace).await?;
            serde_json::to_value(
                client
                    .remove_plugin_source(PluginSourceRemoveRequest { node_id, source_id })
                    .await?,
            )?
        }
    })
}

async fn source_id(
    client: &mut impl GatewayClient,
    node_id: Option<String>,
    name: &str,
) -> Result<MarketplaceSourceId, Box<dyn std::error::Error>> {
    let listed = client
        .list_plugin_sources(PluginSourceListRequest { node_id })
        .await?;
    if let Some(source) = listed.sources.iter().find(|source| source.id.0 == name) {
        return Ok(source.id.clone());
    }
    Ok(unique(
        listed
            .sources
            .into_iter()
            .filter(|source| source.display_name == name)
            .map(|source| source.id),
        "The marketplace was not found.",
        "More than one marketplace has this name. Use the source ID from marketplace list.",
    )?)
}

fn worker_path(path: PathBuf, node: Option<&str>) -> Result<PathBuf, Box<dyn std::error::Error>> {
    if path.is_absolute() {
        return Ok(path);
    }
    if node.is_some_and(|node| node != "embedded") {
        return Err(TalosError::new(
            ErrorCode::InvalidRequest,
            "Use an absolute path on the selected Worker.",
        )
        .into());
    }
    Ok(std::env::current_dir()?.join(path))
}

fn unique<T>(
    mut matches: impl Iterator<Item = T>,
    missing: &'static str,
    ambiguous: &'static str,
) -> Result<T, TalosError> {
    let value = matches
        .next()
        .ok_or_else(|| TalosError::new(ErrorCode::ResourceNotFound, missing))?;
    if matches.next().is_some() {
        return Err(TalosError::new(ErrorCode::Conflict, ambiguous));
    }
    Ok(value)
}
