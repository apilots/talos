use super::*;
use std::fs;
use std::path::{Path, PathBuf};
use talos_guard_task::{GuardTaskHandle, GuardTaskSpec, GuardTasks};

fn start(config: Arc<RwLock<Config>>) -> (Arc<GuardTasks>, GuardTaskHandle) {
    let tasks = Arc::new(GuardTasks::default());
    let handle = tasks
        .register(
            GuardTaskSpec {
                name: "bash_output_cleanup",
                interval: Duration::from_secs(1),
                run_on_start: true,
            },
            OutputLogMaintenance::new(config),
        )
        .unwrap();
    (tasks, handle)
}

pub(super) fn config(root: &Path) -> Config {
    let mut config = Config::default()
        .with_workspace_definition(talos_config::WorkspaceDefinition::new("default", root));
    config
        .agent
        .kernel
        .tools
        .bash
        .output_log
        .cleanup_interval_secs = 1;
    config
}

pub(super) fn fixture(
    config: &Config,
    user: &str,
    name: &str,
    status: &str,
    expired: bool,
) -> PathBuf {
    let workspace = config.resolve_workspace("default", user).unwrap();
    let relative = format!("tool-output/session/turn/{name}/output.log");
    let path = workspace.root.join(&relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, b"retained output").unwrap();
    fs::write(path.with_file_name("metadata.json"), serde_json::to_vec(&serde_json::json!({
        "version": 1, "reservation_bytes": 64 * 1024 * 1024 + 4096, "cleanup": "retained",
        "log": {"path": path, "relativePath": relative, "totalBytes": 15, "storedBytes": 15,
            "totalLines": 1, "captureStatus": status, "expiresAt": if expired {1_i64} else {i64::MAX}}
    })).unwrap()).unwrap();
    path
}

async fn wait_until(probe: impl Fn() -> bool) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !probe() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn startup_recovers_offline_user_logs_without_bash() {
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path());
    let expired = fixture(&config, "offline-user", "expired", "complete", true);
    let interrupted = fixture(&config, "other-user", "orphan", "capturing", false);
    let (task, _) = start(Arc::new(RwLock::new(config)));
    wait_until(|| {
        !expired.exists()
            && fs::read_to_string(interrupted.with_file_name("metadata.json"))
                .unwrap()
                .contains("interrupted")
    })
    .await;
    task.shutdown().await.unwrap();
    assert_eq!(fs::read(&interrupted).unwrap(), b"retained output");
    assert!(interrupted
        .ancestors()
        .nth(4)
        .unwrap()
        .join("quota.json")
        .exists());
}

#[tokio::test]
async fn periodic_expiry_runs_while_no_tool_is_executing_and_shutdown_is_shared() {
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path());
    let log = fixture(&config, "alice", "expired-later", "complete", false);
    let quota = config
        .resolve_workspace("default", "alice")
        .unwrap()
        .root
        .join("tool-output/quota.json");
    let (task, _) = start(Arc::new(RwLock::new(config)));
    wait_until(|| quota.exists()).await;
    let metadata = log.with_file_name("metadata.json");
    let mut value: serde_json::Value =
        serde_json::from_slice(&fs::read(&metadata).unwrap()).unwrap();
    value["log"]["expiresAt"] = serde_json::json!(1);
    fs::write(&metadata, serde_json::to_vec(&value).unwrap()).unwrap();
    wait_until(|| !log.exists()).await;
    let clone = task.clone();
    let (first, second) = tokio::join!(task.shutdown(), clone.shutdown());
    first.unwrap();
    second.unwrap();
    task.shutdown().await.unwrap();
}

#[tokio::test]
async fn broken_workspace_does_not_stop_another_user_cleanup_or_next_round() {
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path());
    let broken = fixture(&config, "alice", "broken", "complete", true);
    let valid = fixture(&config, "bob", "expired", "complete", true);
    let metadata = broken.with_file_name("metadata.json");
    let saved = fs::read(&metadata).unwrap();
    fs::write(&metadata, b"invalid metadata").unwrap();
    let (task, _) = start(Arc::new(RwLock::new(config)));
    wait_until(|| !valid.exists()).await;
    assert!(broken.exists());
    fs::write(metadata, saved).unwrap();
    wait_until(|| !broken.exists()).await;
    task.shutdown().await.unwrap();
}

#[tokio::test]
async fn config_publication_wakes_the_single_task_and_changes_its_policy() {
    let root = tempfile::tempdir().unwrap();
    let mut config = config(root.path());
    config
        .agent
        .kernel
        .tools
        .bash
        .output_log
        .cleanup_interval_secs = 3600;
    let retained = fixture(&config, "alice", "retained", "complete", false);
    let quota = config
        .resolve_workspace("default", "alice")
        .unwrap()
        .root
        .join("tool-output/quota.json");
    let active = Arc::new(RwLock::new(config));
    let (task, changed) = start(active.clone());
    changed.set_interval(Duration::from_secs(3600)).unwrap();
    wait_until(|| quota.exists()).await;
    let expired = fixture(
        &active.read().unwrap(),
        "alice",
        "newly-expired",
        "complete",
        true,
    );
    active
        .write()
        .unwrap()
        .agent
        .kernel
        .tools
        .bash
        .output_log
        .cleanup_interval_secs = 1;
    changed.set_interval(Duration::from_secs(1)).unwrap();
    changed.wake();
    wait_until(|| !expired.exists()).await;
    task.shutdown().await.unwrap();
    assert!(retained.exists());
}

#[cfg(unix)]
#[test]
fn discovery_skips_symlinked_users_and_workspaces_without_logs() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let config = config(root.path());
    fs::create_dir(root.path().join("unused")).unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join("linked-user")).unwrap();
    fixture(&config, "alice", "retained", "complete", false);
    let mut discovery = discovery::Discovery::new();
    let (roots, errors, complete) = discovery.step(&config, || false, Duration::from_secs(1), 128);
    assert!(errors.is_empty());
    assert!(complete);
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0].user_id, "alice");
}

#[tokio::test]
async fn invalid_workspace_prefix_does_not_block_another_configured_root() {
    let root = tempfile::tempdir().unwrap();
    let valid_root = root.path().join("valid");
    let invalid_root = root.path().join("invalid");
    fs::write(&invalid_root, b"not a directory").unwrap();
    let mut config = config(&valid_root);
    config
        .workspaces
        .definitions
        .push(talos_config::WorkspaceDefinition::new(
            "broken",
            invalid_root,
        ));
    let expired = fixture(&config, "alice", "expired", "complete", true);
    let (task, _) = start(Arc::new(RwLock::new(config)));
    wait_until(|| !expired.exists()).await;
    task.shutdown().await.unwrap();
}

#[test]
fn agents_cannot_bypass_the_execution_node_workspace_quota() {
    let config = Config::default();
    let mut kernel = config.agent.kernel.clone();
    assert!(validate_policy(&config, &kernel).is_ok());
    kernel.tools.bash.output_log.max_workspace_bytes *= 2;
    assert!(validate_policy(&config, &kernel)
        .unwrap_err()
        .is_code(ErrorCode::InvalidRequest));
}

// Observe the supervisor's real restart event without sleeping or adding a
// production-only test hook. This test uses a current-thread Tokio runtime.
struct RestartObserved(Arc<std::sync::atomic::AtomicBool>);
impl tracing::Subscriber for RestartObserved {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        if event
            .metadata()
            .fields()
            .field("retry_after_secs")
            .is_some()
        {
            self.0.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

#[tokio::test]
async fn unexpected_loop_panic_recovers_after_configuration_is_repaired() {
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path());
    let expired = fixture(&config, "alice", "after-panic", "complete", true);
    let active = Arc::new(RwLock::new(config));
    let poisoned = active.clone();
    let _ = std::panic::catch_unwind(move || {
        let _guard = poisoned.write().unwrap();
        panic!("injected configuration writer panic");
    });
    let observed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let _subscriber = tracing::subscriber::set_default(RestartObserved(observed.clone()));
    let (task, _) = start(active.clone());
    let restarted = tokio::time::timeout(Duration::from_secs(5), async {
        while !observed.load(std::sync::atomic::Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
    })
    .await;
    active.clear_poison();
    if restarted.is_ok() {
        wait_until(|| !expired.exists()).await;
    }
    task.shutdown().await.unwrap();
    assert!(
        restarted.is_ok(),
        "a reported loop panic must schedule recovery"
    );
    assert!(!expired.exists());
}

#[tokio::test]
async fn shutdown_cancels_a_panicked_loop_during_restart_backoff() {
    let root = tempfile::tempdir().unwrap();
    let active = Arc::new(RwLock::new(config(root.path())));
    let poisoned = active.clone();
    let _ = std::panic::catch_unwind(move || {
        let _guard = poisoned.write().unwrap();
        panic!("injected configuration writer panic");
    });
    let observed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let _subscriber = tracing::subscriber::set_default(RestartObserved(observed.clone()));
    let (task, _) = start(active);
    wait_until(|| observed.load(std::sync::atomic::Ordering::SeqCst)).await;
    tokio::time::timeout(Duration::from_millis(500), task.shutdown())
        .await
        .unwrap()
        .unwrap();
}
