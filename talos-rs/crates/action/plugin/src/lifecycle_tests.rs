use crate::{
    test_support::{manager_with_state, write_skill},
    PluginManager, PluginStateStore,
};
use talos_config::SkillsRuntimeConfig;
use talos_types::{
    MarketplaceSourceId, MarketplaceSourceInput, PluginDisableRequest, PluginEnableRequest,
    PluginId, PluginInstallConflictPolicy, PluginInstallRequest, PluginInstallResponse,
    PluginInstallTarget, PluginKind, PluginListRequest, PluginListStatusFilter,
    PluginSearchRefreshPolicy, PluginSearchRequest, PluginSearchStatusFilter,
    PluginSourceAddRequest, PluginStatus, PluginUninstallRequest, PluginUpdateConflictPolicy,
    PluginUpdateRequest,
};

struct PluginFixture {
    skills: std::path::PathBuf,
    state: PluginStateStore,
    manager: PluginManager,
    plugin_id: PluginId,
    source_id: MarketplaceSourceId,
}

impl PluginFixture {
    fn new(temp: &tempfile::TempDir, name: &str, contents: &str) -> Self {
        let repo = temp.path().join("repo");
        let skills = repo.join("skills");
        write_skill(&skills, name, contents);
        let state = PluginStateStore::new(temp.path().join("state"));
        let manager = manager_with_state(state.clone());
        let added = manager
            .add_source(PluginSourceAddRequest {
                node_id: None,
                source: MarketplaceSourceInput::LocalFolder {
                    path: repo,
                    sparse_paths: vec!["skills".to_string()],
                },
                refresh: true,
            })
            .expect("source should add and refresh");
        Self {
            skills,
            state,
            manager,
            plugin_id: added.plugins[0].id.clone(),
            source_id: added.source.id,
        }
    }

    fn install(&self, enable: bool) -> PluginInstallResponse {
        self.install_with_policy(enable, PluginInstallConflictPolicy::Reject)
    }

    fn install_with_policy(
        &self,
        enable: bool,
        conflict_policy: PluginInstallConflictPolicy,
    ) -> PluginInstallResponse {
        self.manager
            .install(PluginInstallRequest {
                node_id: None,
                target: PluginInstallTarget::Plugin {
                    plugin_id: self.plugin_id.clone(),
                    expected_source_commit: None,
                },
                enable: Some(enable),
                conflict_policy,
            })
            .expect("plugin should install")
    }

    fn write_source_skill(&self, name: &str, contents: &str) {
        write_skill(&self.skills, name, contents);
    }
}

#[test]
fn installs_skill_plugin_and_controls_catalog_enabled_state() {
    let temp = tempfile::tempdir().expect("temp dir should exist");
    let fixture = PluginFixture::new(
        &temp,
        "translate",
        r#"---
name: translate
description: Translate text with domain-specific rules.
short_description: Translate text
---

Use this skill to translate text.
"#,
    );

    let installed = fixture.install(true);

    assert!(matches!(
        installed.plugin.status,
        PluginStatus::Installed { enabled: true, .. }
    ));
    assert_eq!(
        installed.install.installed_path,
        temp.path().join("state").join("skills").join("translate")
    );
    assert!(installed.install.installed_path.exists());

    let roots = fixture
        .manager
        .skill_roots(&SkillsRuntimeConfig::default())
        .expect("skill roots should load");
    assert_eq!(roots.state_path, fixture.state.skills_state_path());
    let snapshot = talos_skills::refresh(&roots).expect("installed skill should load");
    assert_eq!(snapshot.skills.len(), 1);
    assert_eq!(snapshot.skills[0].name, "translate");
    assert!(snapshot.skills[0].enabled);
    assert_eq!(
        snapshot.skills[0].source_install.as_deref(),
        Some(installed.install.id.0.as_str())
    );

    let disabled = fixture
        .manager
        .disable(PluginDisableRequest {
            node_id: None,
            install_id: installed.install.id.clone(),
        })
        .expect("plugin should disable");
    assert!(matches!(
        disabled.plugin.status,
        PluginStatus::Installed { enabled: false, .. }
    ));
    let snapshot = talos_skills::refresh(
        &fixture
            .manager
            .skill_roots(&SkillsRuntimeConfig::default())
            .expect("skill roots should load"),
    )
    .expect("installed skill should load");
    assert!(snapshot.skills.is_empty());

    let install_id = installed.install.id.clone();
    let enabled = fixture
        .manager
        .enable(PluginEnableRequest {
            node_id: None,
            install_id: install_id.clone(),
        })
        .expect("plugin should enable");
    assert!(matches!(
        enabled.plugin.status,
        PluginStatus::Installed { enabled: true, .. }
    ));
    let listed = fixture
        .manager
        .list_plugins(PluginListRequest {
            node_id: None,
            kind: Some(PluginKind::Skill),
            status: Some(PluginListStatusFilter::Installed),
            source_id: Some(fixture.source_id.clone()),
            include_disabled: false,
        })
        .expect("plugins should list");
    assert_eq!(listed.plugins.len(), 1);

    let installed_search = fixture
        .manager
        .search(PluginSearchRequest {
            node_id: None,
            query: Some("translate".to_string()),
            source_id: Some(fixture.source_id.clone()),
            kind: Some(PluginKind::Skill),
            status: Some(PluginSearchStatusFilter::Installed),
            refresh: PluginSearchRefreshPolicy::CacheOnly,
            limit: Some(10),
        })
        .expect("search should merge installed status");
    assert_eq!(installed_search.plugins.len(), 1);
    assert!(matches!(
        installed_search.plugins[0].status,
        PluginStatus::Installed { enabled: true, .. }
    ));

    let not_installed_search = fixture
        .manager
        .search(PluginSearchRequest {
            node_id: None,
            query: Some("translate".to_string()),
            source_id: Some(fixture.source_id),
            kind: Some(PluginKind::Skill),
            status: Some(PluginSearchStatusFilter::NotInstalled),
            refresh: PluginSearchRefreshPolicy::CacheOnly,
            limit: Some(10),
        })
        .expect("search should filter installed plugin out");
    assert!(not_installed_search.plugins.is_empty());

    let uninstalled = fixture
        .manager
        .uninstall(PluginUninstallRequest {
            node_id: None,
            install_id,
        })
        .expect("plugin should uninstall");
    assert!(uninstalled.uninstalled);
    assert!(fixture
        .manager
        .skill_roots(&SkillsRuntimeConfig::default())
        .expect("skill roots should load")
        .roots
        .is_empty());
}

#[test]
fn installs_mcp_plugin_and_controls_runtime_capabilities() {
    let temp = tempfile::tempdir().expect("temp dir should exist");
    let repo = temp.path().join("repo");
    let mcp_dir = repo.join("mcp").join("playwright");
    std::fs::create_dir_all(&mcp_dir).expect("MCP dir should exist");
    std::fs::write(
        mcp_dir.join("talos-mcp.json"),
        r#"{
  "name": "playwright",
  "description": "Browser automation MCP server.",
  "credentials": {
    "reference": "00000000-0000-0000-0000-000000000000",
    "authentication": { "type": "bearer" }
  },
  "transport": {
    "type": "stdio",
    "command": "npx",
    "args": ["@playwright/mcp@latest"]
  }
}"#,
    )
    .expect("MCP manifest should be written");
    let state = PluginStateStore::new(temp.path().join("state"));
    let manager = manager_with_state(state);
    let added = manager
        .add_source(PluginSourceAddRequest {
            node_id: None,
            source: MarketplaceSourceInput::LocalFolder {
                path: repo,
                sparse_paths: vec!["mcp".to_string()],
            },
            refresh: true,
        })
        .expect("MCP source should add");
    let plugin = added.plugins[0].clone();

    let installed = manager
        .install(PluginInstallRequest {
            node_id: None,
            target: PluginInstallTarget::Plugin {
                plugin_id: plugin.id.clone(),
                expected_source_commit: None,
            },
            enable: Some(true),
            conflict_policy: PluginInstallConflictPolicy::Reject,
        })
        .expect("MCP plugin should install");

    assert_eq!(
        installed.install.installed_path,
        temp.path().join("state").join("mcp").join("playwright")
    );
    assert_eq!(installed.install.content_digest.file_count, 1);
    let servers = manager
        .mcp_servers()
        .expect("MCP servers should list")
        .servers;
    assert_eq!(servers.len(), 1);
    assert_eq!(servers[0].server.name, "playwright");
    assert!(servers[0].server.credentials.is_none());

    std::fs::write(
        installed.install.installed_path.join("talos-mcp.json"),
        "invalid manifest",
    )
    .unwrap();
    manager
        .update(PluginUpdateRequest {
            node_id: None,
            install_id: installed.install.id.clone(),
            expected_source_commit: None,
            conflict_policy: PluginUpdateConflictPolicy::ReplaceLocalChanges,
        })
        .expect("marketplace update should repair the installed manifest");
    assert!(manager
        .read_mcp_definition(&installed.install.id)
        .unwrap()
        .credentials
        .is_none());

    manager
        .disable(PluginDisableRequest {
            node_id: None,
            install_id: installed.install.id.clone(),
        })
        .expect("MCP plugin should disable");
    assert!(manager
        .mcp_servers()
        .expect("MCP servers should list")
        .servers
        .is_empty());

    manager
        .enable(PluginEnableRequest {
            node_id: None,
            install_id: installed.install.id,
        })
        .expect("MCP plugin should enable");
    assert_eq!(
        manager
            .mcp_servers()
            .expect("MCP servers should list")
            .servers
            .len(),
        1
    );
}

#[test]
fn update_rejects_local_changes_and_replace_updates_install() {
    let temp = tempfile::tempdir().expect("temp dir should exist");
    let fixture = PluginFixture::new(
        &temp,
        "writer",
        r#"---
name: writer
description: Write the first version.
short_description: Write text
---

First version.
"#,
    );
    let installed = fixture.install(true);

    fixture.write_source_skill(
        "writer",
        r#"---
name: writer
description: Write the second version.
short_description: Write text
---

Second version.
"#,
    );
    std::fs::write(
        installed.install.installed_path.join("SKILL.md"),
        "local change",
    )
    .expect("installed skill should be changed");

    let rejected = fixture
        .manager
        .update(PluginUpdateRequest {
            node_id: None,
            install_id: installed.install.id.clone(),
            expected_source_commit: None,
            conflict_policy: PluginUpdateConflictPolicy::RejectLocalChanges,
        })
        .expect_err("local changes should reject update");
    assert!(rejected.is_code(talos_error::ErrorCode::Conflict));

    let updated = fixture
        .manager
        .update(PluginUpdateRequest {
            node_id: None,
            install_id: installed.install.id,
            expected_source_commit: None,
            conflict_policy: PluginUpdateConflictPolicy::ReplaceLocalChanges,
        })
        .expect("local changes should be replaced");
    assert_eq!(updated.install.content_digest.file_count, 1);
    let contents = std::fs::read_to_string(updated.install.installed_path.join("SKILL.md"))
        .expect("updated skill should exist");
    assert!(contents.contains("Second version."));
}

#[test]
fn update_rolls_back_install_dir_when_installs_save_fails() {
    let temp = tempfile::tempdir().expect("temp dir should exist");
    let fixture = PluginFixture::new(
        &temp,
        "writer",
        r#"---
name: writer
description: Write safely.
---

First version.
"#,
    );
    let installed = fixture.install(true);
    let old_path = installed.install.installed_path.clone();
    let new_path = temp
        .path()
        .join("state")
        .join("skills")
        .join("writer-renamed");

    fixture.write_source_skill(
        "writer",
        r#"---
name: writer-renamed
description: Write safely after rename.
---

Second version.
"#,
    );
    fixture.state.fail_next_save_installs();
    let err = fixture
        .manager
        .update(PluginUpdateRequest {
            node_id: None,
            install_id: installed.install.id.clone(),
            expected_source_commit: None,
            conflict_policy: PluginUpdateConflictPolicy::ReplaceLocalChanges,
        })
        .expect_err("update should fail when installs state cannot save");

    assert!(err.is_internal());
    assert!(old_path.exists());
    assert!(!new_path.exists());
    let contents =
        std::fs::read_to_string(old_path.join("SKILL.md")).expect("rolled back skill should exist");
    assert!(contents.contains("First version."));
    assert!(!fixture
        .state
        .transaction_backup_dir(&installed.install.id)
        .exists());
    // An unreadable state file must fail before an install can activate a directory.
    std::fs::write(fixture.state.installs_path(), "broken installation state").unwrap();
    assert!(fixture
        .manager
        .install(PluginInstallRequest {
            node_id: None,
            target: PluginInstallTarget::Plugin {
                plugin_id: fixture.plugin_id.clone(),
                expected_source_commit: None
            },
            enable: Some(true),
            conflict_policy: PluginInstallConflictPolicy::Replace,
        })
        .is_err());
    assert!(!new_path.exists());
    assert_eq!(
        std::fs::read_to_string(old_path.join("SKILL.md")).unwrap(),
        contents
    );
}

#[test]
fn list_plugins_cleans_stale_transaction_dirs() {
    let temp = tempfile::tempdir().expect("temp dir should exist");
    let state = PluginStateStore::new(temp.path().join("state"));
    let stale_tmp = temp.path().join("state").join(".tmp").join("install_stale");
    std::fs::create_dir_all(&stale_tmp).expect("tmp dir should be created");
    let manager = manager_with_state(state);

    let listed = manager
        .list_plugins(PluginListRequest {
            node_id: None,
            kind: None,
            status: None,
            source_id: None,
            include_disabled: true,
        })
        .expect("plugins should list");

    assert!(listed.plugins.is_empty());
    assert!(!stale_tmp.exists());
}

#[test]
fn install_handles_path_conflicts_and_materialization_failures() {
    {
        let temp = tempfile::tempdir().expect("temp dir should exist");
        let repo = temp.path().join("repo");
        let skills = repo.join("skills");
        write_skill(
            &skills,
            "first",
            r#"---
name: shared-writer
description: First shared writer.
---

First version.
"#,
        );
        write_skill(
            &skills,
            "second",
            r#"---
name: shared-writer
description: Second shared writer.
---

Second version.
"#,
        );
        let manager = manager_with_state(PluginStateStore::new(temp.path().join("state")));
        let added = manager
            .add_source(PluginSourceAddRequest {
                node_id: None,
                source: MarketplaceSourceInput::LocalFolder {
                    path: repo,
                    sparse_paths: vec!["skills".to_string()],
                },
                refresh: true,
            })
            .expect("source should add");
        assert_eq!(added.plugins.len(), 2);
        let first = added
            .plugins
            .iter()
            .find(|plugin| plugin.path_in_source.as_deref() == Some("skills/first"))
            .expect("first plugin should exist")
            .clone();
        let second = added
            .plugins
            .iter()
            .find(|plugin| plugin.path_in_source.as_deref() == Some("skills/second"))
            .expect("second plugin should exist")
            .clone();

        manager
            .install(PluginInstallRequest {
                node_id: None,
                target: PluginInstallTarget::Plugin {
                    plugin_id: first.id.clone(),
                    expected_source_commit: None,
                },
                enable: Some(true),
                conflict_policy: PluginInstallConflictPolicy::Reject,
            })
            .expect("first plugin should install");
        manager
            .install(PluginInstallRequest {
                node_id: None,
                target: PluginInstallTarget::Plugin {
                    plugin_id: second.id.clone(),
                    expected_source_commit: None,
                },
                enable: Some(true),
                conflict_policy: PluginInstallConflictPolicy::Replace,
            })
            .expect("second plugin should replace final path");

        let installs = manager
            .list_plugins(PluginListRequest {
                node_id: None,
                kind: Some(PluginKind::Skill),
                status: Some(PluginListStatusFilter::Installed),
                source_id: Some(added.source.id),
                include_disabled: true,
            })
            .expect("plugins should list");
        assert_eq!(installs.plugins.len(), 1);
        assert_eq!(installs.plugins[0].id, second.id);
        let contents =
            std::fs::read_to_string(temp.path().join("state/skills/shared-writer/SKILL.md"))
                .expect("replacement skill should exist");
        assert!(contents.contains("Second version."));
    }

    {
        let temp = tempfile::tempdir().expect("temp dir should exist");
        let fixture = PluginFixture::new(
            &temp,
            "escape",
            r#"---
name: ../escape
description: Invalid install name.
---

Invalid skill.
"#,
        );
        let err = fixture
            .manager
            .install(PluginInstallRequest {
                node_id: None,
                target: PluginInstallTarget::Plugin {
                    plugin_id: fixture.plugin_id.clone(),
                    expected_source_commit: None,
                },
                enable: Some(true),
                conflict_policy: PluginInstallConflictPolicy::Reject,
            })
            .expect_err("invalid skill install name should reject");

        assert!(err.is_code(talos_error::ErrorCode::InvalidRequest));
        assert!(!temp.path().join("state").join("escape").exists());
    }

    {
        let temp = tempfile::tempdir().expect("temp dir should exist");
        let fixture = PluginFixture::new(
            &temp,
            "writer",
            r#"---
name: writer
description: Write safely.
---

First version.
"#,
        );
        let installed = fixture.install(true);
        let installed_skill_md = installed.install.installed_path.join("SKILL.md");
        assert!(installed_skill_md.exists());

        std::fs::remove_file(fixture.skills.join("writer").join("SKILL.md"))
            .expect("source skill should be broken");
        let err = fixture
            .manager
            .install(PluginInstallRequest {
                node_id: None,
                target: PluginInstallTarget::Plugin {
                    plugin_id: fixture.plugin_id.clone(),
                    expected_source_commit: None,
                },
                enable: Some(true),
                conflict_policy: PluginInstallConflictPolicy::Replace,
            })
            .expect_err("replace should fail when new source cannot materialize");

        assert!(err.is_code(talos_error::ErrorCode::ResourceNotFound));
        assert!(installed_skill_md.exists());
        let contents =
            std::fs::read_to_string(installed_skill_md).expect("existing install should remain");
        assert!(contents.contains("First version."));
    }
}

#[test]
fn uninstall_rolls_back_install_dir_when_installs_save_fails() {
    let temp = tempfile::tempdir().expect("temp dir should exist");
    let fixture = PluginFixture::new(
        &temp,
        "writer",
        r#"---
name: writer
description: Write safely.
---

Installed version.
"#,
    );
    let installed = fixture.install(true);

    fixture.state.fail_next_save_installs();
    let err = fixture
        .manager
        .uninstall(PluginUninstallRequest {
            node_id: None,
            install_id: installed.install.id.clone(),
        })
        .expect_err("uninstall should fail when installs state cannot save");

    assert!(err.is_internal());
    assert!(installed.install.installed_path.exists());
    assert!(fixture
        .state
        .load_installs()
        .expect("installs should load")
        .installs
        .iter()
        .any(|install| install.id == installed.install.id));
    assert!(!fixture
        .state
        .transaction_trash_dir(&installed.install.id)
        .exists());
}
