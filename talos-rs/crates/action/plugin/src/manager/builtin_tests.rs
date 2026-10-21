use crate::{test_support::manager_with_state, PluginStateStore};
use talos_types::*;

#[test]
fn builtin_browser_is_idempotent_preserves_disable_and_cannot_be_removed() {
    let temp = tempfile::tempdir().unwrap();
    let state = PluginStateStore::new(temp.path());
    let manager = manager_with_state(state.clone());
    let executable = temp.path().join("talos-worker");
    manager.provision_builtin_browser(&executable).unwrap();
    let first = state.load_installs().unwrap();
    assert_eq!(first.installs.len(), 1);
    assert!(first.installs[0].enabled);
    assert_eq!(first.installs[0].plugin_id.0, BUILTIN_BROWSER_PLUGIN_ID);
    manager.provision_builtin_browser(&executable).unwrap();
    assert_eq!(state.load_installs().unwrap(), first);
    let id = first.installs[0].id.clone();
    manager
        .disable(PluginDisableRequest {
            node_id: None,
            install_id: id.clone(),
        })
        .unwrap();
    let reopened = manager_with_state(state.clone());
    reopened
        .provision_builtin_browser(&temp.path().join("upgraded-worker"))
        .unwrap();
    let upgraded = state.load_installs().unwrap();
    assert_eq!(upgraded.installs.len(), 1);
    assert_eq!(upgraded.installs[0].id, id);
    assert!(!upgraded.installs[0].enabled);
    let failure = reopened
        .uninstall(PluginUninstallRequest {
            node_id: None,
            install_id: id.clone(),
        })
        .unwrap_err();
    assert_eq!(
        failure.to_api_error().code,
        talos_error::ErrorCode::InvalidRequest
    );
    reopened
        .enable(PluginEnableRequest {
            node_id: None,
            install_id: id,
        })
        .unwrap();
    assert!(state.load_installs().unwrap().installs[0].enabled);
}

#[test]
fn builtin_browser_upgrade_rolls_back_when_state_commit_fails() {
    let temp = tempfile::tempdir().unwrap();
    let state = PluginStateStore::new(temp.path());
    let manager = manager_with_state(state.clone());
    manager
        .provision_builtin_browser(&temp.path().join("worker"))
        .unwrap();
    let before = state.load_installs().unwrap();
    let path = before.installs[0].installed_path.join("talos-mcp.json");
    let manifest = std::fs::read(&path).unwrap();
    state.fail_next_save_installs();
    assert!(manager
        .provision_builtin_browser(&temp.path().join("new-worker"))
        .is_err());
    assert_eq!(state.load_installs().unwrap(), before);
    assert_eq!(std::fs::read(path).unwrap(), manifest);
}

#[test]
fn builtin_browser_collision_preserves_startup_and_user_management() {
    for enabled in [true, false] {
        let temp = tempfile::tempdir().unwrap();
        let state = PluginStateStore::new(temp.path());
        let manager = manager_with_state(state.clone());
        // Model a valid installation imported before this name became reserved.
        let server = serde_json::from_value(serde_json::json!({
            "name": BUILTIN_BROWSER_SERVER_NAME, "display_name": "Existing browser",
            "description": "User-owned server", "transport": {
                "type": "stdio", "command": "existing-browser", "args": [], "env": {}, "cwd": null
            }
        }))
        .unwrap();
        let previous = state.load_installs().unwrap();
        let candidate = crate::installer::manual_mcp_candidate(server).unwrap();
        let activation = crate::installer::install_plugin(
            &state,
            candidate,
            PluginInstallConflictPolicy::Reject,
            enabled,
        )
        .unwrap();
        let custom = manager.finish_install(previous, activation).unwrap();
        let before = state.load_installs().unwrap();
        let path = custom.install.installed_path.join("talos-mcp.json");
        let bytes = std::fs::read(&path).unwrap();
        let executable = temp.path().join("worker");
        for _ in 0..2 {
            assert_eq!(
                manager.provision_builtin_browser(&executable).unwrap(),
                super::builtin::BuiltinBrowserProvision::NameConflict
            );
            assert!(!manager.builtin_browser_enabled().unwrap());
            assert_eq!(state.load_installs().unwrap(), before);
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
            let listed = manager
                .list_plugins(PluginListRequest {
                    node_id: None,
                    kind: None,
                    status: None,
                    source_id: None,
                    include_disabled: true,
                })
                .unwrap();
            assert_eq!(listed.plugins.len(), 1);
        }
        manager
            .uninstall(PluginUninstallRequest {
                node_id: None,
                install_id: custom.install.id,
            })
            .unwrap();
        assert_eq!(
            manager.provision_builtin_browser(&executable).unwrap(),
            super::builtin::BuiltinBrowserProvision::Ready
        );
        assert!(manager.builtin_browser_enabled().unwrap());
    }
}
