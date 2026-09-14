use std::fs;

use talos_config::{Config, TalosPaths, WorkspaceDefinition, DEFAULT_WORKSPACE_ID};

use super::*;

#[tokio::test]
async fn startup_runs_versioned_product_tasks_in_real_migration_order() {
    let temp = tempfile::tempdir().expect("temp Talos home");
    let paths = TalosPaths::from_home(temp.path().to_path_buf());
    let config = Config::default()
        .with_runtime_paths(paths.clone())
        .with_workspace_definition(WorkspaceDefinition::new(
            DEFAULT_WORKSPACE_ID,
            "workspace-talos",
        ));
    let legacy_user = paths
        .legacy_workspace_dir_for_agent(&config.agent.name)
        .join("users/alice");
    fs::create_dir_all(legacy_user.join("memory")).unwrap();
    fs::write(
        legacy_user.join("memory/2026-07-17.md"),
        "## Legacy memory\n\nUser-authored body.\n",
    )
    .unwrap();
    let migrations =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../conf/migrations/sqlite");
    let pool = SqlitePool::connect("sqlite::memory:")
        .await
        .expect("connect sqlite");

    let error = run_startup_upgrades_for_version(
        StartupUpgradeParams {
            config: &config,
            database_pool: &pool,
            database_migration_dir: &migrations,
        },
        "0.1.7",
    )
    .await
    .expect_err("0.1.7 must not expose the new layout before its task runs");
    assert!(error.is_code(talos_error::ErrorCode::InvalidRequest));
    let converted_legacy = fs::read_to_string(legacy_user.join("memory/2026-07-17.md"))
        .expect("read Dream Memory converted by 0.1.7 task");
    assert!(converted_legacy.contains("talos:dream-memory"));
    assert!(!paths.workspace_prefix().join("alice").exists());

    let report = run_startup_upgrades_for_version(
        StartupUpgradeParams {
            config: &config,
            database_pool: &pool,
            database_migration_dir: &migrations,
        },
        "0.1.8-SNAPSHOT",
    )
    .await
    .expect("complete 0.1.8 workspace upgrade during development");

    assert_eq!(
        (
            report.database.applied_versions,
            report.product.previous_version,
            report.product.current_version,
            report.product.applied,
            report.product.skipped,
        ),
        (
            Vec::<i64>::new(),
            "0.1.7".to_string(),
            "0.1.8-SNAPSHOT".to_string(),
            vec![
                "202608290001_workspace_layout_v1".to_string(),
                "202609140001_history_repair_0_1_8".to_string()
            ],
            vec!["202608290002_dream_memory_okf".to_string()],
        )
    );
    let converted = fs::read_to_string(report.workspace_prefix.join("alice/memory/2026-07-17.md"))
        .expect("read migrated Dream Memory");
    assert!(converted.contains("talos:dream-memory"));
    assert!(!paths
        .legacy_workspace_dir_for_agent(&config.agent.name)
        .join("users")
        .exists());
}
