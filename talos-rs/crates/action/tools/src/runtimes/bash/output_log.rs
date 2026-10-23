//! Bash-owned durable files. These are not Provider transcript artifacts.
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use talos_config::BashOutputLogConfig;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_fs::{FileLock, FilePermissions, FileSystem, WriteOptions};
use talos_types::talos::{BashOutputCaptureStatus as CaptureStatus, BashOutputLog};
use tokio::io::AsyncWriteExt;

const METADATA_RESERVE: u64 = 4096;
const MAX_LOG_DIRECTORIES: usize = 4096;

/// Summary of maintenance batches or one complete round on a user-scoped execution Workspace.
#[derive(Debug, Default)]
pub struct OutputLogMaintenanceStats {
    pub scanned_entries: usize,
    pub deleted_logs: usize,
    pub reclaimed_bytes: u64,
    pub recovered_logs: usize,
    pub active_logs: usize,
}

impl OutputLogMaintenanceStats {
    pub fn merge(&mut self, other: &Self) {
        self.scanned_entries += other.scanned_entries;
        self.deleted_logs += other.deleted_logs;
        self.reclaimed_bytes += other.reclaimed_bytes;
        self.recovered_logs += other.recovered_logs;
        self.active_logs += other.active_logs;
    }
}

/// Run only from the execution host lifecycle, never from a Bash invocation.
/// The caller owns configuration, scheduling and error reporting; this operation
/// validates persisted ownership and serializes maintenance with writer admission.
pub async fn maintain_bash_output_workspace(
    workspace: PathBuf,
    config: BashOutputLogConfig,
) -> TalosResult<OutputLogMaintenanceStats> {
    LogWriter::maintain_workspace(workspace, config).await
}

mod creation;
mod maintenance;
mod quota;
mod scan;
mod walk;
pub use scan::{BashOutputMaintenance, OutputLogMaintenanceStep};

#[derive(Debug, Serialize, Deserialize)]
struct Record {
    version: u8,
    reservation_bytes: u64,
    #[serde(default)]
    cleanup: CleanupState,
    log: BashOutputLog,
}

#[derive(Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CleanupState {
    #[default]
    Retained,
    Deleting,
}

pub(super) struct LogCompletion {
    pub log: BashOutputLog,
    pub error: Option<TalosError>,
}

pub(super) struct LogWriter {
    file: tokio::fs::File,
    directory: PathBuf,
    record: Record,
    limit: u64,
    reserved_bytes: u64,
    retention_days: u32,
    _capture_lock: FileLock,
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .min(i64::MAX as u64) as i64
}

fn io_error(error: std::io::Error) -> TalosError {
    TalosError::internal_with_source(
        "Bash output log I/O failed; command execution must not be retried automatically.",
        error,
    )
}

fn workspace_lock(root: &Path) -> TalosResult<FileLock> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        if let Some(lock) = try_private_lock(&root.join("workspace.lock"))? {
            return Ok(lock);
        }
        if std::time::Instant::now() >= deadline {
            return Err(TalosError::new(
                ErrorCode::ServiceUnavailable,
                "Bash output maintenance is busy.",
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn try_private_lock(path: &Path) -> TalosResult<Option<FileLock>> {
    if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(TalosError::new(
            ErrorCode::PermissionDenied,
            "Bash log locks cannot be symbolic links.",
        ));
    }
    let lock = talos_fs::std_fs().try_file_lock(path)?;
    #[cfg(unix)]
    if lock.is_some() {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(io_error)?;
    }
    Ok(lock)
}

fn private_directory(path: &Path) -> TalosResult<bool> {
    let mut created = false;
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(TalosError::new(
                ErrorCode::PermissionDenied,
                "Bash output directory must be a regular directory.",
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(io_error)?;
        }
    } else {
        let mut options = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            options.mode(0o700);
        }
        match options.create(path) {
            Ok(()) => created = true,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                return private_directory(path)
            }
            Err(error) => return Err(io_error(error)),
        }
    }
    Ok(created)
}

fn save_record(directory: &Path, record: &Record) -> TalosResult<()> {
    // Atomic replacement temporarily owns both the old and new pretty JSON.
    if serde_json::to_vec_pretty(record)
        .map_err(|error| {
            TalosError::internal_with_source("Bash metadata serialization failed.", error)
        })?
        .len()
        > METADATA_RESERVE as usize / 2
    {
        return Err(TalosError::new(
            ErrorCode::InvalidRequest,
            "Bash log identity metadata exceeds its reserved space.",
        ));
    }
    talos_fs::std_fs().write_json_pretty_with(
        &directory.join("metadata.json"),
        record,
        WriteOptions::default().with_permissions(FilePermissions::unix(0o600)),
    )
}

fn load_record(directory: &Path) -> TalosResult<Record> {
    let path = directory.join("metadata.json");
    let metadata = fs::symlink_metadata(&path).map_err(io_error)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(TalosError::new(
            ErrorCode::PermissionDenied,
            "Bash log metadata must be a regular file.",
        ));
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(io_error)?
        .take(METADATA_RESERVE + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() as u64 > METADATA_RESERVE {
        return Err(TalosError::new(
            ErrorCode::InvalidRequest,
            "Bash log metadata exceeds its limit.",
        ));
    }
    serde_json::from_slice(&bytes)
        .map_err(|error| TalosError::internal_with_source("Bash log metadata is invalid.", error))
}

impl LogWriter {
    pub async fn maintain_workspace(
        workspace: PathBuf,
        config: BashOutputLogConfig,
    ) -> TalosResult<OutputLogMaintenanceStats> {
        let mut scan = BashOutputMaintenance::new(workspace);
        let mut stats = OutputLogMaintenanceStats::default();
        let mut first_error = None;
        loop {
            let (next, step) = scan
                .advance(config.clone(), std::time::Duration::from_millis(250))
                .await?;
            scan = next;
            if step.stats.scanned_entries == 0 && !step.complete && step.errors.is_empty() {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
            stats.merge(&step.stats);
            for error in step.errors {
                retain_failure(&mut first_error, error);
            }
            if step.complete {
                break;
            }
            tokio::task::yield_now().await;
        }
        if let Some(error) = first_error {
            return Err(error);
        }
        Ok(stats)
    }

    pub async fn open(
        workspace: PathBuf,
        session: String,
        turn: String,
        call: String,
        config: BashOutputLogConfig,
    ) -> TalosResult<Self> {
        tokio::task::spawn_blocking(move || {
            let root = workspace.join("tool-output");
            let fresh = private_directory(&root)?;
            let _workspace_lock = quota::admission_lock(&root, fresh)?;
            let reserve = config.max_file_bytes.saturating_add(METADATA_RESERVE);
            quota::reserve(&root, fresh, reserve, config.max_workspace_bytes)?;
            let mut created_directory = None;
            let result = (|| {
                let session = root.join(crate::state::sha256_hex(session.as_bytes()));
                private_directory(&session)?;
                let turn = session.join(crate::state::sha256_hex(turn.as_bytes()));
                private_directory(&turn)?;
                let directory = turn.join(format!(
                    "{}-{}",
                    crate::state::sha256_hex(call.as_bytes()),
                    uuid::Uuid::new_v4()
                ));
                private_directory(&directory)?;
                created_directory = Some(directory.clone());
                let capture_lock =
                    try_private_lock(&directory.join("capture.lock"))?.ok_or_else(|| {
                        TalosError::internal("New Bash log identity is already locked.")
                    })?;
                let path = directory.join("output.log");
                let relative_path = path
                    .strip_prefix(&workspace)
                    .map_err(|_| TalosError::internal("Bash output escaped its Workspace."))?
                    .to_string_lossy()
                    .into_owned();
                let mut options = OpenOptions::new();
                options.write(true).create_new(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.mode(0o600);
                }
                let file = options.open(&path).map_err(io_error)?;
                let record = Record {
                    version: 1,
                    reservation_bytes: reserve,
                    cleanup: CleanupState::Retained,
                    log: BashOutputLog {
                        path: path.to_string_lossy().into_owned(),
                        relative_path,
                        total_bytes: 0,
                        stored_bytes: 0,
                        total_lines: 0,
                        capture_status: CaptureStatus::Capturing,
                        expires_at: now().saturating_add(i64::from(config.retention_days) * 86400),
                    },
                };
                save_record(&directory, &record)?;
                Ok(Self {
                    file: tokio::fs::File::from_std(file),
                    directory,
                    record,
                    limit: config.max_file_bytes,
                    reserved_bytes: reserve,
                    retention_days: config.retention_days,
                    _capture_lock: capture_lock,
                })
            })();
            if result.is_err() {
                // No output can be written before setup returns a writer. Only
                // release after known partial files have been safely removed.
                let rollback = creation::rollback(created_directory.as_deref())
                    .and_then(|()| quota::release(&root, reserve, 0));
                if let Err(failure) = rollback {
                    talos_logging::report_error(&failure);
                }
            }
            result
        })
        .await
        .map_err(|error| {
            TalosError::internal_with_source("Bash log creation task failed.", error)
        })?
    }

    pub async fn append(&mut self, bytes: &[u8]) -> TalosResult<()> {
        let remaining = self
            .limit
            .saturating_sub(self.record.log.stored_bytes)
            .min(bytes.len() as u64) as usize;
        if let Err(error) = self.file.write_all(&bytes[..remaining]).await {
            self.record.log.capture_status = CaptureStatus::WriteFailed;
            return Err(io_error(error));
        }
        self.record.log.stored_bytes += remaining as u64;
        if remaining < bytes.len() {
            self.record.log.capture_status = CaptureStatus::QuotaLimited;
        }
        Ok(())
    }

    pub fn accepts_output(&self) -> bool {
        self.record.log.capture_status != CaptureStatus::WriteFailed
    }

    pub async fn finish(
        mut self,
        total_bytes: u64,
        total_lines: u64,
        interrupted: bool,
    ) -> LogCompletion {
        let mut error = self.file.flush().await.err().map(io_error);
        if error.is_none() {
            error = self.file.sync_all().await.err().map(io_error);
        }
        // Tokio may have accepted a write into its buffer before the OS failed.
        // Publish confirmed file bytes, not the number accepted by write_all.
        let size = self.file.metadata().await.map(|metadata| metadata.len());
        self.confirm_stored_size(size, &mut error);
        self.record.log.total_bytes = total_bytes;
        self.record.log.total_lines = total_lines;
        if error.is_some() {
            self.record.log.capture_status = CaptureStatus::WriteFailed;
        } else if self.record.log.capture_status == CaptureStatus::Capturing {
            self.record.log.capture_status = if interrupted {
                CaptureStatus::Interrupted
            } else {
                CaptureStatus::Complete
            };
        }
        self.record.log.expires_at = now().saturating_add(i64::from(self.retention_days) * 86400);
        let mut fallback = self.record.log.clone();
        // Consume the entire writer in the blocking operation. If the awaiting
        // caller is cancelled, its capture lock still spans metadata commit.
        match tokio::task::spawn_blocking(move || self.commit(error)).await {
            Ok(completion) => completion,
            Err(failure) => {
                fallback.capture_status = CaptureStatus::WriteFailed;
                LogCompletion {
                    log: fallback,
                    error: Some(TalosError::internal_with_source(
                        "Bash log finalization task failed.",
                        failure,
                    )),
                }
            }
        }
    }

    fn confirm_stored_size(&mut self, size: std::io::Result<u64>, error: &mut Option<TalosError>) {
        match size {
            Ok(bytes) => {
                self.record.log.stored_bytes = bytes;
                self.record.reservation_bytes = bytes.saturating_add(METADATA_RESERVE);
            }
            Err(failure) => {
                self.record.log.stored_bytes = 0;
                // Unknown size is not empty. Retain the original reservation
                // until maintenance can inspect the terminal file successfully.
                self.record.reservation_bytes = self.reserved_bytes;
                retain_failure(error, io_error(failure));
            }
        }
    }

    fn commit(self, mut error: Option<TalosError>) -> LogCompletion {
        let mut log = self.record.log.clone();
        let root = self
            .directory
            .parent()
            .and_then(Path::parent)
            .and_then(Path::parent)
            .expect("log has Session/Turn/call directories");
        let result = (|| {
            let _lock = workspace_lock(root)?;
            quota::save(root, quota::load(root)?)?;
            save_record(&self.directory, &self.record)?;
            quota::release(root, self.reserved_bytes, self.record.reservation_bytes)
        })();
        if let Err(failure) = result {
            log.capture_status = CaptureStatus::WriteFailed;
            retain_failure(&mut error, failure);
        }
        LogCompletion { log, error }
    }
}

fn retain_failure(first: &mut Option<TalosError>, failure: TalosError) {
    if first.is_some() {
        // The primary error is reported with the command result. Otherwise-lost
        // secondary finalization failures have this handling owner.
        talos_logging::report_error(&failure);
    } else {
        *first = Some(failure);
    }
}

#[cfg(test)]
#[path = "output_log_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "output_log/quota_tests.rs"]
mod quota_tests;
