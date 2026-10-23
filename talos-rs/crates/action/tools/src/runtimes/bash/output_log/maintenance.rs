//! Bounded Workspace maintenance under the cross-process root lock.
use super::*;

fn remove_log_directory(directory: &Path, record: &mut Record) -> TalosResult<()> {
    // Validate before removing any owned data. Unexpected files require repair,
    // not a half-deleted log whose ownership metadata has already disappeared.
    for entry in fs::read_dir(directory).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        if !matches!(
            entry.file_name().to_str(),
            Some("output.log" | "metadata.json" | "capture.lock")
        ) {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Bash log cleanup found an unexpected directory entry.",
            ));
        }
    }
    if record.cleanup != CleanupState::Deleting {
        record.cleanup = CleanupState::Deleting;
        save_record(directory, record)?;
    }
    // Keep ownership metadata until the body is gone. A crash must not leave
    // an unidentifiable large log behind. Never recursively remove extra files.
    for name in ["output.log", "capture.lock", "metadata.json"] {
        match fs::remove_file(directory.join(name)) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(io_error(error)),
        }
    }
    fs::remove_dir(directory).map_err(io_error)?;
    // Empty Turn/Session directories must not eventually exhaust the bounded
    // maintenance scan after many short-lived invocations.
    let mut parent = directory.parent();
    for _ in 0..2 {
        let Some(path) = parent else {
            break;
        };
        match fs::remove_dir(path) {
            Ok(()) => parent = path.parent(),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::DirectoryNotEmpty | std::io::ErrorKind::NotFound
                ) =>
            {
                break
            }
            Err(error) => return Err(io_error(error)),
        }
    }
    Ok(())
}

fn log_directories(
    root: &Path,
    depth: usize,
    result: &mut Vec<PathBuf>,
    visited: &mut usize,
) -> TalosResult<bool> {
    for entry in fs::read_dir(root).map_err(io_error)? {
        *visited += 1;
        if *visited > MAX_LOG_DIRECTORIES {
            return Ok(false);
        }
        let entry = entry.map_err(io_error)?;
        let kind = entry.file_type().map_err(io_error)?;
        if !kind.is_dir() {
            continue;
        }
        if depth == 1 {
            result.push(entry.path());
        } else {
            if !log_directories(&entry.path(), depth - 1, result, visited)? {
                return Ok(false);
            }
            // A crash after the last unlink may leave empty ancestors behind.
            // rmdir cannot remove data, even without ownership metadata.
            remove_if_empty(&entry.path())?;
        }
    }
    Ok(true)
}

/// Called with the Workspace lock held. Active captures retain their complete
/// reservation; dead Capturing records recover from the saved file size.
pub(super) fn maintain(
    root: &Path,
    config: &BashOutputLogConfig,
    reserve: u64,
) -> TalosResult<OutputLogMaintenanceStats> {
    let mut stats = OutputLogMaintenanceStats::default();
    let mut directories = Vec::new();
    let scan_complete = log_directories(root, 3, &mut directories, &mut stats.scanned_entries)?;
    let mut used = 0_u64;
    let mut terminal = Vec::new();
    for directory in directories {
        // Unknown directories are not owned just because their names look right.
        if !directory.join("metadata.json").exists() {
            remove_if_empty(&directory)?;
            continue;
        }
        // Acquire first: finalization can replace Capturing with Complete just
        // before releasing this lock. Reading first could misclassify a clean
        // completion as an orphan using a stale record.
        let lock = try_private_lock(&directory.join("capture.lock"))?;
        let mut record = load_record(&directory)?;
        let relative = directory
            .strip_prefix(root.parent().expect("output root has a parent"))
            .map_err(|_| TalosError::internal("Bash log is outside its Workspace."))?
            .join("output.log");
        if record.version != 1 || Path::new(&record.log.relative_path) != relative {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Bash log ownership metadata does not match its location.",
            ));
        }
        let Some(lock) = lock else {
            stats.active_logs += 1;
            used = used.saturating_add(record.reservation_bytes);
            continue;
        };
        if record.cleanup == CleanupState::Deleting {
            stats.reclaimed_bytes += fs::symlink_metadata(directory.join("output.log"))
                .map(|m| m.len())
                .unwrap_or(0);
            remove_log_directory(&directory, &mut record)?;
            stats.deleted_logs += 1;
            continue;
        }
        let output = directory.join("output.log");
        let size = match fs::symlink_metadata(&output) {
            Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
                metadata.len()
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
            Ok(_) => {
                return Err(TalosError::new(
                    ErrorCode::PermissionDenied,
                    "Bash output must be a regular file.",
                ))
            }
            Err(error) => return Err(io_error(error)),
        };
        if record.log.capture_status == CaptureStatus::Capturing {
            stats.recovered_logs += 1;
            record.log.capture_status = CaptureStatus::Interrupted;
            record.log.stored_bytes = size;
            record.log.total_bytes = record.log.total_bytes.max(size);
            record.log.expires_at = now().saturating_add(i64::from(config.retention_days) * 86400);
            record.reservation_bytes = size.saturating_add(METADATA_RESERVE);
            save_record(&directory, &record)?;
        }
        let bytes = size.saturating_add(METADATA_RESERVE);
        if record.log.expires_at <= now() {
            remove_log_directory(&directory, &mut record)?;
            stats.deleted_logs += 1;
            stats.reclaimed_bytes += size;
        } else {
            used = used.saturating_add(bytes);
            terminal.push((record.log.expires_at, directory, bytes));
        }
        drop(lock);
    }
    terminal.sort_by_key(|(expires, _, _)| *expires);
    let mut removed = 0;
    let reserve = reserve.max(config.max_file_bytes.saturating_add(METADATA_RESERVE));
    for (_, directory, bytes) in terminal {
        // A saturated directory scan must also make progress before TTL. Evict
        // a bounded batch of owned terminal logs, then require a complete scan
        // before granting any reservation. Active and unknown files stay intact.
        if (scan_complete && used.saturating_add(reserve) <= config.max_workspace_bytes)
            || (!scan_complete && removed >= 128)
        {
            break;
        }
        if let Some(_lock) = try_private_lock(&directory.join("capture.lock"))? {
            let mut record = load_record(&directory)?;
            remove_log_directory(&directory, &mut record)?;
            used = used.saturating_sub(bytes);
            removed += 1;
            stats.deleted_logs += 1;
            stats.reclaimed_bytes += bytes.saturating_sub(METADATA_RESERVE);
        }
    }
    if !scan_complete {
        return Err(TalosError::new(ErrorCode::ServiceUnavailable, "Bash output maintenance scan limit reached; owned terminal logs were reclaimed when available. Retry maintenance."));
    }
    quota::save(root, used)?;
    if used > config.max_workspace_bytes {
        return Err(TalosError::new(
            ErrorCode::UsageLimitExceeded,
            "Bash output Workspace quota is occupied by active captures.",
        ));
    }
    Ok(stats)
}

fn remove_if_empty(path: &Path) -> TalosResult<()> {
    // Unknown nonempty directories are not cleanup candidates. In particular,
    // attempting rmdir under a read-only foreign parent can fail with EACCES
    // before the OS reports ENOTEMPTY and would block unrelated owned cleanup.
    let mut entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(io_error(error)),
    };
    if entries.next().transpose().map_err(io_error)?.is_some() {
        return Ok(());
    }
    match fs::remove_dir(path) {
        Ok(()) => Ok(()),
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::DirectoryNotEmpty | std::io::ErrorKind::NotFound
            ) =>
        {
            Ok(())
        }
        Err(error) => Err(io_error(error)),
    }
}
