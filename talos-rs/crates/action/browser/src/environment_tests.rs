use super::*;
use crate::tests::Backend;
use std::sync::atomic::Ordering;
#[tokio::test]
async fn dependency_change_fences_the_bound_backend_until_worker_restart() {
    let home = tempfile::tempdir().unwrap();
    let runtime = BrowserRuntime::new(
        home.path().into(),
        BrowserHostKind::Docker,
        Arc::new(|| Ok(true)),
    )
    .unwrap();
    let first = Backend::new();
    runtime
        .activate_browser_environment(PreparedBrowserEnvironment {
            backend: first.clone(),
            identity: ("first".into(), home.path().join("python")),
        })
        .await
        .unwrap();
    assert!(matches!(
        runtime.snapshot().unwrap().targets[0].readiness,
        BrowserReadiness::Ready
    ));
    runtime
        .activate_browser_environment(PreparedBrowserEnvironment {
            backend: Backend::new(),
            identity: ("second".into(), home.path().join("python")),
        })
        .await
        .unwrap();
    assert!(matches!(
        runtime.snapshot().unwrap().targets[0].readiness,
        BrowserReadiness::Unavailable { .. }
    ));
    assert_eq!(
        runtime.backend().err().unwrap().error_code(),
        ErrorCode::ServiceUnavailable
    );
    assert!(runtime.restart_required.load(Ordering::Acquire));
}

#[tokio::test]
async fn startup_reuse_does_not_wait_for_an_environment_being_downloaded() {
    use talos_fs::FileSystem;
    let root = tempfile::tempdir().unwrap();
    let held = talos_fs::std_fs()
        .with_file_lock(&root.path().join("prepare.lock"))
        .unwrap();
    let reused = tokio::time::timeout(
        Duration::from_secs(1),
        preparation_lock(root.path().into(), Preparation::ReuseOnly),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(reused.is_none());
    drop(held);
    assert!(
        preparation_lock(root.path().into(), Preparation::InstallMissing)
            .await
            .unwrap()
            .is_some()
    );
}
