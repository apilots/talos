use super::*;
use crate::BrowserRuntime;
use talos_types::{BrowserDisplayCapability, BrowserDisplayUnavailableReason, BrowserHostKind};

fn bundled_backend(
    home: &Path,
    cleanup: Arc<crate::process::ProcessCleanup>,
) -> Arc<ManagedBrowser> {
    Arc::new(ManagedBrowser {
        package: Package {
            schema_version: 1,
            browser_use_version: "0.13.11".into(),
            chromium_version: "156.0.8078.4".into(),
            python: home.join("python"),
            driver: home.join("driver.py"),
            chromium: home.join("chromium"),
        },
        home: home.into(),
        driver: Driver {
            python: home.join("python"),
            script: home.join("driver.py"),
            downloads: home.join("downloads"),
            pressed: Default::default(),
            cleanup: cleanup.clone(),
        },
        components: AsyncMutex::new(None),
        state: AsyncMutex::new(Default::default()),
        status: AtomicU8::new(0),
        failure: Mutex::new(None),
    })
}

#[tokio::test]
async fn packaged_desktop_capability_survives_component_failure_without_becoming_ready() {
    let home = tempfile::tempdir().unwrap();
    let cleanup = Arc::new(crate::process::ProcessCleanup::default());
    let mut command = Command::new("/bin/sh");
    command.args(["-c", "exit 7"]);
    let mut child = BrowserProcess::spawn(command, cleanup.clone()).unwrap();
    child.wait().await.unwrap();
    let profile_lock = talos_fs::StdFileSystem
        .try_file_lock(&home.path().join("profile.lock"))
        .unwrap()
        .unwrap();
    let backend = bundled_backend(home.path(), cleanup.clone());
    *backend.components.lock().await = Some(Components {
        children: vec![child],
        browser_index: 0,
        endpoint: String::new(),
        _profile_lock: profile_lock,
    });
    backend.status.store(2, Ordering::Release);
    let runtime = BrowserRuntime::new(
        home.path().join("docker"),
        BrowserHostKind::Docker,
        Arc::new(|| Ok(true)),
    )
    .unwrap();
    assert_eq!(
        runtime.snapshot().unwrap().display,
        BrowserDisplayCapability::Unavailable {
            reason: BrowserDisplayUnavailableReason::NotBundled,
        }
    );
    runtime
        .set_desktop_connector(crate::tests::desktop_connector())
        .unwrap();
    runtime.set_backend(backend.clone()).unwrap();
    assert!(backend
        .endpoint()
        .await
        .unwrap_err()
        .is_code(ErrorCode::ServiceUnavailable));
    let snapshot = runtime.snapshot().unwrap();
    assert_eq!(
        snapshot.display,
        BrowserDisplayCapability::DockerDesktop {
            target_id: "managed".into(),
        }
    );
    assert!(matches!(
        &snapshot.targets[0].readiness,
        BrowserReadiness::Unavailable { error } if error.code == ErrorCode::ServiceUnavailable
    ));
    assert!(backend
        .desktop()
        .await
        .unwrap_err()
        .is_code(ErrorCode::ServiceUnavailable));

    let native = BrowserRuntime::new(
        home.path().join("native"),
        BrowserHostKind::Desktop,
        Arc::new(|| Ok(true)),
    )
    .unwrap();
    native.set_backend(backend.clone()).unwrap();
    assert_eq!(
        native.snapshot().unwrap().display,
        BrowserDisplayCapability::Unavailable {
            reason: BrowserDisplayUnavailableReason::NativeHost,
        }
    );
    drop(native);
    drop(runtime);
    drop(backend);
    cleanup.drain().await.unwrap();
}

#[tokio::test]
async fn packaged_startup_failure_preserves_display_and_reports_readiness_error() {
    let home = tempfile::tempdir().unwrap();
    let blocked_home = home.path().join("blocked-home");
    std::fs::write(&blocked_home, b"not a directory").unwrap();
    let cleanup = Arc::new(crate::process::ProcessCleanup::default());
    let backend = bundled_backend(&blocked_home, cleanup.clone());
    let runtime = BrowserRuntime::new(
        home.path().join("runtime"),
        BrowserHostKind::Docker,
        Arc::new(|| Ok(true)),
    )
    .unwrap();
    runtime
        .set_desktop_connector(crate::tests::desktop_connector())
        .unwrap();
    runtime.set_backend(backend.clone()).unwrap();
    let error = backend.endpoint().await.unwrap_err().to_api_error();
    let snapshot = runtime.snapshot().unwrap();
    assert_eq!(
        snapshot.display,
        BrowserDisplayCapability::DockerDesktop {
            target_id: "managed".into(),
        }
    );
    assert_eq!(
        snapshot.targets[0].readiness,
        BrowserReadiness::Unavailable { error }
    );
    assert!(backend.components.lock().await.is_none());
    drop(runtime);
    drop(backend);
    cleanup.drain().await.unwrap();
}
