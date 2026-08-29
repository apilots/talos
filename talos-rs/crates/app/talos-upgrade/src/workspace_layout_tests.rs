use super::*;
use crate::runner::UpgradeRunner;

fn test_config(home: &Path) -> Config {
    Config::default()
        .with_runtime_paths(TalosPaths::from_home(home.to_path_buf()))
        .with_workspace_definition(talos_config::WorkspaceDefinition::new(
            talos_config::DEFAULT_WORKSPACE_ID,
            "workspace-talos",
        ))
}

fn backup_source(home: &Path) -> PathBuf {
    home.join("upgrades")
        .join(WORKSPACE_LAYOUT_V1_UPGRADE_ID)
        .join("source")
}

async fn run_workspace_upgrade(config: &Config) -> TalosResult<crate::runner::UpgradeRunReport> {
    run_workspace_upgrade_at_version(config, "0.1.8").await
}

async fn run_workspace_upgrade_at_version(
    config: &Config,
    current_version: &str,
) -> TalosResult<crate::runner::UpgradeRunReport> {
    let tasks = [upgrade_task()];
    crate::runner::ProductUpgradeRunner::for_version(
        config,
        &tasks,
        legacy_installation_exists,
        current_version,
    )?
    .run()
    .await
}

#[tokio::test]
async fn fresh_install_records_baseline_and_initializes_anonymous_workspace() {
    let temp = tempfile::tempdir().expect("temp home");
    let config = test_config(temp.path());

    let baseline = run_workspace_upgrade_at_version(&config, "0.1.7")
        .await
        .expect("record fresh installation baseline");
    let prefix = initialize_workspace_layout(&config).expect("initialize workspace layout");
    let upgraded = run_workspace_upgrade(&config)
        .await
        .expect("upgrade fresh 0.1.7 installation");

    assert_eq!(
        (
            baseline.applied,
            baseline.fresh_install,
            baseline.deferred,
            upgraded.applied,
        ),
        (
            Vec::<String>::new(),
            true,
            vec![WORKSPACE_LAYOUT_V1_UPGRADE_ID.to_string()],
            vec![WORKSPACE_LAYOUT_V1_UPGRADE_ID.to_string()],
        )
    );
    assert!(prefix.join("anonymous").is_dir());
    assert!(!backup_source(temp.path()).exists());
}

#[tokio::test]
async fn renames_default_legacy_workspace_and_rebuilds_user_directories() {
    let temp = tempfile::tempdir().expect("temp home");
    let config = test_config(temp.path());
    let prefix = config.talos_paths().workspace_prefix();
    fs::create_dir_all(prefix.join("users/anonymous")).unwrap();
    fs::create_dir_all(prefix.join("users/alice")).unwrap();
    fs::write(prefix.join("users/alice/note.md"), "alice").unwrap();
    fs::write(prefix.join("SOUL.md"), "unowned").unwrap();
    fs::write(prefix.join(".DS_Store"), "system").unwrap();

    let report = run_workspace_upgrade(&config)
        .await
        .expect("legacy layout should migrate");
    initialize_workspace_layout(&config).expect("initialize migrated layout");
    let backup = backup_source(temp.path());

    assert_eq!(
        report.applied,
        vec![WORKSPACE_LAYOUT_V1_UPGRADE_ID.to_string()]
    );
    assert_eq!(
        fs::read_to_string(prefix.join("alice/note.md")).unwrap(),
        "alice"
    );
    assert!(prefix.join("anonymous").is_dir());
    assert!(!prefix.join("users").exists());
    assert!(!prefix.join("SOUL.md").exists());
    assert_eq!(
        fs::read_to_string(backup.join("users/alice/note.md")).unwrap(),
        "alice"
    );
    assert_eq!(
        fs::read_to_string(backup.join("SOUL.md")).unwrap(),
        "unowned"
    );
    assert!(backup.join(".DS_Store").is_file());
}

#[tokio::test]
async fn migrates_only_the_single_legacy_workspace_derived_from_agent_name() {
    let temp = tempfile::tempdir().expect("temp home");
    let paths = TalosPaths::from_home(temp.path().to_path_buf());
    let mut config = test_config(temp.path());
    config.agent.name = "writer".to_string();
    config.agent.agents = vec![talos_config::AgentDefinitionConfig {
        id: "reviewer".to_string(),
        name: "Reviewer".to_string(),
        kind: talos_config::AgentKind::Kernel,
        enabled: true,
        description: None,
        runtime_config: None,
    }];
    let writer = paths.legacy_workspace_dir_for_agent("writer");
    let reviewer = paths.legacy_workspace_dir_for_agent("reviewer");
    fs::create_dir_all(writer.join("users/alice")).unwrap();
    fs::write(writer.join("users/alice/writer.md"), "writer").unwrap();
    fs::create_dir_all(reviewer.join("users/alice")).unwrap();
    fs::write(reviewer.join("users/alice/reviewer.md"), "reviewer").unwrap();

    run_workspace_upgrade(&config)
        .await
        .expect("single legacy workspace should migrate");

    let prefix = config
        .workspace_root_prefix(config.default_workspace_id())
        .unwrap();
    assert_eq!(
        fs::read_to_string(prefix.join("alice/writer.md")).unwrap(),
        "writer"
    );
    assert!(!prefix.join("alice/reviewer.md").exists());
    assert!(reviewer.join("users/alice/reviewer.md").is_file());
    assert!(backup_source(temp.path())
        .join("users/alice/writer.md")
        .is_file());
}

#[tokio::test]
async fn resumes_from_stable_backup_after_an_interrupted_migration() {
    let temp = tempfile::tempdir().expect("temp home");
    let config = test_config(temp.path());
    let backup = backup_source(temp.path());
    let prefix = config.talos_paths().workspace_prefix();
    fs::create_dir_all(backup.join("users/alice")).unwrap();
    fs::write(backup.join("users/alice/one.md"), "one").unwrap();
    fs::write(backup.join("users/alice/two.md"), "two").unwrap();
    fs::create_dir_all(prefix.join("alice")).unwrap();
    fs::write(prefix.join("alice/one.md"), "one").unwrap();

    let report = run_workspace_upgrade(&config)
        .await
        .expect("backup migration should resume");

    assert_eq!(
        report.applied,
        vec![WORKSPACE_LAYOUT_V1_UPGRADE_ID.to_string()]
    );
    assert_eq!(
        fs::read_to_string(prefix.join("alice/two.md")).unwrap(),
        "two"
    );
    assert!(backup.join("users/alice/one.md").is_file());
}

#[tokio::test]
async fn backup_recovery_stops_on_target_content_conflict() {
    let temp = tempfile::tempdir().expect("temp home");
    let config = test_config(temp.path());
    let backup = backup_source(temp.path());
    let prefix = config.talos_paths().workspace_prefix();
    fs::create_dir_all(backup.join("users/alice")).unwrap();
    fs::write(backup.join("users/alice/shared.md"), "legacy").unwrap();
    fs::create_dir_all(prefix.join("alice")).unwrap();
    fs::write(prefix.join("alice/shared.md"), "current").unwrap();

    let error = run_workspace_upgrade(&config)
        .await
        .expect_err("conflict must stop startup");

    assert!(error.is_code(ErrorCode::Conflict));
    assert_eq!(
        fs::read_to_string(backup.join("users/alice/shared.md")).unwrap(),
        "legacy"
    );
}

#[tokio::test]
async fn nested_target_prefix_does_not_recurse_into_the_legacy_users_directory() {
    let temp = tempfile::tempdir().expect("temp home");
    let paths = TalosPaths::from_home(temp.path().to_path_buf());
    let config = Config::default()
        .with_runtime_paths(paths.clone())
        .with_workspace_definition(talos_config::WorkspaceDefinition::new(
            talos_config::DEFAULT_WORKSPACE_ID,
            "workspace-talos/users/new",
        ));
    let legacy = paths.legacy_workspace_dir_for_agent("talos");
    fs::create_dir_all(legacy.join("users/alice")).unwrap();
    fs::write(legacy.join("users/alice/note.md"), "alice").unwrap();

    run_workspace_upgrade(&config)
        .await
        .expect("nested target should rebuild from backup");

    let target = config
        .workspace_root_prefix(config.default_workspace_id())
        .unwrap();
    assert_eq!(
        fs::read_to_string(target.join("alice/note.md")).unwrap(),
        "alice"
    );
    assert!(!target.join("new").exists());
    assert!(backup_source(temp.path())
        .join("users/alice/note.md")
        .is_file());
}
