use crate::{PluginManager, PluginStateStore};
use talos_config::{
    PluginRootConfig as ConfigRoot, PluginRootKind as ConfigPluginRootKind, PluginSourceConfig,
    PluginsRuntimeConfig,
};
use talos_error::TalosError;
use talos_types::{
    MarketplaceSourceInput, MarketplaceSourceType, PluginDefinition, PluginDisableRequest,
    PluginEnableRequest, PluginInstallConflictPolicy, PluginInstallRequest, PluginInstallTarget,
    PluginKind, PluginListRequest, PluginListStatusFilter, PluginRootKind,
    PluginSearchRefreshPolicy, PluginSearchRequest, PluginSearchStatusFilter,
    PluginSourceAddRequest, PluginSourceListRequest, PluginSourceRefreshRequest,
    PluginSourceRemoveRequest, PluginStatus, PluginUninstallRequest, PluginUpdateConflictPolicy,
    PluginUpdateRequest, PluginWarningCode,
};

#[test]
fn lists_default_official_source_and_user_sources() {
    let temp = tempfile::tempdir().expect("temp dir should exist");
    let state = PluginStateStore::new(temp.path());
    let manager = PluginManager::with_state_store(PluginsRuntimeConfig::default(), state);

    let sources = manager
        .list_sources(PluginSourceListRequest {
            include_disabled: false,
        })
        .expect("sources should list")
        .sources;

    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].source_type, MarketplaceSourceType::Official);
    assert_eq!(sources[0].display_name, "Apilots Skills");
}

#[test]
fn corrupt_source_state_fails_fast() {
    let temp = tempfile::tempdir().expect("temp dir should exist");
    let state = PluginStateStore::new(temp.path());
    std::fs::create_dir_all(temp.path()).expect("state dir should exist");
    std::fs::write(state.sources_path(), "{not-json").expect("corrupt state should be written");
    let manager = PluginManager::with_state_store(
        PluginsRuntimeConfig {
            sources: Vec::<PluginSourceConfig>::new(),
            roots: Vec::<ConfigRoot>::new(),
        },
        state,
    );

    let err = manager
        .list_sources(talos_types::PluginSourceListRequest::default())
        .expect_err("corrupt source state should fail");

    assert!(matches!(err, TalosError::SerializationError(_)));
}

#[test]
fn configured_skill_roots_are_read_only_external_roots() {
    let manager = PluginManager::with_state_store(
        PluginsRuntimeConfig {
            sources: Vec::<PluginSourceConfig>::new(),
            roots: vec![ConfigRoot {
                kind: ConfigPluginRootKind::Skill,
                path: "/tmp/agents-skills".to_string(),
                read_only: true,
            }],
        },
        PluginStateStore::new("/tmp/talos-plugins"),
    );

    let roots = manager.configured_skill_roots();

    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0].kind, PluginRootKind::Skill);
    assert!(roots[0].read_only);
}

#[test]
fn adds_and_removes_user_git_source() {
    let temp = tempfile::tempdir().expect("temp dir should exist");
    let state = PluginStateStore::new(temp.path());
    let manager = PluginManager::with_state_store(
        PluginsRuntimeConfig {
            sources: Vec::<PluginSourceConfig>::new(),
            roots: Vec::<ConfigRoot>::new(),
        },
        state,
    );

    let added = manager
        .add_source(PluginSourceAddRequest {
            source: MarketplaceSourceInput::Git {
                url: "https://github.com/apilots/skills".to_string(),
                reference: "main".to_string(),
                sparse_paths: vec!["skills".to_string()],
            },
            refresh: false,
        })
        .expect("source should add");

    assert_eq!(added.source.source_type, MarketplaceSourceType::User);
    assert_eq!(added.source.display_name, "skills");

    let listed = manager
        .list_sources(PluginSourceListRequest::default())
        .expect("sources should list");
    assert_eq!(listed.sources.len(), 1);
    let cache_dir = temp
        .path()
        .join("cache")
        .join("marketplaces")
        .join(&added.source.id.0);
    std::fs::create_dir_all(&cache_dir).expect("cache dir should be created");

    let removed = manager
        .remove_source(PluginSourceRemoveRequest {
            source_id: added.source.id.clone(),
        })
        .expect("source should remove");

    assert!(removed.removed);
    assert!(!cache_dir.exists());
    let listed = manager
        .list_sources(PluginSourceListRequest::default())
        .expect("sources should list");
    assert!(listed.sources.is_empty());
}

#[test]
fn configured_sources_cannot_be_removed() {
    let temp = tempfile::tempdir().expect("temp dir should exist");
    let manager = PluginManager::with_state_store(
        PluginsRuntimeConfig::default(),
        PluginStateStore::new(temp.path()),
    );
    let source = manager
        .list_sources(PluginSourceListRequest::default())
        .expect("sources should list")
        .sources
        .into_iter()
        .next()
        .expect("default source should exist");

    let err = manager
        .remove_source(PluginSourceRemoveRequest {
            source_id: source.id,
        })
        .expect_err("configured source removal should fail");

    assert!(matches!(err, TalosError::ValidationError(_)));
}

#[test]
fn refresh_updates_user_source_timestamp() {
    let temp = tempfile::tempdir().expect("temp dir should exist");
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(&repo).expect("repo dir should be created");
    let manager = PluginManager::with_state_store(
        PluginsRuntimeConfig {
            sources: Vec::<PluginSourceConfig>::new(),
            roots: Vec::<ConfigRoot>::new(),
        },
        PluginStateStore::new(temp.path()),
    );
    let added = manager
        .add_source(PluginSourceAddRequest {
            source: MarketplaceSourceInput::LocalFolder {
                path: repo,
                sparse_paths: Vec::new(),
            },
            refresh: false,
        })
        .expect("source should add");

    let refreshed = manager
        .refresh_source(PluginSourceRefreshRequest {
            source_id: added.source.id,
        })
        .expect("source should refresh");

    assert!(refreshed.source.last_checked_at_ms.is_some());
    assert!(refreshed.plugins.is_empty());
}

#[test]
fn refresh_scans_local_skill_source_and_search_reads_cache() {
    let temp = tempfile::tempdir().expect("temp dir should exist");
    let repo = temp.path().join("repo");
    let skills = repo.join("skills");
    write_skill(
        &skills,
        "docx",
        r#"---
name: docx
description: Work with Word documents.
short_description: Edit Word documents
---

Use this skill for docx files.
"#,
    );
    let manager = PluginManager::with_state_store(
        PluginsRuntimeConfig {
            sources: Vec::<PluginSourceConfig>::new(),
            roots: Vec::<ConfigRoot>::new(),
        },
        PluginStateStore::new(temp.path().join("state")),
    );

    let added = manager
        .add_source(PluginSourceAddRequest {
            source: MarketplaceSourceInput::LocalFolder {
                path: repo,
                sparse_paths: vec!["skills".to_string()],
            },
            refresh: true,
        })
        .expect("source should add and refresh");

    assert_eq!(added.plugins.len(), 1);
    assert_eq!(added.plugins[0].kind, PluginKind::Skill);
    assert_eq!(
        added.plugins[0].path_in_source.as_deref(),
        Some("skills/docx")
    );
    match &added.plugins[0].definition {
        PluginDefinition::Skill(skill) => {
            assert_eq!(skill.manifest.name, "docx");
            assert_eq!(skill.skill_md_path, "skills/docx/SKILL.md");
        }
        _ => panic!("expected skill plugin"),
    }

    let search = manager
        .search(PluginSearchRequest {
            query: Some("word".to_string()),
            source_id: Some(added.source.id),
            kind: Some(PluginKind::Skill),
            status: Some(PluginSearchStatusFilter::NotInstalled),
            refresh: PluginSearchRefreshPolicy::CacheOnly,
            limit: Some(10),
        })
        .expect("search should read scan cache");

    assert_eq!(search.plugins.len(), 1);
    assert_eq!(search.plugins[0].name, "docx");
}

#[test]
fn refresh_reports_invalid_and_nested_skill_warnings() {
    let temp = tempfile::tempdir().expect("temp dir should exist");
    let repo = temp.path().join("repo");
    let skills = repo.join("skills");
    write_skill(
        &skills,
        "valid",
        r#"---
name: valid
description: A valid skill.
---

Valid skill.
"#,
    );
    write_skill(
        &skills,
        "outer",
        r#"---
name: outer
description: Outer skill.
---

Outer skill.
"#,
    );
    write_skill(
        &skills.join("outer"),
        "inner",
        r#"---
name: inner
description: Inner skill.
---

Inner skill.
"#,
    );
    let broken_dir = skills.join("broken");
    std::fs::create_dir_all(&broken_dir).expect("broken dir should be created");
    std::fs::write(
        broken_dir.join("SKILL.md"),
        r#"---
name: broken
---

Broken skill.
"#,
    )
    .expect("broken skill should be written");

    let manager = PluginManager::with_state_store(
        PluginsRuntimeConfig {
            sources: Vec::<PluginSourceConfig>::new(),
            roots: Vec::<ConfigRoot>::new(),
        },
        PluginStateStore::new(temp.path().join("state")),
    );
    let added = manager
        .add_source(PluginSourceAddRequest {
            source: MarketplaceSourceInput::LocalFolder {
                path: repo,
                sparse_paths: vec!["skills".to_string()],
            },
            refresh: true,
        })
        .expect("source should refresh");

    let mut names = added
        .plugins
        .iter()
        .map(|plugin| plugin.name.as_str())
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(names, vec!["inner", "outer", "valid"]);
    assert!(added
        .warnings
        .iter()
        .any(|warning| warning.code == PluginWarningCode::MissingRequiredField));
    assert!(added
        .warnings
        .iter()
        .any(|warning| warning.code == PluginWarningCode::NestedPlugin));
    assert!(added.warnings.iter().all(|warning| {
        warning.kind == Some(PluginKind::Skill)
            && warning.source_id.as_ref() == Some(&added.source.id)
            && warning.path.is_some()
    }));

    let search = manager
        .search(PluginSearchRequest {
            query: None,
            source_id: Some(added.source.id),
            kind: Some(PluginKind::Skill),
            status: Some(PluginSearchStatusFilter::Any),
            refresh: PluginSearchRefreshPolicy::CacheOnly,
            limit: None,
        })
        .expect("search should include cached warnings");

    assert_eq!(search.plugins.len(), 3);
    assert_eq!(search.warnings, added.warnings);
}

#[test]
fn search_refresh_policy_controls_source_refresh() {
    let temp = tempfile::tempdir().expect("temp dir should exist");
    let repo = temp.path().join("repo");
    let skills = repo.join("skills");
    write_skill(
        &skills,
        "first",
        r#"---
name: first
description: First skill.
---

First skill.
"#,
    );
    let manager = PluginManager::with_state_store(
        PluginsRuntimeConfig {
            sources: Vec::<PluginSourceConfig>::new(),
            roots: Vec::<ConfigRoot>::new(),
        },
        PluginStateStore::new(temp.path().join("state")),
    );
    let added = manager
        .add_source(PluginSourceAddRequest {
            source: MarketplaceSourceInput::LocalFolder {
                path: repo,
                sparse_paths: vec!["skills".to_string()],
            },
            refresh: true,
        })
        .expect("source should refresh");

    write_skill(
        &skills,
        "second",
        r#"---
name: second
description: Second skill.
---

Second skill.
"#,
    );

    let cache_only = manager
        .search(PluginSearchRequest {
            query: None,
            source_id: Some(added.source.id.clone()),
            kind: Some(PluginKind::Skill),
            status: Some(PluginSearchStatusFilter::Any),
            refresh: PluginSearchRefreshPolicy::CacheOnly,
            limit: None,
        })
        .expect("cache-only search should succeed");
    assert_eq!(cache_only.plugins.len(), 1);

    let refresh_stale = manager
        .search(PluginSearchRequest {
            query: None,
            source_id: Some(added.source.id.clone()),
            kind: Some(PluginKind::Skill),
            status: Some(PluginSearchStatusFilter::Any),
            refresh: PluginSearchRefreshPolicy::RefreshStale,
            limit: None,
        })
        .expect("refresh-stale search should use non-empty cache");
    assert_eq!(refresh_stale.plugins.len(), 1);

    let forced = manager
        .search(PluginSearchRequest {
            query: None,
            source_id: Some(added.source.id),
            kind: Some(PluginKind::Skill),
            status: Some(PluginSearchStatusFilter::Any),
            refresh: PluginSearchRefreshPolicy::ForceRefresh,
            limit: None,
        })
        .expect("force-refresh search should refresh source");
    assert_eq!(forced.plugins.len(), 2);
}

#[cfg(unix)]
#[test]
fn refresh_rejects_sparse_path_symlink_escape() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().expect("temp dir should exist");
    let repo = temp.path().join("repo");
    let outside = temp.path().join("outside");
    write_skill(
        &outside,
        "escaped",
        r#"---
name: escaped
description: Escaped skill.
---

Escaped skill.
"#,
    );
    std::fs::create_dir_all(&repo).expect("repo dir should be created");
    symlink(&outside, repo.join("linked")).expect("source symlink should be created");

    let manager = PluginManager::with_state_store(
        PluginsRuntimeConfig {
            sources: Vec::<PluginSourceConfig>::new(),
            roots: Vec::<ConfigRoot>::new(),
        },
        PluginStateStore::new(temp.path().join("state")),
    );
    let err = manager
        .add_source(PluginSourceAddRequest {
            source: MarketplaceSourceInput::LocalFolder {
                path: repo,
                sparse_paths: vec!["linked".to_string()],
            },
            refresh: true,
        })
        .expect_err("sparse path symlink escape should be rejected");

    assert!(matches!(err, TalosError::ValidationError(_)));
}

#[test]
fn installs_skill_plugin_and_controls_catalog_enabled_state() {
    let temp = tempfile::tempdir().expect("temp dir should exist");
    let repo = temp.path().join("repo");
    let skills = repo.join("skills");
    write_skill(
        &skills,
        "translate",
        r#"---
name: translate
description: Translate text with domain-specific rules.
short_description: Translate text
---

Use this skill to translate text.
"#,
    );
    let state = PluginStateStore::new(temp.path().join("state"));
    let manager = PluginManager::with_state_store(
        PluginsRuntimeConfig {
            sources: Vec::<PluginSourceConfig>::new(),
            roots: Vec::<ConfigRoot>::new(),
        },
        state.clone(),
    );
    let added = manager
        .add_source(PluginSourceAddRequest {
            source: MarketplaceSourceInput::LocalFolder {
                path: repo,
                sparse_paths: vec!["skills".to_string()],
            },
            refresh: true,
        })
        .expect("source should add and refresh");
    let plugin_id = added.plugins[0].id.clone();

    let installed = manager
        .install(PluginInstallRequest {
            target: PluginInstallTarget::Plugin {
                plugin_id,
                expected_source_commit: None,
            },
            enable: Some(true),
            conflict_policy: PluginInstallConflictPolicy::Reject,
        })
        .expect("plugin should install");

    assert!(matches!(
        installed.plugin.status,
        PluginStatus::Installed { enabled: true, .. }
    ));
    assert!(installed.install.installed_path.exists());

    let roots = manager
        .installed_skill_roots()
        .expect("installed roots should load");
    let snapshot = talos_skills::refresh(&talos_skills::SkillRoots {
        roots,
        state_path: state.skills_state_path(),
        max_skill_md_bytes: 64 * 1024,
        max_available_summary_bytes: 8 * 1024,
    })
    .expect("installed skill should load");
    assert_eq!(snapshot.skills.len(), 1);
    assert_eq!(snapshot.skills[0].name, "translate");
    assert!(snapshot.skills[0].enabled);
    assert_eq!(
        snapshot.skills[0].source_install.as_deref(),
        Some(installed.install.id.0.as_str())
    );

    let disabled = manager
        .disable(PluginDisableRequest {
            install_id: installed.install.id.clone(),
        })
        .expect("plugin should disable");
    assert!(matches!(
        disabled.plugin.status,
        PluginStatus::Installed { enabled: false, .. }
    ));
    let snapshot = talos_skills::refresh(&talos_skills::SkillRoots {
        roots: manager
            .installed_skill_roots()
            .expect("installed roots should load"),
        state_path: state.skills_state_path(),
        max_skill_md_bytes: 64 * 1024,
        max_available_summary_bytes: 8 * 1024,
    })
    .expect("installed skill should load");
    assert!(!snapshot.skills[0].enabled);

    let install_id = installed.install.id.clone();
    let enabled = manager
        .enable(PluginEnableRequest {
            install_id: install_id.clone(),
        })
        .expect("plugin should enable");
    assert!(matches!(
        enabled.plugin.status,
        PluginStatus::Installed { enabled: true, .. }
    ));
    let listed = manager
        .list_plugins(PluginListRequest {
            kind: Some(PluginKind::Skill),
            status: Some(PluginListStatusFilter::Installed),
            source_id: Some(added.source.id.clone()),
            include_disabled: false,
        })
        .expect("plugins should list");
    assert_eq!(listed.plugins.len(), 1);

    let installed_search = manager
        .search(PluginSearchRequest {
            query: Some("translate".to_string()),
            source_id: Some(added.source.id.clone()),
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

    let not_installed_search = manager
        .search(PluginSearchRequest {
            query: Some("translate".to_string()),
            source_id: Some(added.source.id),
            kind: Some(PluginKind::Skill),
            status: Some(PluginSearchStatusFilter::NotInstalled),
            refresh: PluginSearchRefreshPolicy::CacheOnly,
            limit: Some(10),
        })
        .expect("search should filter installed plugin out");
    assert!(not_installed_search.plugins.is_empty());

    let uninstalled = manager
        .uninstall(PluginUninstallRequest { install_id })
        .expect("plugin should uninstall");
    assert!(uninstalled.uninstalled);
    assert!(manager
        .installed_skill_roots()
        .expect("installed roots should load")
        .is_empty());
}

#[test]
fn update_rejects_local_changes_and_replace_updates_install() {
    let temp = tempfile::tempdir().expect("temp dir should exist");
    let repo = temp.path().join("repo");
    let skills = repo.join("skills");
    write_skill(
        &skills,
        "writer",
        r#"---
name: writer
description: Write the first version.
short_description: Write text
---

First version.
"#,
    );
    let manager = PluginManager::with_state_store(
        PluginsRuntimeConfig {
            sources: Vec::<PluginSourceConfig>::new(),
            roots: Vec::<ConfigRoot>::new(),
        },
        PluginStateStore::new(temp.path().join("state")),
    );
    let added = manager
        .add_source(PluginSourceAddRequest {
            source: MarketplaceSourceInput::LocalFolder {
                path: repo,
                sparse_paths: vec!["skills".to_string()],
            },
            refresh: true,
        })
        .expect("source should add and refresh");
    let installed = manager
        .install(PluginInstallRequest {
            target: PluginInstallTarget::Plugin {
                plugin_id: added.plugins[0].id.clone(),
                expected_source_commit: None,
            },
            enable: Some(true),
            conflict_policy: PluginInstallConflictPolicy::Reject,
        })
        .expect("plugin should install");

    write_skill(
        &skills,
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

    let rejected = manager
        .update(PluginUpdateRequest {
            install_id: installed.install.id.clone(),
            expected_source_commit: None,
            conflict_policy: PluginUpdateConflictPolicy::RejectLocalChanges,
        })
        .expect_err("local changes should reject update");
    assert!(matches!(rejected, TalosError::Conflict(_)));

    let updated = manager
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
    let manager = PluginManager::with_state_store(
        PluginsRuntimeConfig {
            sources: Vec::<PluginSourceConfig>::new(),
            roots: Vec::<ConfigRoot>::new(),
        },
        state,
    );

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
    let manager = PluginManager::with_state_store(
        PluginsRuntimeConfig {
            sources: Vec::<PluginSourceConfig>::new(),
            roots: Vec::<ConfigRoot>::new(),
        },
        state,
    );

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

fn write_skill(root: &std::path::Path, name: &str, contents: &str) {
    let skill_dir = root.join(name);
    std::fs::create_dir_all(&skill_dir).expect("skill dir should be created");
    std::fs::write(skill_dir.join("SKILL.md"), contents).expect("skill file should be written");
}
