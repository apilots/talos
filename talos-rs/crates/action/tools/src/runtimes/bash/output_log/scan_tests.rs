use super::*;

fn config() -> BashOutputLogConfig {
    BashOutputLogConfig {
        max_file_bytes: 32,
        max_workspace_bytes: 64 * 1024 * 1024,
        ..BashOutputLogConfig::default()
    }
}
fn seed(workspace: &Path, count: usize) -> u64 {
    let root = workspace.join("tool-output");
    fs::create_dir_all(root.join("session/turn")).unwrap();
    for index in 0..count {
        let directory = root.join(format!("session/turn/{index}"));
        fs::create_dir(&directory).unwrap();
        let path = directory.join("output.log");
        fs::write(&path, b"saved output").unwrap();
        save_record(
            &directory,
            &Record {
                version: 1,
                reservation_bytes: 12 + METADATA_RESERVE,
                cleanup: CleanupState::Retained,
                log: BashOutputLog {
                    path: path.to_string_lossy().into_owned(),
                    relative_path: path
                        .strip_prefix(workspace)
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                    total_bytes: 12,
                    stored_bytes: 12,
                    total_lines: 1,
                    capture_status: CaptureStatus::Complete,
                    expires_at: i64::MAX,
                },
            },
        )
        .unwrap();
    }
    let total = count as u64 * (12 + METADATA_RESERVE);
    quota::save(&root, total).unwrap();
    total
}
fn start_audit(scan: &mut BashOutputMaintenance) {
    while scan.phase != Phase::Audit {
        let step = scan.batch(&config(), Duration::from_secs(60)).unwrap();
        assert!(step.errors.is_empty());
        assert!(step.stats.scanned_entries <= MAX_LOG_DIRECTORIES);
    }
    let step = scan.batch(&config(), Duration::from_secs(60)).unwrap();
    assert!(!step.complete);
    assert!(scan.sum > 0);
}
fn finish(scan: &mut BashOutputMaintenance) -> bool {
    let mut deferred = false;
    loop {
        let step = scan.batch(&config(), Duration::from_secs(60)).unwrap();
        assert!(step.errors.is_empty());
        deferred |= step.accounting_deferred;
        if step.complete {
            return deferred;
        }
    }
}

#[test]
fn readonly_partial_audit_does_not_overwrite_aba_or_live_reservations() {
    let workspace = tempfile::tempdir().unwrap();
    let total = seed(workspace.path(), 300);
    let root = workspace.path().join("tool-output");
    let mut scan = BashOutputMaintenance::new(workspace.path().into());
    start_audit(&mut scan);
    // A reserve/release pair returns to the same usage, but its revision changes.
    let before = quota::snapshot(&root).unwrap();
    {
        let _lock = workspace_lock(&root).unwrap();
        quota::reserve(&root, false, 100, config().max_workspace_bytes).unwrap();
        quota::release(&root, 100, 0).unwrap();
    }
    let after = quota::snapshot(&root).unwrap();
    assert_eq!(before.used_bytes, after.used_bytes);
    assert_ne!(before, after);
    assert!(finish(&mut scan));
    assert_eq!(quota::load(&root).unwrap(), total);

    let mut scan = BashOutputMaintenance::new(workspace.path().into());
    start_audit(&mut scan);
    {
        let _lock = workspace_lock(&root).unwrap();
        quota::reserve(&root, false, 100, config().max_workspace_bytes).unwrap();
    }
    assert!(finish(&mut scan));
    assert_eq!(quota::load(&root).unwrap(), total + 100);
    // Once stable, a complete read-only audit reclaims a crash-leaked reservation.
    let mut scan = BashOutputMaintenance::new(workspace.path().into());
    assert!(!finish(&mut scan));
    assert_eq!(quota::load(&root).unwrap(), total);
}

#[test]
fn deletion_crash_gap_retains_charge_until_a_complete_audit() {
    let workspace = tempfile::tempdir().unwrap();
    let total = seed(workspace.path(), 2);
    let root = workspace.path().join("tool-output");
    let deleted = root.join("session/turn/0");
    let mut record = load_record(&deleted).unwrap();
    {
        let _lock = workspace_lock(&root).unwrap();
        quota::save(&root, total).unwrap();
        maintenance::remove_log_files(&deleted, &mut record).unwrap();
        // Process stops before quota::release; ownership is already gone.
    }
    assert_eq!(quota::load(&root).unwrap(), total);
    let mut scan = BashOutputMaintenance::new(workspace.path().into());
    assert!(!finish(&mut scan));
    assert_eq!(quota::load(&root).unwrap(), total / 2);
    let mut scan = BashOutputMaintenance::new(workspace.path().into());
    finish(&mut scan);
    assert_eq!(quota::load(&root).unwrap(), total / 2);
}

#[test]
fn malformed_entry_does_not_reset_the_iterator_or_block_later_expiry() {
    let workspace = tempfile::tempdir().unwrap();
    let total = seed(workspace.path(), 300);
    let root = workspace.path().join("tool-output");
    fs::write(root.join("session/turn/0/metadata.json"), b"invalid").unwrap();
    for index in 1..300 {
        let directory = root.join(format!("session/turn/{index}"));
        let mut record = load_record(&directory).unwrap();
        record.log.expires_at = 0;
        save_record(&directory, &record).unwrap();
    }
    let mut scan = BashOutputMaintenance::new(workspace.path().into());
    let mut deleted = 0;
    let mut errors = 0;
    loop {
        let step = scan.batch(&config(), Duration::from_secs(60)).unwrap();
        deleted += step.stats.deleted_logs;
        errors += step.errors.len();
        assert!(step.errors.len() <= 128);
        if step.complete {
            break;
        }
    }
    assert_eq!(deleted, 299);
    assert!(errors > 0);
    assert!(root.join("session/turn/0/metadata.json").exists());
    assert_eq!(quota::load(&root).unwrap(), total / 300);
}
