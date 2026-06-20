use crate::{
    test_support::{manager_with_state, write_skill},
    PluginManager, PluginStateStore,
};
use talos_config::SkillsRuntimeConfig;
use talos_error::TalosError;
use talos_types::{
    MarketplaceSourceId, MarketplaceSourceInput, PluginDisableRequest, PluginEnableRequest,
    PluginId, PluginInstallConflictPolicy, PluginInstallRequest, PluginInstallResponse,
    PluginInstallTarget, PluginKind, PluginListRequest, PluginListStatusFilter,
    PluginSearchRefreshPolicy, PluginSearchRequest, PluginSearchStatusFilter,
    PluginSourceAddRequest, PluginStatus, PluginUninstallRequest, PluginUpdateConflictPolicy,
    PluginUpdateRequest, PluginWarningCode,
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
    assert!(!snapshot.skills[0].enabled);

    let install_id = installed.install.id.clone();
    let enabled = fixture
        .manager
        .enable(PluginEnableRequest {
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
        .uninstall(PluginUninstallRequest { install_id })
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
            target: PluginInstallTarget::Plugin {
                plugin_id: plugin.id.clone(),
                expected_source_commit: None,
            },
            enable: Some(true),
            conflict_policy: PluginInstallConflictPolicy::Reject,
        })
        .expect("MCP plugin should install");

    assert!(installed.install.exposed_skill_root.is_none());
    assert_eq!(installed.install.exposed_mcp_servers.len(), 1);
    let servers = manager.mcp_servers().expect("MCP servers should list");
    assert_eq!(servers.len(), 1);
    assert_eq!(servers[0].server.name, "playwright");

    manager
        .disable(PluginDisableRequest {
            install_id: installed.install.id.clone(),
        })
        .expect("MCP plugin should disable");
    assert!(manager
        .mcp_servers()
        .expect("MCP servers should list")
        .is_empty());

    manager
        .enable(PluginEnableRequest {
            install_id: installed.install.id,
        })
        .expect("MCP plugin should enable");
    assert_eq!(
        manager
            .mcp_servers()
            .expect("MCP servers should list")
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
        installed
            .install
            .installed_path
            .join("skills")
            .join("writer")
            .join("SKILL.md"),
        "local change",
    )
    .expect("installed skill should be changed");

    let rejected = fixture
        .manager
        .update(PluginUpdateRequest {
            install_id: installed.install.id.clone(),
            expected_source_commit: None,
            conflict_policy: PluginUpdateConflictPolicy::RejectLocalChanges,
        })
        .expect_err("local changes should reject update");
    assert!(matches!(rejected, TalosError::Conflict(_)));

    let updated = fixture
        .manager
        .update(PluginUpdateRequest {
            install_id: installed.install.id,
            expected_source_commit: None,
            conflict_policy: PluginUpdateConflictPolicy::ReplaceLocalChanges,
        })
        .expect("local changes should be replaced");
    assert_eq!(updated.install.file_manifest.files.len(), 1);
    let contents = std::fs::read_to_string(
        updated
            .install
            .installed_path
            .join("skills")
            .join("writer")
            .join("SKILL.md"),
    )
    .expect("updated skill should exist");
    assert!(contents.contains("Second version."));
}

#[test]
fn list_plugins_cleans_stale_transaction_dirs() {
    let temp = tempfile::tempdir().expect("temp dir should exist");
    let state = PluginStateStore::new(temp.path().join("state"));
    let stale_tmp = temp
        .path()
        .join("state")
        .join("installs")
        .join(".tmp-install_stale");
    let stale_backup = temp
        .path()
        .join("state")
        .join("installs")
        .join(".backup-install_stale");
    std::fs::create_dir_all(&stale_tmp).expect("tmp dir should be created");
    std::fs::create_dir_all(&stale_backup).expect("backup dir should be created");
    let manager = manager_with_state(state);

    let listed = manager
        .list_plugins(PluginListRequest {
            kind: None,
            status: None,
            source_id: None,
            include_disabled: true,
        })
        .expect("plugins should list");

    assert!(listed.plugins.is_empty());
    assert!(!stale_tmp.exists());
    assert!(!stale_backup.exists());
}

#[test]
fn list_plugins_reports_orphan_install_dirs() {
    let temp = tempfile::tempdir().expect("temp dir should exist");
    let state = PluginStateStore::new(temp.path().join("state"));
    let orphan = state.installs_dir().join("install_orphan");
    std::fs::create_dir_all(&orphan).expect("orphan dir should be created");
    let manager = manager_with_state(state);

    let listed = manager
        .list_plugins(PluginListRequest {
            kind: None,
            status: None,
            source_id: None,
            include_disabled: true,
        })
        .expect("plugins should list");

    assert!(listed.plugins.is_empty());
    assert_eq!(listed.warnings.len(), 1);
    assert_eq!(
        listed.warnings[0].code,
        PluginWarningCode::OrphanInstallDirectory
    );
    assert!(orphan.exists());
}

#[test]
fn install_replace_keeps_existing_install_when_materialize_fails() {
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
    let installed_skill_md = installed
        .install
        .installed_path
        .join("skills")
        .join("writer")
        .join("SKILL.md");
    assert!(installed_skill_md.exists());

    std::fs::remove_file(fixture.skills.join("writer").join("SKILL.md"))
        .expect("source skill should be broken");
    let err = fixture
        .manager
        .install(PluginInstallRequest {
            target: PluginInstallTarget::Plugin {
                plugin_id: fixture.plugin_id.clone(),
                expected_source_commit: None,
            },
            enable: Some(true),
            conflict_policy: PluginInstallConflictPolicy::Replace,
        })
        .expect_err("replace should fail when new source cannot materialize");

    assert!(matches!(err, TalosError::ResourceNotFound(_)));
    assert!(installed_skill_md.exists());
    let contents =
        std::fs::read_to_string(installed_skill_md).expect("existing install should remain");
    assert!(contents.contains("First version."));
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
    let installed_skill_md = installed
        .install
        .installed_path
        .join("skills")
        .join("writer")
        .join("SKILL.md");

    fixture.write_source_skill(
        "writer",
        r#"---
name: writer
description: Write safely.
---

Second version.
"#,
    );
    fixture.state.fail_next_save_installs();
    let err = fixture
        .manager
        .update(PluginUpdateRequest {
            install_id: installed.install.id.clone(),
            expected_source_commit: None,
            conflict_policy: PluginUpdateConflictPolicy::ReplaceLocalChanges,
        })
        .expect_err("update should fail when installs state cannot save");

    assert!(matches!(err, TalosError::StorageError(_)));
    let contents =
        std::fs::read_to_string(&installed_skill_md).expect("rolled back skill should exist");
    assert!(contents.contains("First version."));
    assert!(!fixture
        .state
        .install_backup_dir(&installed.install.id)
        .exists());
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
            install_id: installed.install.id.clone(),
        })
        .expect_err("uninstall should fail when installs state cannot save");

    assert!(matches!(err, TalosError::StorageError(_)));
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
        .install_trash_dir(&installed.install.id)
        .exists());
}

#[test]
fn cleanup_deletes_uninstall_trash_when_install_record_was_removed() {
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
    let mut installs = fixture.state.load_installs().expect("installs should load");
    installs
        .installs
        .retain(|install| install.id != installed.install.id);
    fixture
        .state
        .save_installs(&installs)
        .expect("removed install record should save");
    let trash_dir = fixture.state.install_trash_dir(&installed.install.id);
    std::fs::rename(&installed.install.installed_path, &trash_dir)
        .expect("install dir should be staged as trash");

    let listed = fixture
        .manager
        .list_plugins(PluginListRequest {
            kind: None,
            status: None,
            source_id: None,
            include_disabled: true,
        })
        .expect("list should clean crash remnant");

    assert!(listed.plugins.is_empty());
    assert!(!trash_dir.exists());
    assert!(!installed.install.installed_path.exists());
}

#[test]
fn cleanup_restores_backup_when_install_dir_does_not_match_state() {
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
    let backup_dir = fixture.state.install_backup_dir(&installed.install.id);
    std::fs::rename(&installed.install.installed_path, &backup_dir)
        .expect("old install should be staged as backup");
    let current_skill = installed
        .install
        .installed_path
        .join("skills")
        .join("writer");
    std::fs::create_dir_all(&current_skill).expect("new install skill dir should exist");
    std::fs::write(
        current_skill.join("SKILL.md"),
        "---\nname: writer\ndescription: Write safely.\n---\n\nSecond version.\n",
    )
    .expect("new unmatched install should exist");

    fixture
        .manager
        .list_plugins(PluginListRequest {
            kind: None,
            status: None,
            source_id: None,
            include_disabled: true,
        })
        .expect("list should clean crash remnant");

    let contents = std::fs::read_to_string(
        installed
            .install
            .installed_path
            .join("skills")
            .join("writer")
            .join("SKILL.md"),
    )
    .expect("restored install should exist");
    assert!(contents.contains("First version."));
    assert!(!backup_dir.exists());
}

#[test]
fn cleanup_deletes_backup_when_install_dir_matches_state() {
    let temp = tempfile::tempdir().expect("temp dir should exist");
    let fixture = PluginFixture::new(
        &temp,
        "writer",
        r#"---
name: writer
description: Write safely.
---

Current version.
"#,
    );
    let installed = fixture.install(true);
    let backup_dir = fixture.state.install_backup_dir(&installed.install.id);
    let backup_skill = backup_dir.join("skills").join("writer");
    std::fs::create_dir_all(&backup_skill).expect("backup skill dir should exist");
    std::fs::write(backup_skill.join("SKILL.md"), "stale backup")
        .expect("backup file should exist");

    fixture
        .manager
        .list_plugins(PluginListRequest {
            kind: None,
            status: None,
            source_id: None,
            include_disabled: true,
        })
        .expect("list should clean crash remnant");

    assert!(!backup_dir.exists());
    assert!(installed.install.installed_path.exists());
}

#[test]
fn uninstall_rolls_back_installs_record_when_skill_state_save_fails() {
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
    let installed = fixture.install(false);

    fixture.state.fail_next_save_skill_state();
    let err = fixture
        .manager
        .uninstall(PluginUninstallRequest {
            install_id: installed.install.id.clone(),
        })
        .expect_err("uninstall should fail when skill state cannot save");

    assert!(matches!(err, TalosError::StorageError(_)));
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
        .install_trash_dir(&installed.install.id)
        .exists());
}
