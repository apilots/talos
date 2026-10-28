use super::*;
use crate::{
    state::ScanCacheFailure,
    test_support::{manager_with_state, write_skill},
    tests_git::run_git,
};
use talos_types::*;

fn fixture(root: &std::path::Path) -> (PluginManager, std::path::PathBuf, MarketplaceSource) {
    let repo = root.join("git");
    std::fs::create_dir_all(&repo).unwrap();
    run_git(&repo, ["init", "-b", "main"]);
    run_git(&repo, ["config", "user.email", "test@example.test"]);
    run_git(&repo, ["config", "user.name", "Test"]);
    version(&repo, "one");
    let manager = manager_with_state(PluginStateStore::new(root.join("state")));
    let source = manager
        .add_source(PluginSourceAddRequest {
            node_id: None,
            source: MarketplaceSourceInput::Git {
                url: format!("file://{}", repo.display()),
                reference: "main".into(),
                sparse_paths: vec!["skills".into()],
            },
            refresh: true,
        })
        .unwrap()
        .source;
    (manager, repo, source)
}
fn version(repo: &std::path::Path, value: &str) {
    write_skill(
        &repo.join("skills"),
        "writer",
        &format!("---\nname: writer\ndescription: {value}\n---\n{value}\n"),
    );
    run_git(repo, ["add", "."]);
    run_git(repo, ["commit", "-m", value]);
}
fn prepare(
    manager: &PluginManager,
    source: &MarketplaceSource,
) -> (
    StagingDirectory,
    crate::state::SourceScanCacheFile,
    crate::scanner::SourceScanResult,
) {
    let previous = manager.state.load_source_scan_cache(&source.id).unwrap();
    let dir = StagingDirectory::new(&manager.state).unwrap();
    let staging = PluginStateStore::new(dir.path().join("source"));
    let scan = crate::scanner::scan_source(&staging, source).unwrap();
    (dir, previous, scan)
}
fn installed_text(manager: &PluginManager, source: &MarketplaceSource) -> String {
    let cache = manager.state.load_source_scan_cache(&source.id).unwrap();
    let install = manager
        .install(PluginInstallRequest {
            node_id: None,
            target: PluginInstallTarget::Plugin {
                plugin_id: cache.plugins[0].id.clone(),
                expected_source_commit: cache.source_commit,
            },
            enable: Some(true),
            conflict_policy: PluginInstallConflictPolicy::Replace,
        })
        .unwrap();
    std::fs::read_to_string(install.install.installed_path.join("SKILL.md")).unwrap()
}
#[test]
fn competing_git_publications_keep_one_complete_commit() {
    let root = tempfile::tempdir().unwrap();
    let (manager, repo, source) = fixture(root.path());
    version(&repo, "two");
    let (first, old, scan) = prepare(&manager, &source);
    let (second, same, other) = prepare(&manager, &source);
    let commit = scan.source_commit.clone();
    manager
        .publish_source_scan(source.clone(), old, &first, scan)
        .unwrap();
    assert_eq!(
        manager
            .publish_source_scan(source.clone(), same, &second, other)
            .unwrap_err()
            .error_code(),
        talos_error::ErrorCode::Conflict
    );
    assert_eq!(
        manager
            .state
            .load_source_scan_cache(&source.id)
            .unwrap()
            .source_commit,
        commit
    );
    assert!(installed_text(&manager, &source).ends_with("two\n"));
}
#[test]
fn failed_git_cache_index_write_never_replaces_the_published_tree() {
    for failure in [ScanCacheFailure::BeforeWrite, ScanCacheFailure::AfterWrite] {
        let root = tempfile::tempdir().unwrap();
        let (manager, repo, source) = fixture(root.path());
        version(&repo, "two");
        let (dir, old, scan) = prepare(&manager, &source);
        let expected = old.source_commit.clone();
        manager.state.fail_scan_cache_save(failure);
        assert!(manager
            .publish_source_scan(source.clone(), old, &dir, scan)
            .is_err());
        assert_eq!(
            manager
                .state
                .load_source_scan_cache(&source.id)
                .unwrap()
                .source_commit,
            expected
        );
        let text = installed_text(&manager, &source);
        assert!(text.ends_with("one\n"));
    }
}
