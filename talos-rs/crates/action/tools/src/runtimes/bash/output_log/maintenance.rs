//! Bounded Workspace maintenance under the cross-process root lock.
use super::*;

pub(super) fn remove_log_files(directory: &Path, record: &mut Record) -> TalosResult<()> {
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
    Ok(())
}

pub(super) fn remove_if_empty(path: &Path) -> TalosResult<bool> {
    // Unknown nonempty directories are not cleanup candidates. In particular,
    // attempting rmdir under a read-only foreign parent can fail with EACCES
    // before the OS reports ENOTEMPTY and would block unrelated owned cleanup.
    let mut entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(io_error(error)),
    };
    if entries.next().transpose().map_err(io_error)?.is_some() {
        return Ok(false);
    }
    match fs::remove_dir(path) {
        Ok(()) => Ok(true),
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::DirectoryNotEmpty | std::io::ErrorKind::NotFound
            ) =>
        {
            Ok(false)
        }
        Err(error) => Err(io_error(error)),
    }
}
