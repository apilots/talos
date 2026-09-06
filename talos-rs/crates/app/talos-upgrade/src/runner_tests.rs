use std::fs;

use talos_config::{TalosPaths, WorkspaceDefinition, DEFAULT_WORKSPACE_ID};
use talos_error::{ErrorCode, TalosError};

use super::*;

const FIRST_TASK_ID: &str = "202608290001_first";
const SECOND_TASK_ID: &str = "202608290002_second";

fn test_config(home: &Path) -> Config {
    Config::default()
        .with_runtime_paths(TalosPaths::from_home(home.to_path_buf()))
        .with_workspace_definition(WorkspaceDefinition::new(
            DEFAULT_WORKSPACE_ID,
            "workspace-talos",
        ))
}

fn fresh_install(_context: &UpgradeContext<'_>) -> TalosResult<bool> {
    Ok(false)
}

fn legacy_install(_context: &UpgradeContext<'_>) -> TalosResult<bool> {
    Ok(true)
}

fn no_op<'a>(_context: &'a UpgradeContext<'_>) -> UpgradeFuture<'a> {
    Box::pin(async { Ok(()) })
}

fn write_first<'a>(context: &'a UpgradeContext<'_>) -> UpgradeFuture<'a> {
    Box::pin(async move {
        fs::write(context.paths.talos_home.join("first-ran"), "done").map_err(|error| {
            TalosError::internal_with_source(
                "write first proof; technical details were withheld.",
                error,
            )
        })
    })
}

fn write_second<'a>(context: &'a UpgradeContext<'_>) -> UpgradeFuture<'a> {
    Box::pin(async move {
        fs::write(context.paths.talos_home.join("second-ran"), "done").map_err(|error| {
            TalosError::internal_with_source(
                "write second proof; technical details were withheld.",
                error,
            )
        })
    })
}

fn fail_while_marker_exists<'a>(context: &'a UpgradeContext<'_>) -> UpgradeFuture<'a> {
    Box::pin(async move {
        let marker = context.paths.talos_home.join("fail-upgrade");
        if marker.exists() {
            return Err(TalosError::new(
                ErrorCode::Conflict,
                "forced upgrade failure".to_string(),
            ));
        }
        fs::write(context.paths.talos_home.join("upgrade-ran"), "done").map_err(|error| {
            TalosError::internal_with_source(
                "write upgrade proof; technical details were withheld.",
                error,
            )
        })
    })
}

fn task(
    id: &'static str,
    target_version: Version,
    priority: u32,
    execute: UpgradeExecuteFn,
) -> Box<dyn UpgradeTask> {
    Box::new(UpgradeTaskRegistration::new(
        UpgradeTaskDefinition {
            id,
            revision: 1,
            target_version,
            priority,
        },
        execute,
    ))
}

async fn run_at_version(
    config: &Config,
    current_version: &str,
    tasks: &[Box<dyn UpgradeTask>],
    probe: LegacyInstallProbe,
) -> TalosResult<UpgradeRunReport> {
    ProductUpgradeRunner::for_version(config, tasks, probe, current_version)?
        .run()
        .await
}

#[tokio::test]
async fn fresh_install_records_current_version_without_running_history() {
    let temp = tempfile::tempdir().expect("temp home");
    let config = test_config(temp.path());
    let tasks = [task(FIRST_TASK_ID, Version::new(0, 1, 7), 100, write_first)];

    let first = run_at_version(&config, "0.1.7", &tasks, fresh_install)
        .await
        .expect("record fresh install baseline");
    let second = run_at_version(&config, "0.1.7", &tasks, fresh_install)
        .await
        .expect("reuse product version baseline");
    let state = load_state(&first.state_path)
        .expect("read product version state")
        .expect("state should exist");

    assert_eq!(
        (
            first.previous_version,
            first.current_version,
            first.fresh_install,
            first.applied,
            first.skipped,
            second.fresh_install,
            state.product_version,
            state.tasks,
            temp.path().join("first-ran").exists(),
        ),
        (
            "0.1.7".to_string(),
            "0.1.7".to_string(),
            true,
            Vec::<String>::new(),
            vec![FIRST_TASK_ID.to_string()],
            false,
            Version::new(0, 1, 7),
            BTreeMap::new(),
            false,
        )
    );
}

#[tokio::test]
async fn only_tasks_in_product_version_interval_run() {
    let temp = tempfile::tempdir().expect("temp home");
    let config = test_config(temp.path());
    let tasks = [
        task(SECOND_TASK_ID, Version::new(0, 1, 8), 100, write_second),
        task(FIRST_TASK_ID, Version::new(0, 1, 7), 100, write_first),
    ];

    let first = run_at_version(&config, "0.1.7", &tasks, legacy_install)
        .await
        .expect("upgrade through 0.1.7");
    let second = run_at_version(&config, "0.1.8-SNAPSHOT", &tasks, legacy_install)
        .await
        .expect("exercise 0.1.8 tasks during development");
    let release = run_at_version(&config, "0.1.8", &tasks, legacy_install)
        .await
        .expect("promote the completed snapshot to 0.1.8");
    let state = load_state(&release.state_path)
        .expect("read completed upgrade state")
        .expect("completed upgrade state should exist");
    let first_task_state = state.tasks.get(FIRST_TASK_ID).expect("first task state");
    let second_task_state = state.tasks.get(SECOND_TASK_ID).expect("second task state");

    assert_eq!(
        (
            first.previous_version,
            first.applied,
            first.deferred,
            second.previous_version,
            second.current_version,
            second.applied,
            second.skipped,
            release.previous_version,
            release.applied,
            release.skipped,
        ),
        (
            "0.1.6".to_string(),
            vec![FIRST_TASK_ID.to_string()],
            vec![SECOND_TASK_ID.to_string()],
            "0.1.7".to_string(),
            "0.1.8-SNAPSHOT".to_string(),
            vec![SECOND_TASK_ID.to_string()],
            vec![FIRST_TASK_ID.to_string()],
            "0.1.8-SNAPSHOT".to_string(),
            Vec::<String>::new(),
            vec![FIRST_TASK_ID.to_string(), SECOND_TASK_ID.to_string()],
        )
    );
    assert_eq!(
        (
            first_task_state.target_version.clone(),
            first_task_state.executed_version.clone(),
            first_task_state.status,
            second_task_state.target_version.clone(),
            second_task_state.executed_version.clone(),
            second_task_state.status,
        ),
        (
            Version::new(0, 1, 7),
            Version::new(0, 1, 7),
            UpgradeTaskStatus::Success,
            Version::new(0, 1, 8),
            Version::parse("0.1.8-SNAPSHOT").unwrap(),
            UpgradeTaskStatus::Success,
        )
    );
    assert!(temp.path().join("first-ran").is_file());
    assert!(temp.path().join("second-ran").is_file());
}

#[tokio::test]
async fn failed_task_keeps_previous_version_and_can_retry() {
    let temp = tempfile::tempdir().expect("temp home");
    let config = test_config(temp.path());
    let marker = temp.path().join("fail-upgrade");
    fs::write(&marker, "fail").unwrap();
    let tasks = [task(
        FIRST_TASK_ID,
        Version::new(0, 1, 7),
        100,
        fail_while_marker_exists,
    )];

    let error = run_at_version(&config, "0.1.7", &tasks, legacy_install)
        .await
        .expect_err("task must fail");
    let state = load_state(&temp.path().join(UPGRADE_DIR).join(UPGRADE_STATE_FILE))
        .expect("read failed upgrade state")
        .expect("bootstrap state should exist");
    assert!(error.is_code(ErrorCode::Conflict));
    assert_eq!(state.product_version, Version::new(0, 1, 6));
    assert_eq!(
        state.tasks.get(FIRST_TASK_ID).map(|state| state.status),
        Some(UpgradeTaskStatus::Failed)
    );

    fs::remove_file(marker).unwrap();
    let report = run_at_version(&config, "0.1.7", &tasks, legacy_install)
        .await
        .expect("retry task");

    assert_eq!(report.applied, vec![FIRST_TASK_ID.to_string()]);
    assert!(temp.path().join("upgrade-ran").is_file());
}

#[tokio::test]
async fn completed_task_revision_change_is_rejected() {
    let temp = tempfile::tempdir().expect("temp home");
    let config = test_config(temp.path());
    run_at_version(
        &config,
        "0.1.7",
        &[task(FIRST_TASK_ID, Version::new(0, 1, 7), 100, no_op)],
        legacy_install,
    )
    .await
    .expect("complete task");

    let error = run_at_version(
        &config,
        "0.1.8",
        &[Box::new(UpgradeTaskRegistration::new(
            UpgradeTaskDefinition {
                id: FIRST_TASK_ID,
                revision: 2,
                target_version: Version::new(0, 1, 7),
                priority: 100,
            },
            no_op,
        ))],
        legacy_install,
    )
    .await
    .expect_err("revision change must fail");

    assert!(error.is_code(ErrorCode::InvalidRequest));
}

#[tokio::test]
async fn invalid_registry_and_corrupt_state_fail_fast() {
    let temp = tempfile::tempdir().expect("temp home");
    let config = test_config(temp.path());
    assert!(run_at_version(
        &config,
        "0.1.7",
        &[
            task(FIRST_TASK_ID, Version::new(0, 1, 7), 100, no_op),
            task(FIRST_TASK_ID, Version::new(0, 1, 7), 100, no_op),
        ],
        fresh_install,
    )
    .await
    .is_err());

    let upgrade_dir = temp.path().join(UPGRADE_DIR);
    fs::create_dir_all(&upgrade_dir).unwrap();
    fs::write(upgrade_dir.join(UPGRADE_STATE_FILE), "not-json").unwrap();
    let error = run_at_version(
        &config,
        "0.1.7",
        &[task(FIRST_TASK_ID, Version::new(0, 1, 7), 100, no_op)],
        fresh_install,
    )
    .await
    .expect_err("corrupt state must stop startup");

    assert!(error.is_code(ErrorCode::InvalidRequest));
}
