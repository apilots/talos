use super::*;

fn settings() -> BashOutputLogConfig {
    BashOutputLogConfig {
        max_file_bytes: 32,
        max_workspace_bytes: 16 * 1024,
        ..BashOutputLogConfig::default()
    }
}

async fn writer(root: &Path) -> TalosResult<LogWriter> {
    LogWriter::open(
        root.into(),
        "session".into(),
        "turn".into(),
        "call".into(),
        settings(),
    )
    .await
}

#[tokio::test]
async fn rejected_log_creation_does_not_leak_a_full_reservation() {
    let workspace = tempfile::tempdir().unwrap();
    let log = writer(workspace.path()).await.unwrap();
    assert!(log.finish(0, 0, false).await.error.is_none());
    let root = workspace.path().join("tool-output");
    let before = quota::load(&root).unwrap();
    fs::write(
        root.join(crate::state::sha256_hex(b"blocked-session")),
        b"unexpected file",
    )
    .unwrap();
    let result = LogWriter::open(
        workspace.path().into(),
        "blocked-session".into(),
        "turn".into(),
        "call".into(),
        settings(),
    )
    .await;
    assert!(result.is_err());
    assert_eq!(
        quota::load(&root).unwrap(),
        before,
        "failed setup must release the unused reservation"
    );
}

#[tokio::test]
async fn unknown_final_size_retains_reservation_until_maintenance_confirms_it() {
    let workspace = tempfile::tempdir().unwrap();
    let mut log = writer(workspace.path()).await.unwrap();
    log.append(b"saved output").await.unwrap();
    log.file.flush().await.unwrap();
    let root = workspace.path().join("tool-output");
    let reserved = quota::load(&root).unwrap();
    let mut error = None;
    log.confirm_stored_size(
        Err(std::io::Error::other("injected fstat failure")),
        &mut error,
    );
    log.record.log.capture_status = CaptureStatus::WriteFailed;
    assert!(log.commit(error).error.is_some());
    assert_eq!(quota::load(&root).unwrap(), reserved);
    LogWriter::maintain_workspace(workspace.path().into(), settings())
        .await
        .unwrap();
    assert_eq!(quota::load(&root).unwrap(), 12 + METADATA_RESERVE);
}

#[test]
fn rollback_refuses_unknown_or_nonempty_data() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("output.log"), b"unexpected bytes").unwrap();
    assert!(creation::rollback(Some(directory.path())).is_err());
    assert_eq!(
        fs::read(directory.path().join("output.log")).unwrap(),
        b"unexpected bytes"
    );
}

#[test]
fn rollback_removes_only_known_empty_setup_files() {
    let parent = tempfile::tempdir().unwrap();
    let directory = parent.path().join("log");
    fs::create_dir(&directory).unwrap();
    for name in ["output.log", "metadata.json", "capture.lock"] {
        fs::write(directory.join(name), b"").unwrap();
    }
    creation::rollback(Some(&directory)).unwrap();
    assert!(!directory.exists());
}

#[tokio::test]
async fn corrupt_accounting_fails_closed_without_resetting_usage() {
    let workspace = tempfile::tempdir().unwrap();
    let log = writer(workspace.path()).await.unwrap();
    assert!(log.finish(0, 0, false).await.error.is_none());
    let root = workspace.path().join("tool-output");
    let quota_path = root.join("quota.json");
    fs::write(&quota_path, b"broken ledger").unwrap();
    assert!(writer(workspace.path()).await.is_err());
    assert_eq!(fs::read(&quota_path).unwrap(), b"broken ledger");
    LogWriter::maintain_workspace(workspace.path().into(), settings())
        .await
        .unwrap();
    assert_eq!(quota::load(&root).unwrap(), METADATA_RESERVE);
}

#[tokio::test]
async fn maintenance_and_parallel_finalization_reconcile_exactly() {
    let workspace = tempfile::tempdir().unwrap();
    let mut config = settings();
    config.max_workspace_bytes = 64 * 1024;
    let mut writers = Vec::new();
    for _ in 0..8 {
        let mut log = LogWriter::open(
            workspace.path().into(),
            "session".into(),
            "turn".into(),
            "call".into(),
            config.clone(),
        )
        .await
        .unwrap();
        log.append(b"saved output").await.unwrap();
        writers.push(log);
    }
    let maintenance = LogWriter::maintain_workspace(workspace.path().into(), config.clone());
    let finalization = async {
        let mut tasks = Vec::new();
        for log in writers {
            tasks.push(tokio::spawn(async move { log.finish(12, 0, false).await }));
        }
        for task in tasks {
            assert!(task.await.unwrap().error.is_none());
        }
    };
    let (result, ()) = tokio::join!(maintenance, finalization);
    result.unwrap();
    let root = workspace.path().join("tool-output");
    assert_eq!(quota::load(&root).unwrap(), 8 * (12 + METADATA_RESERVE));
    LogWriter::maintain_workspace(workspace.path().into(), config)
        .await
        .unwrap();
    assert_eq!(quota::load(&root).unwrap(), 8 * (12 + METADATA_RESERVE));
}

#[cfg(unix)]
#[test]
fn nonregular_metadata_is_rejected_before_opening_a_fifo() {
    let directory = tempfile::tempdir().unwrap();
    let metadata = directory.path().join("metadata.json");
    assert!(std::process::Command::new("mkfifo")
        .arg(&metadata)
        .status()
        .unwrap()
        .success());
    assert!(load_record(directory.path())
        .unwrap_err()
        .is_code(ErrorCode::PermissionDenied));
}
