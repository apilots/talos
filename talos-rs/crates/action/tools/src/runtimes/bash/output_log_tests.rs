use super::*;

fn settings() -> BashOutputLogConfig {
    BashOutputLogConfig {
        max_file_bytes: 32,
        max_workspace_bytes: 16 * 1024,
        retention_days: 7,
        cleanup_interval_secs: 3600,
    }
}

async fn writer(root: &Path, config: BashOutputLogConfig) -> TalosResult<LogWriter> {
    LogWriter::open(
        root.into(),
        "session".into(),
        "turn".into(),
        "call".into(),
        config,
    )
    .await
}

async fn finish(
    log: LogWriter,
    bytes: u64,
    lines: u64,
    interrupted: bool,
) -> TalosResult<BashOutputLog> {
    let completion = log.finish(bytes, lines, interrupted).await;
    match completion.error {
        Some(error) => Err(error),
        None => Ok(completion.log),
    }
}

#[tokio::test]
async fn full_output_survives_writer_release_and_paths_do_not_repeat_user() {
    let workspace = tempfile::tempdir().unwrap();
    let mut log = writer(workspace.path(), settings()).await.unwrap();
    log.append(b"hello\n").await.unwrap();
    let reference = finish(log, 6, 1, false).await.unwrap();
    assert_eq!(reference.capture_status, CaptureStatus::Complete);
    assert_eq!(fs::read(&reference.path).unwrap(), b"hello\n");
    assert_eq!(
        workspace.path().join(&reference.relative_path),
        PathBuf::from(&reference.path)
    );
    assert!(reference.relative_path.starts_with("tool-output/"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&reference.path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[tokio::test]
async fn file_quota_preserves_prefix_and_marks_incomplete() {
    let workspace = tempfile::tempdir().unwrap();
    let mut log = writer(workspace.path(), settings()).await.unwrap();
    log.append(&[b'x'; 100]).await.unwrap();
    let reference = finish(log, 100, 1, false).await.unwrap();
    assert_eq!(reference.stored_bytes, 32);
    assert_eq!(reference.total_bytes, 100);
    assert_eq!(reference.capture_status, CaptureStatus::QuotaLimited);
    assert_eq!(fs::read(reference.path).unwrap(), vec![b'x'; 32]);
}

#[tokio::test]
async fn active_reservations_cannot_be_evicted_and_completed_logs_can() {
    let workspace = tempfile::tempdir().unwrap();
    let mut config = settings();
    config.max_workspace_bytes = config.max_file_bytes + METADATA_RESERVE;
    let mut active = writer(workspace.path(), config.clone()).await.unwrap();
    let second = writer(workspace.path(), config.clone()).await;
    assert!(matches!(second, Err(error) if error.is_code(ErrorCode::UsageLimitExceeded)));
    active.append(b"saved").await.unwrap();
    let reference = finish(active, 5, 1, false).await.unwrap();
    LogWriter::maintain_workspace(workspace.path().into(), config.clone())
        .await
        .unwrap();
    let _second = writer(workspace.path(), config).await.unwrap();
    assert!(!Path::new(&reference.path).exists());
}

#[tokio::test]
async fn expired_logs_are_removed_only_after_the_capture_lock_is_released() {
    let workspace = tempfile::tempdir().unwrap();
    let active = writer(workspace.path(), settings()).await.unwrap();
    let directory = active.directory.clone();
    let reference = finish(active, 0, 0, false).await.unwrap();
    let mut record = load_record(&directory).unwrap();
    record.log.expires_at = 0;
    save_record(&directory, &record).unwrap();
    LogWriter::maintain_workspace(workspace.path().into(), settings())
        .await
        .unwrap();
    let _second = writer(workspace.path(), settings()).await.unwrap();
    assert!(!Path::new(&reference.path).exists());
}

#[tokio::test]
async fn orphaned_capture_recovers_partial_bytes_without_an_exit_code() {
    let workspace = tempfile::tempdir().unwrap();
    let mut active = writer(workspace.path(), settings()).await.unwrap();
    active.append(b"partial").await.unwrap();
    active.file.flush().await.unwrap();
    let directory = active.directory.clone();
    drop(active);
    LogWriter::maintain_workspace(workspace.path().into(), settings())
        .await
        .unwrap();
    let _second = writer(workspace.path(), settings()).await.unwrap();
    let record = load_record(&directory).unwrap();
    assert_eq!(record.log.capture_status, CaptureStatus::Interrupted);
    assert_eq!(record.log.stored_bytes, 7);
    assert_eq!(fs::read(record.log.path).unwrap(), b"partial");
}

#[tokio::test]
async fn write_failure_is_explicit_and_does_not_become_complete() {
    let workspace = tempfile::tempdir().unwrap();
    let mut log = writer(workspace.path(), settings()).await.unwrap();
    log.file = tokio::fs::File::from_std(File::open(&log.record.log.path).unwrap());
    match log.append(b"failure").await {
        Err(_) => assert_eq!(log.record.log.capture_status, CaptureStatus::WriteFailed),
        // Tokio may first accept into its bounded buffer. Finalization must
        // still surface the OS write failure instead of publishing Complete.
        Ok(()) => assert!(finish(log, 7, 1, false).await.is_err()),
    }
}

#[tokio::test]
async fn failed_flush_persists_actual_bytes_and_write_failed_metadata() {
    let workspace = tempfile::tempdir().unwrap();
    let mut log = writer(workspace.path(), settings()).await.unwrap();
    let directory = log.directory.clone();
    log.file = tokio::fs::File::from_std(File::open(&log.record.log.path).unwrap());
    let _write_result = log.append(b"failure").await;
    assert!(finish(log, 7, 1, false).await.is_err());
    let record = load_record(&directory).unwrap();
    assert_eq!(record.log.capture_status, CaptureStatus::WriteFailed);
    assert_eq!(record.log.stored_bytes, 0);
    assert_eq!(record.log.total_bytes, 7);
}

#[tokio::test]
async fn cleanup_resumes_after_the_body_was_removed_before_metadata() {
    let workspace = tempfile::tempdir().unwrap();
    let log = writer(workspace.path(), settings()).await.unwrap();
    let directory = log.directory.clone();
    let reference = finish(log, 0, 0, false).await.unwrap();
    let metadata = directory.join("metadata.json");
    let mut record: serde_json::Value =
        serde_json::from_slice(&fs::read(&metadata).unwrap()).unwrap();
    record["cleanup"] = serde_json::json!("deleting");
    fs::write(&metadata, serde_json::to_vec(&record).unwrap()).unwrap();
    fs::remove_file(reference.path).unwrap();
    LogWriter::maintain_workspace(workspace.path().into(), settings())
        .await
        .unwrap();
    assert!(
        !directory.exists(),
        "cleanup intent must survive a partial removal"
    );
}

#[tokio::test]
async fn cleanup_reclaims_empty_directories_left_after_the_last_unlink() {
    let workspace = tempfile::tempdir().unwrap();
    let log = writer(workspace.path(), settings()).await.unwrap();
    let directory = log.directory.clone();
    finish(log, 0, 0, false).await.unwrap();
    for name in ["output.log", "capture.lock", "metadata.json"] {
        fs::remove_file(directory.join(name)).unwrap();
    }
    for _ in 0..2 {
        LogWriter::maintain_workspace(workspace.path().into(), settings())
            .await
            .unwrap();
    }
    assert!(!directory.exists());
    assert_eq!(
        fs::read_dir(workspace.path().join("tool-output"))
            .unwrap()
            .count(),
        2
    );
}

#[tokio::test]
async fn unknown_directory_entries_do_not_destroy_owned_files_during_cleanup() {
    let workspace = tempfile::tempdir().unwrap();
    let mut log = writer(workspace.path(), settings()).await.unwrap();
    log.append(b"retained").await.unwrap();
    let directory = log.directory.clone();
    let reference = finish(log, 8, 1, false).await.unwrap();
    let mut record = load_record(&directory).unwrap();
    record.log.expires_at = 0;
    save_record(&directory, &record).unwrap();
    fs::write(directory.join("unexpected"), b"user data").unwrap();
    assert!(
        LogWriter::maintain_workspace(workspace.path().into(), settings())
            .await
            .is_err()
    );
    assert_eq!(fs::read(reference.path).unwrap(), b"retained");
    assert!(directory.join("metadata.json").exists());
    assert_eq!(
        fs::read(directory.join("unexpected")).unwrap(),
        b"user data"
    );
}

#[tokio::test]
async fn concurrent_reservations_cannot_both_claim_the_last_log_slot() {
    let workspace = tempfile::tempdir().unwrap();
    let config = BashOutputLogConfig {
        max_workspace_bytes: settings().max_file_bytes + METADATA_RESERVE,
        ..settings()
    };
    let (first, second) = tokio::join!(
        writer(workspace.path(), config.clone()),
        writer(workspace.path(), config)
    );
    let outcomes = [first, second];
    assert_eq!(outcomes.iter().filter(|result| result.is_ok()).count(), 1);
    assert!(outcomes.iter().any(
        |result| matches!(result, Err(error) if error.is_code(ErrorCode::UsageLimitExceeded))
    ));
}

#[tokio::test]
async fn process_capture_lock_survives_gc_and_releases_after_crash() {
    let workspace = tempfile::tempdir().unwrap();
    let fixture = format!(
        "{}::capture_process_fixture",
        module_path!().split_once("::").unwrap().1
    );
    let mut child = tokio::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", &fixture, "--ignored"])
        .env("TALOS_TEST_OUTPUT_LOG_WORKSPACE", workspace.path())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let ready = workspace.path().join("ready.json");
    let reference: BashOutputLog =
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                if let Ok(bytes) = fs::read(&ready) {
                    break serde_json::from_slice(&bytes).unwrap();
                }
                assert!(
                    child.try_wait().unwrap().is_none(),
                    "capture fixture exited early"
                );
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
    let config = BashOutputLogConfig {
        max_workspace_bytes: settings().max_file_bytes + METADATA_RESERVE,
        ..settings()
    };
    assert!(
        matches!(writer(workspace.path(), config.clone()).await, Err(error) if error.is_code(ErrorCode::UsageLimitExceeded))
    );
    assert_eq!(fs::read(&reference.path).unwrap(), b"partial");
    child.kill().await.unwrap();
    child.wait().await.unwrap();
    LogWriter::maintain_workspace(workspace.path().into(), settings())
        .await
        .unwrap();
    let directory = Path::new(&reference.path).parent().unwrap();
    let recovered = load_record(directory).unwrap();
    assert_eq!(recovered.log.capture_status, CaptureStatus::Interrupted);
    assert_eq!(recovered.log.stored_bytes, 7);
    LogWriter::maintain_workspace(workspace.path().into(), config.clone())
        .await
        .unwrap();
    let next = writer(workspace.path(), config).await.unwrap();
    assert!(!Path::new(&reference.path).exists());
    finish(next, 0, 0, false).await.unwrap();
}

#[tokio::test]
#[ignore = "Subprocess fixture invoked by process_capture_lock_survives_gc_and_releases_after_crash"]
async fn capture_process_fixture() {
    let root = PathBuf::from(std::env::var_os("TALOS_TEST_OUTPUT_LOG_WORKSPACE").unwrap());
    let mut log = writer(&root, settings()).await.unwrap();
    log.append(b"partial").await.unwrap();
    log.file.flush().await.unwrap();
    fs::write(
        root.join("ready.tmp"),
        serde_json::to_vec(&log.record.log).unwrap(),
    )
    .unwrap();
    fs::rename(root.join("ready.tmp"), root.join("ready.json")).unwrap();
    std::future::pending::<()>().await;
    drop(log);
}

#[tokio::test]
async fn metadata_budget_includes_pretty_json_and_atomic_replacement() {
    let workspace = tempfile::tempdir().unwrap();
    let log = writer(workspace.path(), settings()).await.unwrap();
    let directory = log.directory.clone();
    finish(log, 0, 0, false).await.unwrap();
    let mut record = load_record(&directory).unwrap();
    record.log.path.clear();
    let base = serde_json::to_vec(&record).unwrap().len();
    record.log.path = "p".repeat(METADATA_RESERVE as usize / 2 - base - 16);
    assert!(serde_json::to_vec(&record).unwrap().len() <= METADATA_RESERVE as usize / 2);
    assert!(serde_json::to_vec_pretty(&record).unwrap().len() > METADATA_RESERVE as usize / 2);
    assert!(save_record(&directory, &record).is_err());
}

#[tokio::test]
async fn saturated_scan_reclaims_terminal_logs_before_ttl_and_makes_retry_progress() {
    let workspace = tempfile::tempdir().unwrap();
    let config = BashOutputLogConfig {
        max_workspace_bytes: 64 * 1024 * 1024,
        ..settings()
    };
    let log = writer(workspace.path(), config.clone()).await.unwrap();
    let directory = log.directory.clone();
    finish(log, 0, 0, false).await.unwrap();
    let mut record = load_record(&directory).unwrap();
    let turn = directory.parent().unwrap();
    for index in 0..MAX_LOG_DIRECTORIES {
        let extra = turn.join(format!("fixture-{index}"));
        fs::create_dir(&extra).unwrap();
        let path = extra.join("output.log");
        record.log.path = path.to_string_lossy().into_owned();
        record.log.relative_path = path
            .strip_prefix(workspace.path())
            .unwrap()
            .to_string_lossy()
            .into_owned();
        fs::write(
            extra.join("metadata.json"),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
    }
    let result = LogWriter::maintain_workspace(workspace.path().into(), config.clone()).await;
    assert!(matches!(result, Err(error) if error.is_code(ErrorCode::ServiceUnavailable)));
    LogWriter::maintain_workspace(workspace.path().into(), config)
        .await
        .unwrap();
    assert!(fs::read_dir(turn).unwrap().count() < MAX_LOG_DIRECTORIES);
}

#[cfg(unix)]
#[tokio::test]
async fn readonly_nonempty_foreign_directories_do_not_block_owned_cleanup() {
    use std::os::unix::fs::PermissionsExt;

    let workspace = tempfile::tempdir().unwrap();
    let log = writer(workspace.path(), settings()).await.unwrap();
    let directory = log.directory.clone();
    finish(log, 0, 0, false).await.unwrap();
    let mut record = load_record(&directory).unwrap();
    record.log.expires_at = 0;
    save_record(&directory, &record).unwrap();
    let foreign = workspace.path().join("tool-output/foreign/data/keep");
    fs::create_dir_all(&foreign).unwrap();
    fs::write(foreign.join("unrelated.txt"), b"preserve").unwrap();
    let parent = foreign.parent().unwrap();
    fs::set_permissions(parent, fs::Permissions::from_mode(0o555)).unwrap();
    let result = LogWriter::maintain_workspace(workspace.path().into(), settings()).await;
    fs::set_permissions(parent, fs::Permissions::from_mode(0o755)).unwrap();
    result.unwrap();
    assert!(!directory.exists());
    assert_eq!(
        fs::read(foreign.join("unrelated.txt")).unwrap(),
        b"preserve"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn symlinked_output_root_is_rejected() {
    let workspace = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), workspace.path().join("tool-output")).unwrap();
    assert!(
        matches!(writer(workspace.path(), settings()).await, Err(error) if error.is_code(ErrorCode::PermissionDenied))
    );
}

#[tokio::test]
async fn opening_another_log_never_scans_or_cleans_expired_metadata() {
    let workspace = tempfile::tempdir().unwrap();
    let first = writer(workspace.path(), settings()).await.unwrap();
    let directory = first.directory.clone();
    let reference = finish(first, 0, 0, false).await.unwrap();
    fs::write(
        directory.join("metadata.json"),
        b"invalid metadata that only maintenance reads",
    )
    .unwrap();
    let next = writer(workspace.path(), settings()).await.unwrap();
    finish(next, 0, 0, false).await.unwrap();
    assert!(Path::new(&reference.path).exists());
    assert!(
        LogWriter::maintain_workspace(workspace.path().into(), settings())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn missing_accounting_is_rebuilt_by_maintenance_not_by_bash() {
    let workspace = tempfile::tempdir().unwrap();
    let first = writer(workspace.path(), settings()).await.unwrap();
    finish(first, 0, 0, false).await.unwrap();
    fs::remove_file(workspace.path().join("tool-output/quota.json")).unwrap();
    assert!(
        matches!(writer(workspace.path(), settings()).await, Err(error) if error.is_code(ErrorCode::ServiceUnavailable))
    );
    LogWriter::maintain_workspace(workspace.path().into(), settings())
        .await
        .unwrap();
    finish(
        writer(workspace.path(), settings()).await.unwrap(),
        0,
        0,
        false,
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn admission_waits_for_background_initialization_without_holding_its_lock() {
    let workspace = tempfile::tempdir().unwrap();
    fs::create_dir(workspace.path().join("tool-output")).unwrap();
    let root = workspace.path().to_owned();
    let pending = tokio::spawn(async move { writer(&root, settings()).await });
    tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    LogWriter::maintain_workspace(workspace.path().into(), settings())
        .await
        .unwrap();
    finish(pending.await.unwrap().unwrap(), 0, 0, false)
        .await
        .unwrap();
}
