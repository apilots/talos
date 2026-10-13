use crate::{test_support::manager_with_state, PluginStateStore};
use talos_types::*;

#[test]
fn manual_mcp_uses_installed_manifest_and_existing_replacement_transaction() {
    let temp = tempfile::tempdir().unwrap();
    let state = PluginStateStore::new(temp.path());
    let manager = manager_with_state(state.clone());
    let mut server: McpServerDefinition = serde_json::from_value(serde_json::json!({
        "name": "Local_Tools", "display_name": "Local tools", "description": "Manual fixture",
        "transport": { "type": "stdio", "command": "npx", "args": ["--yes", "fixture-mcp"], "env": {"PUBLIC_MODE": "test"}, "cwd": temp.path() }
    })).unwrap();
    let request = |server: McpServerDefinition, conflict_policy| PluginInstallRequest {
        node_id: None,
        target: PluginInstallTarget::ManualMcpServer {
            server: Box::new(server),
            credentials: None,
        },
        enable: Some(true),
        conflict_policy,
    };
    let first = manager
        .install(request(server.clone(), PluginInstallConflictPolicy::Reject))
        .unwrap();
    assert_eq!(first.install.source, PluginInstallSource::Manual);
    assert!(matches!(
        first.plugin.status,
        PluginStatus::Installed { enabled: true, .. }
    ));
    let read = manager
        .read(PluginReadRequest {
            node_id: None,
            plugin_id: first.plugin.id.clone(),
        })
        .unwrap();
    assert!(read.documents[0].contents.contains("fixture-mcp"));
    assert!(manager
        .install(request(server.clone(), PluginInstallConflictPolicy::Reject))
        .is_err());
    if let McpTransportDefinition::Stdio { args, .. } = &mut server.transport {
        args.push("--new".into());
    }
    state.fail_next_save_installs();
    assert!(manager
        .install(request(
            server.clone(),
            PluginInstallConflictPolicy::Replace
        ))
        .is_err());
    assert!(!manager
        .read(PluginReadRequest {
            node_id: None,
            plugin_id: first.plugin.id.clone()
        })
        .unwrap()
        .documents[0]
        .contents
        .contains("--new"));
    let replaced = manager
        .install(request(
            server.clone(),
            PluginInstallConflictPolicy::Replace,
        ))
        .unwrap();
    assert_eq!(replaced.install.id, first.install.id);
    assert_ne!(
        replaced.install.content_digest,
        first.install.content_digest
    );
    assert_eq!(manager.mcp_servers().unwrap().servers[0].server, server);
    let mut invalid = server.clone();
    invalid.name = "../escape".into();
    assert!(manager
        .install(request(invalid, PluginInstallConflictPolicy::Replace))
        .is_err());
    if let McpTransportDefinition::Stdio { cwd, .. } = &mut server.transport {
        *cwd = Some("relative".into());
    }
    assert!(manager
        .install(request(server, PluginInstallConflictPolicy::Replace))
        .is_err());
    manager
        .disable(PluginDisableRequest {
            node_id: None,
            install_id: first.install.id.clone(),
        })
        .unwrap();
    assert!(manager.mcp_servers().unwrap().servers.is_empty());
    manager
        .enable(PluginEnableRequest {
            node_id: None,
            install_id: first.install.id.clone(),
        })
        .unwrap();
    assert_eq!(manager.mcp_servers().unwrap().servers.len(), 1);
    manager
        .uninstall(PluginUninstallRequest {
            node_id: None,
            install_id: first.install.id,
        })
        .unwrap();
    assert!(!first.install.installed_path.exists());
    assert!(state.load_installs().unwrap().installs.is_empty());
}
