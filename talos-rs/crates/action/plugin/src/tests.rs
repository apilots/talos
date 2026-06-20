use crate::{
    test_support::{manager_with_state, write_skill, LocalSkillSourceFixture},
    PluginManager, PluginStateStore,
};
use talos_config::{PluginSourceConfig, PluginSourceKind, PluginSourceType, PluginsRuntimeConfig};
use talos_error::TalosError;
use talos_types::{
    MarketplaceSourceInput, MarketplaceSourceType, PluginDefinition, PluginInstallConflictPolicy,
    PluginInstallRequest, PluginInstallTarget, PluginKind, PluginReadRequest,
    PluginSearchRefreshPolicy, PluginSearchRequest, PluginSearchStatusFilter,
    PluginSourceAddRequest, PluginSourceListRequest, PluginSourceRefreshRequest,
    PluginSourceRemoveRequest, PluginWarningCode,
};

#[test]
fn lists_default_official_source_and_user_sources() {
    let temp = tempfile::tempdir().expect("temp dir should exist");
    let state = PluginStateStore::new(temp.path());
    let manager = PluginManager::with_state_store(PluginsRuntimeConfig::default(), state);

    let sources = manager
        .list_sources(PluginSourceListRequest {})
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
    let manager = manager_with_state(state);

    let err = manager
        .list_sources(talos_types::PluginSourceListRequest::default())
        .expect_err("corrupt source state should fail");

    assert!(matches!(err, TalosError::SerializationError(_)));
}

#[test]
fn adds_and_removes_user_git_source() {
    let temp = tempfile::tempdir().expect("temp dir should exist");
    let state = PluginStateStore::new(temp.path());
    let manager = manager_with_state(state);

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
    let manager = manager_with_state(PluginStateStore::new(temp.path()));
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
    let fixture = LocalSkillSourceFixture::new(&temp);
    fixture.write_skill(
        "docx",
        r#"---
name: docx
description: Work with Word documents.
short_description: Edit Word documents
---

Use this skill for docx files.
"#,
    );
    let added = fixture.add_source(true);

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

    let search = fixture
        .manager
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
fn reads_marketplace_and_installed_skill_document() {
    let temp = tempfile::tempdir().expect("temp dir should exist");
    let fixture = LocalSkillSourceFixture::new(&temp);
    fixture.write_skill(
        "manual-reviewer",
        r#"---
name: manual-reviewer
display_name: Manual Reviewer
description: Review a manual test marker.
---

# Manual Reviewer

Expected marker: plugin-read-visible
"#,
    );
    let added = fixture.add_source(true);
    let plugin = added.plugins[0].clone();

    let marketplace_doc = fixture
        .manager
        .read(PluginReadRequest {
            plugin_id: plugin.id.clone(),
        })
        .expect("marketplace plugin should read");
    assert!(marketplace_doc
        .contents
        .contains("Expected marker: plugin-read-visible"));
    assert!(matches!(
        marketplace_doc.plugin.status,
        talos_types::PluginStatus::NotInstalled
    ));

    fixture
        .manager
        .install(PluginInstallRequest {
            target: PluginInstallTarget::Plugin {
                plugin_id: plugin.id.clone(),
                expected_source_commit: plugin.source_commit.clone(),
            },
            enable: Some(true),
            conflict_policy: PluginInstallConflictPolicy::Reject,
        })
        .expect("plugin should install");
    let installed_doc = fixture
        .manager
        .read(PluginReadRequest {
            plugin_id: plugin.id.clone(),
        })
        .expect("installed plugin should read");

    assert!(installed_doc
        .contents
        .contains("Expected marker: plugin-read-visible"));
    assert!(matches!(
        installed_doc.plugin.status,
        talos_types::PluginStatus::Installed { .. }
    ));
}

#[test]
fn read_rejects_skill_document_above_server_limit() {
    let temp = tempfile::tempdir().expect("temp dir should exist");
    let fixture = LocalSkillSourceFixture::new(&temp);
    fixture.write_skill(
        "manual-reviewer",
        r#"---
name: manual-reviewer
description: Review a manual test marker.
---

This document is intentionally longer than the test limit.
"#,
    );
    let manager = manager_with_state(fixture.state.clone()).with_max_skill_md_bytes(32);
    let added = manager
        .add_source(PluginSourceAddRequest {
            source: MarketplaceSourceInput::LocalFolder {
                path: fixture.repo.clone(),
                sparse_paths: vec!["skills".to_string()],
            },
            refresh: true,
        })
        .expect("source should add");

    let err = manager
        .read(PluginReadRequest {
            plugin_id: added.plugins[0].id.clone(),
        })
        .expect_err("oversized skill document should be rejected");

    assert!(matches!(err, TalosError::ValidationError(_)));
}

#[test]
fn refresh_reports_invalid_and_nested_skill_warnings() {
    let temp = tempfile::tempdir().expect("temp dir should exist");
    let fixture = LocalSkillSourceFixture::new(&temp);
    fixture.write_skill(
        "valid",
        r#"---
name: valid
description: A valid skill.
---

Valid skill.
"#,
    );
    fixture.write_skill(
        "outer",
        r#"---
name: outer
description: Outer skill.
---

Outer skill.
"#,
    );
    write_skill(
        &fixture.skills.join("outer"),
        "inner",
        r#"---
name: inner
description: Inner skill.
---

Inner skill.
"#,
    );
    let broken_dir = fixture.skills.join("broken");
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

    let added = fixture.add_source(true);

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

    let search = fixture
        .manager
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
    let fixture = LocalSkillSourceFixture::new(&temp);
    fixture.write_skill(
        "first",
        r#"---
name: first
description: First skill.
---

First skill.
"#,
    );
    let added = fixture.add_source(true);

    fixture.write_skill(
        "second",
        r#"---
name: second
description: Second skill.
---

Second skill.
"#,
    );

    let cache_only = fixture
        .manager
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

    let refresh_stale = fixture
        .manager
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

    let forced = fixture
        .manager
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

#[test]
fn search_returns_cached_results_when_another_source_refresh_fails() {
    let temp = tempfile::tempdir().expect("temp dir should exist");
    let repo = temp.path().join("repo");
    let skills = repo.join("skills");
    write_skill(
        &skills,
        "manual",
        r#"---
name: manual
description: Manual local marketplace skill.
---

Manual body.
"#,
    );
    let manager = PluginManager::with_state_store(
        PluginsRuntimeConfig {
            sources: vec![PluginSourceConfig {
                source_type: PluginSourceType::Official,
                display_name: "Broken Source".to_string(),
                kind: PluginSourceKind::Git,
                url: "file:///tmp/talos-missing-plugin-source.git".to_string(),
                reference: "main".to_string(),
                sparse_paths: vec!["skills".to_string()],
            }],
            roots: Vec::new(),
        },
        PluginStateStore::new(temp.path().join("state")),
    );
    manager
        .add_source(PluginSourceAddRequest {
            source: MarketplaceSourceInput::LocalFolder {
                path: repo,
                sparse_paths: vec!["skills".to_string()],
            },
            refresh: true,
        })
        .expect("local source should add and refresh");

    let search = manager
        .search(PluginSearchRequest {
            query: Some("manual".to_string()),
            source_id: None,
            kind: Some(PluginKind::Skill),
            status: Some(PluginSearchStatusFilter::NotInstalled),
            refresh: PluginSearchRefreshPolicy::RefreshStale,
            limit: Some(10),
        })
        .expect("search should continue after one source refresh failure");

    assert_eq!(search.plugins.len(), 1);
    assert_eq!(search.plugins[0].name, "manual");
    assert!(search
        .warnings
        .iter()
        .any(|warning| warning.code == PluginWarningCode::SourceRefreshFailed));
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

    let manager = manager_with_state(PluginStateStore::new(temp.path().join("state")));
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
