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

fn seed_legacy_browser(state: &PluginStateStore, enabled: bool) -> PluginInstall {
    let mut candidate = crate::installer::manual_mcp_candidate(McpServerDefinition {
        name: BROWSER_MCP_SERVER_NAME.into(),
        display_name: Some("Browser".into()),
        description: None,
        transport: McpTransportDefinition::Stdio {
            command: "/old/talos".into(),
            args: vec!["--browser-mcp".into()],
            env: Default::default(),
            cwd: None,
        },
        credentials: None,
    })
    .unwrap();
    candidate.plugin.id = PluginId("builtin_browser".into());
    let install = crate::installer::install_plugin(
        state,
        candidate,
        PluginInstallConflictPolicy::Reject,
        enabled,
    )
    .unwrap()
    .install;
    assert_eq!(install.id.0, "install_5477e7af15e95fb7d303a4df");
    let mut registry = state.load_installs().unwrap();
    registry.installs.push(install.clone());
    state.save_installs(&registry).unwrap();
    install
}

#[test]
fn snapshot_migrates_legacy_browser_with_its_saved_preference_once() {
    for enabled in [true, false] {
        let temporary = tempfile::tempdir().unwrap();
        let state = PluginStateStore::new(temporary.path().join("state"));
        let legacy = seed_legacy_browser(&state, enabled);
        let manager = manager_with_state(state.clone());
        let snapshot = snapshot(temporary.path());

        assert_eq!(
            manager.provision_official_browser(Some(&snapshot)).unwrap(),
            super::official::OfficialBrowserProvision::Ready
        );
        let migrated = state.load_installs().unwrap();
        assert_eq!(migrated.installs.len(), 1);
        let official = &migrated.installs[0];
        assert_ne!(official.id, legacy.id);
        assert_eq!(official.installed_path, legacy.installed_path);
        assert!(super::official::is_official_browser(official));
        assert_eq!(manager.official_browser_enabled().unwrap(), enabled);
        let servers = manager.mcp_servers().unwrap().servers;
        if enabled {
            assert_eq!(servers.len(), 1);
            assert_eq!(
                servers[0].source,
                McpServerSource::Plugin {
                    plugin_id: official.plugin_id.clone(),
                    install_id: official.id.clone(),
                }
            );
            assert!(
                matches!(&servers[0].server.transport, McpTransportDefinition::Stdio { command, .. }
                if std::path::Path::new(command) == std::env::current_exe().unwrap())
            );
        } else {
            assert!(servers.is_empty());
        }

        manager.provision_official_browser(Some(&snapshot)).unwrap();
        assert_eq!(state.load_installs().unwrap(), migrated);
    }
}

#[test]
fn failed_replacement_recovers_the_legacy_directory_after_an_interrupted_upgrade() {
    let temporary = tempfile::tempdir().unwrap();
    let state = PluginStateStore::new(temporary.path().join("state"));
    let legacy = seed_legacy_browser(&state, /*enabled*/ false);
    let before = state.load_installs().unwrap();
    let manager = manager_with_state(state.clone());
    let backup = state.transaction_backup_dir(&crate::installer::install_id_for(&PluginId(
        OFFICIAL_BROWSER_PLUGIN_ID.into(),
    )));
    std::fs::create_dir_all(backup.parent().unwrap()).unwrap();
    std::fs::rename(&legacy.installed_path, &backup).unwrap();
    std::fs::create_dir(&legacy.installed_path).unwrap();

    let snapshot = snapshot(temporary.path());
    let lock = snapshot.join("mcp/browser/requirements.lock");
    std::fs::remove_file(&lock).unwrap();
    assert!(manager.provision_official_browser(Some(&snapshot)).is_err());
    assert_eq!(state.load_installs().unwrap(), before);
    assert_eq!(
        crate::installer::content_digest_for_dir(&legacy.installed_path).unwrap(),
        legacy.content_digest
    );
    assert!(!backup.exists());

    std::fs::write(lock, "browser-use==0.13.11\n").unwrap();
    assert_eq!(
        manager.provision_official_browser(Some(&snapshot)).unwrap(),
        super::official::OfficialBrowserProvision::Ready
    );
    assert!(!manager.official_browser_enabled().unwrap());
}

#[test]
fn missing_source_preserves_legacy_state_without_publishing_its_mcp() {
    let temporary = tempfile::tempdir().unwrap();
    let state = PluginStateStore::new(temporary.path().join("state"));
    let legacy = seed_legacy_browser(&state, /*enabled*/ true);
    let before = state.load_installs().unwrap();
    let manager = manager_with_state(state.clone());

    assert_eq!(
        manager.provision_official_browser(None).unwrap(),
        super::official::OfficialBrowserProvision::Unavailable
    );
    assert!(manager.mcp_servers().unwrap().servers.is_empty());
    assert_eq!(state.load_installs().unwrap(), before);
    assert_eq!(
        crate::installer::content_digest_for_dir(&legacy.installed_path).unwrap(),
        legacy.content_digest
    );
}

#[test]
fn cached_default_replaces_only_the_exact_legacy_identity() {
    let temporary = tempfile::tempdir().unwrap();
    let state = PluginStateStore::new(temporary.path().join("state"));
    let legacy = seed_legacy_browser(&state, /*enabled*/ false);
    let original = state.load_installs().unwrap();
    let mut conflict = original.clone();
    conflict.installs[0].id = PluginInstallId("unrelated_install".into());
    state.save_installs(&conflict).unwrap();
    let manager = manager_with_state(state.clone());

    assert_eq!(
        manager
            .provision_official_browser(Some(&snapshot(temporary.path())))
            .unwrap(),
        super::official::OfficialBrowserProvision::NameConflict
    );
    assert!(manager.default_browser_install_request().unwrap().is_none());
    assert_eq!(state.load_installs().unwrap(), conflict);
    assert_eq!(
        crate::installer::content_digest_for_dir(&legacy.installed_path).unwrap(),
        legacy.content_digest
    );

    state.save_installs(&original).unwrap();
    let request = manager.default_browser_install_request().unwrap().unwrap();
    let prepared = manager.prepare_install(&request).unwrap();
    let migrated = manager.install_prepared(request, prepared).unwrap().install;
    assert_eq!(
        state.load_installs().unwrap().installs,
        vec![migrated.clone()]
    );
    assert!(super::official::is_official_browser(&migrated));
    assert!(!migrated.enabled);
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
