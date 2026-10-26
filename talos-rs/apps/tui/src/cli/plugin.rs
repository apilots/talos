//! CLI request mapping only; discovery, authorization, state and installation belong to Plugin.
use super::types::{PluginCmd, PluginCommands};
use talos_sdk::{
    GatewayClient, PluginDisableRequest, PluginEnableRequest, PluginInstallConflictPolicy,
    PluginInstallId, PluginInstallRequest, PluginInstallTarget, PluginListRequest,
    PluginUninstallRequest, PluginUpdateConflictPolicy, PluginUpdateRequest,
};
use talos_types::PluginId;

pub(super) async fn execute(
    client: &mut impl GatewayClient,
    command: PluginCmd,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let node_id = command.node;
    Ok(match command.command {
        PluginCommands::List => serde_json::to_value(
            client
                .list_plugins(PluginListRequest {
                    node_id,
                    kind: None,
                    status: None,
                    source_id: None,
                    include_disabled: true,
                })
                .await?,
        )?,
        PluginCommands::Install {
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
        PluginCommands::Update {
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
        PluginCommands::Enable { install_id } => serde_json::to_value(
            client
                .enable_plugin(PluginEnableRequest {
                    node_id,
                    install_id: PluginInstallId(install_id),
                })
                .await?,
        )?,
        PluginCommands::Disable { install_id } => serde_json::to_value(
            client
                .disable_plugin(PluginDisableRequest {
                    node_id,
                    install_id: PluginInstallId(install_id),
                })
                .await?,
        )?,
        PluginCommands::Uninstall { install_id } => serde_json::to_value(
            client
                .uninstall_plugin(PluginUninstallRequest {
                    node_id,
                    install_id: PluginInstallId(install_id),
                })
                .await?,
        )?,
    })
}
