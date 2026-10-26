use crate::PluginStateStore;
fn manager_with_state(state: PluginStateStore) -> crate::PluginManager {
    crate::PluginManager::with_state_store(
        talos_config::PluginsRuntimeConfig {
            sources: vec![talos_config::official_mcp_source()],
            roots: vec![],
        },
        state,
    )
}
use talos_types::*;

fn snapshot(root: &std::path::Path) -> std::path::PathBuf {
    let snapshot = root.join("official");
    let browser = snapshot.join("mcp/browser");
    std::fs::create_dir_all(&browser).unwrap();
    std::fs::write(snapshot.join("source.json"), serde_json::to_vec(&serde_json::json!({
        "repository":"https://github.com/apilots/mcps", "commit":"0123456789abcdef0123456789abcdef01234567"
    })).unwrap()).unwrap();
    std::fs::write(browser.join("talos-mcp.json"), serde_json::to_vec(&serde_json::json!({
        "name":"talos_browser", "default_install":true,
        "browser":{"api_version":1,"docker_requirements":"requirements.lock"},
        "transport":{"type":"stdio","command":"talos","args":["--browser-mcp"],"env":{},"cwd":null}
    })).unwrap()).unwrap();
    std::fs::write(browser.join("requirements.lock"), "browser-use==0.13.11\n").unwrap();
    snapshot
}

#[test]
fn official_browser_identity_follows_the_existing_source_and_path_rules() {
    let source = talos_config::official_mcp_source();
    let id = crate::source::source_id_for(&source.url, &source.reference, &source.sparse_paths);
    assert_eq!(
        crate::source::plugin_id_for(&id, PluginKind::McpServer, "mcp/browser").0,
        OFFICIAL_BROWSER_PLUGIN_ID
    );
}

#[test]
fn snapshot_provisions_one_marketplace_install_and_preserves_disable() {
    let temporary = tempfile::tempdir().unwrap();
    let state = PluginStateStore::new(temporary.path().join("state"));
    let manager = manager_with_state(state.clone());
    let snapshot = snapshot(temporary.path());
    assert_eq!(
        manager.provision_official_browser(Some(&snapshot)).unwrap(),
        super::official::OfficialBrowserProvision::Ready
    );
    let first = state.load_installs().unwrap();
    assert_eq!(first.installs.len(), 1);
    let install = &first.installs[0];
    assert!(
        matches!(&install.source, PluginInstallSource::Marketplace {url, path_in_source,..}
        if url == "https://github.com/apilots/mcps" && path_in_source == "mcp/browser")
    );
    assert!(manager.official_browser_enabled().unwrap());
    let servers = manager.mcp_servers().unwrap().servers;
    assert_eq!(servers.len(), 1);
    assert!(
        matches!(&servers[0].server.transport, McpTransportDefinition::Stdio {command,..}
        if std::path::Path::new(command) == std::env::current_exe().unwrap())
    );
    let stored: serde_json::Value =
        talos_fs::read_json(&install.installed_path.join("talos-mcp.json")).unwrap();
    assert_eq!(stored["transport"]["command"], "talos");
    assert_eq!(stored["browser"]["api_version"], 1);
    manager
        .disable(PluginDisableRequest {
            node_id: None,
            install_id: install.id.clone(),
        })
        .unwrap();
    let disabled = state.load_installs().unwrap();
    manager.provision_official_browser(Some(&snapshot)).unwrap();
    assert_eq!(state.load_installs().unwrap(), disabled);
    assert!(!manager.official_browser_enabled().unwrap());
    assert!(manager
        .uninstall(PluginUninstallRequest {
            node_id: None,
            install_id: install.id.clone()
        })
        .is_err());
}

#[test]
fn tampered_browser_content_cannot_receive_the_host_binding() {
    let temporary = tempfile::tempdir().unwrap();
    let state = PluginStateStore::new(temporary.path().join("state"));
    let manager = manager_with_state(state.clone());
    manager
        .provision_official_browser(Some(&snapshot(temporary.path())))
        .unwrap();
    let install = state.load_installs().unwrap().installs.remove(0);
    assert_eq!(
        manager
            .installed_browser_requirements()
            .unwrap()
            .map(|(_, lock)| lock),
        Some(std::fs::canonicalize(install.installed_path.join("requirements.lock")).unwrap())
    );
    std::fs::write(
        install.installed_path.join("requirements.lock"),
        "changed\n",
    )
    .unwrap();
    let discovery = manager.mcp_servers().unwrap();
    assert!(discovery.servers.is_empty());
    assert!(!discovery.warnings.is_empty());
    assert!(manager.installed_browser_requirements().is_err());
}

#[test]
fn missing_snapshot_does_not_invent_an_install_or_source_revision() {
    let temporary = tempfile::tempdir().unwrap();
    let state = PluginStateStore::new(temporary.path().join("state"));
    let manager = manager_with_state(state.clone());
    assert_eq!(
        manager.provision_official_browser(None).unwrap(),
        super::official::OfficialBrowserProvision::Unavailable
    );
    assert!(state.load_installs().unwrap().installs.is_empty());
}

#[test]
fn source_metadata_is_verified_before_installing_a_snapshot() {
    let temporary = tempfile::tempdir().unwrap();
    let state = PluginStateStore::new(temporary.path().join("state"));
    let manager = manager_with_state(state.clone());
    let snapshot = snapshot(temporary.path());
    std::fs::write(snapshot.join("source.json"), r#"{"repository":"https://example.com/mcps","commit":"0123456789abcdef0123456789abcdef01234567"}"#).unwrap();
    assert!(manager.provision_official_browser(Some(&snapshot)).is_err());
    assert!(state.load_installs().unwrap().installs.is_empty());
}

#[test]
fn packaged_snapshot_replaces_the_cache_without_mixing_old_files() {
    let temporary = tempfile::tempdir().unwrap();
    let state = PluginStateStore::new(temporary.path().join("state"));
    let manager = manager_with_state(state.clone());
    let source = talos_config::official_mcp_source();
    let id = crate::source::source_id_for(&source.url, &source.reference, &source.sparse_paths);
    let stale = state
        .marketplace_cache_dir(&id)
        .join("repo/mcp/browser/stale.txt");
    std::fs::create_dir_all(stale.parent().unwrap()).unwrap();
    std::fs::write(&stale, "from another commit").unwrap();
    manager
        .provision_official_browser(Some(&snapshot(temporary.path())))
        .unwrap();
    assert!(!stale.exists());
    let installed = state.load_installs().unwrap().installs.remove(0);
    assert!(!installed.installed_path.join("stale.txt").exists());
    assert_eq!(
        state.load_source_scan_cache(&id).unwrap().source_commit,
        Some("0123456789abcdef0123456789abcdef01234567".into())
    );
}
