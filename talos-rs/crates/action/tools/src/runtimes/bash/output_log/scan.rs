//! Resumable, bounded in-memory traversal. Quota remains an independent ledger.
use super::*;
use std::time::{Duration, Instant};
use walk::{Item, Walk};

/// One Workspace's complete round. Owns directory iterators across batches;
/// dropping/restarting discards only progress, never committed log state.
pub struct BashOutputMaintenance {
    root: PathBuf,
    walk: Option<Walk>,
    phase: Phase,
    stamp: Option<quota::Snapshot>,
    sum: u64,
    healthy: bool,
    pruned: bool,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Start,
    Bootstrap,
    Cleanup,
    Audit,
    Prune,
    Complete,
}

/// Delta for one safe batch. Errors are isolated to records; the caller reports
/// them with Workspace context and continues other records/Workspaces.
pub struct OutputLogMaintenanceStep {
    pub stats: OutputLogMaintenanceStats,
    pub errors: Vec<TalosError>,
    pub complete: bool,
    pub accounting_deferred: bool,
}
impl BashOutputMaintenance {
    pub fn new(workspace: PathBuf) -> Self {
        Self {
            root: workspace.join("tool-output"),
            walk: None,
            phase: Phase::Start,
            stamp: None,
            sum: 0,
            healthy: true,
            pruned: false,
        }
    }

    /// Filesystem work owns the scan until its batch is safe, including when an
    /// awaiting caller is cancelled. No detached task can release a lock early.
    pub async fn advance(
        mut self,
        config: BashOutputLogConfig,
        budget: Duration,
    ) -> TalosResult<(Self, OutputLogMaintenanceStep)> {
        tokio::task::spawn_blocking(move || {
            let result = self.batch(&config, budget);
            result.map(|step| (self, step))
        })
        .await
        .map_err(|error| {
            TalosError::internal_with_source("Bash log maintenance batch failed.", error)
        })?
    }

    fn begin_readonly(&mut self, phase: Phase) {
        self.phase = phase;
        self.stamp = quota::snapshot(&self.root).ok();
        self.sum = 0;
        self.healthy = true;
        self.walk = Some(Walk::new(self.root.clone()));
    }
    fn begin_cleanup(&mut self) {
        self.phase = Phase::Cleanup;
        self.walk = Some(Walk::new(self.root.clone()));
    }
    fn batch(
        &mut self,
        config: &BashOutputLogConfig,
        budget: Duration,
    ) -> TalosResult<OutputLogMaintenanceStep> {
        let deadline = Instant::now() + budget;
        let mut step = OutputLogMaintenanceStep {
            stats: OutputLogMaintenanceStats::default(),
            errors: Vec::new(),
            complete: false,
            accounting_deferred: false,
        };
        private_directory(&self.root)?;
        // Background work yields immediately when writers own the lock. Its
        // lock wait cannot consume a Workspace or global scheduling budget.
        let Some(_lock) = try_private_lock(&self.root.join("workspace.lock"))? else {
            return Ok(step);
        };
        if self.phase == Phase::Start {
            if quota::snapshot(&self.root).is_ok() {
                self.begin_cleanup();
            } else {
                self.begin_readonly(Phase::Bootstrap);
            }
        }
        if let (Phase::Cleanup | Phase::Prune, Ok(used)) = (self.phase, quota::load(&self.root)) {
            // Covers directory pruning and metadata changes before they happen,
            // even if a later filesystem/counter operation is interrupted.
            quota::save(&self.root, used)?;
        }
        let mut records = 0;
        while step.stats.scanned_entries < MAX_LOG_DIRECTORIES
            && records < 128
            && step.errors.len() < 128
            && (step.stats.scanned_entries == 0 || Instant::now() < deadline)
        {
            let item = self
                .walk
                .as_mut()
                .expect("running scan has an iterator")
                .next();
            match item {
                Ok(Item::Complete) => {
                    match self.phase {
                        Phase::Bootstrap | Phase::Audit => {
                            let current = quota::snapshot(&self.root).ok();
                            if self.healthy && current == self.stamp {
                                quota::save(&self.root, self.sum)?;
                            } else {
                                step.accounting_deferred = true;
                            }
                            if self.phase == Phase::Bootstrap {
                                self.begin_cleanup();
                            } else {
                                self.phase = Phase::Prune;
                                self.walk = Some(Walk::pruning(self.root.clone()));
                                self.pruned = false;
                            }
                        }
                        Phase::Cleanup => self.begin_readonly(Phase::Audit),
                        Phase::Prune if self.pruned => {
                            self.walk = Some(Walk::pruning(self.root.clone()));
                            self.pruned = false;
                        }
                        Phase::Prune => self.phase = Phase::Complete,
                        Phase::Start | Phase::Complete => unreachable!("only active scans advance"),
                    }
                    if self.phase == Phase::Complete {
                        step.complete = true;
                        break;
                    }
                    // Never start cleanup in a batch that began as read-only:
                    // the next batch stamps its mutations before pruning.
                    break;
                }
                Ok(Item::Visited) => step.stats.scanned_entries += 1,
                Ok(Item::Pruned) => {
                    self.pruned = true;
                    step.stats.scanned_entries += 1;
                }
                Ok(Item::Log(directory)) => {
                    step.stats.scanned_entries += 1;
                    records += 1;
                    let result = match self.phase {
                        Phase::Cleanup => self.clean_record(&directory, config, &mut step.stats),
                        Phase::Prune => maintenance::remove_if_empty(&directory).map(|removed| {
                            self.pruned |= removed;
                        }),
                        _ => self.count_record(&directory),
                    };
                    if let Err(error) = result {
                        self.healthy = false;
                        step.errors.push(error);
                    }
                }
                Err(error) => {
                    step.stats.scanned_entries += 1;
                    self.healthy = false;
                    step.errors.push(error);
                }
            }
        }
        Ok(step)
    }

    fn record(&self, directory: &Path) -> TalosResult<Option<Record>> {
        if !directory
            .join("metadata.json")
            .try_exists()
            .map_err(io_error)?
        {
            return Ok(None);
        }
        let record = load_record(directory)?;
        let relative = directory
            .strip_prefix(self.root.parent().expect("output root has a parent"))
            .map_err(|_| TalosError::internal("Bash log is outside its Workspace."))?
            .join("output.log");
        if record.version != 1
            || Path::new(&record.log.relative_path) != relative
            || record.reservation_bytes < METADATA_RESERVE
        {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Bash log ownership or charge is invalid.",
            ));
        }
        Ok(Some(record))
    }
    fn size(&self, directory: &Path) -> TalosResult<u64> {
        match fs::symlink_metadata(directory.join("output.log")) {
            Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
                Ok(metadata.len())
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
            Ok(_) => Err(TalosError::new(
                ErrorCode::PermissionDenied,
                "Bash output must be a regular file.",
            )),
            Err(error) => Err(io_error(error)),
        }
    }
    fn count_record(&mut self, directory: &Path) -> TalosResult<()> {
        let Some(record) = self.record(directory)? else {
            return Ok(());
        };
        if self.size(directory)?.saturating_add(METADATA_RESERVE) > record.reservation_bytes {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Bash output exceeds its recorded reservation.",
            ));
        }
        // The total and later per-file deletion must use the SAME charge. A
        // terminal unknown-size reservation cannot silently shrink only here.
        self.sum = self
            .sum
            .checked_add(record.reservation_bytes)
            .ok_or_else(|| TalosError::internal("Bash output accounting overflowed."))?;
        Ok(())
    }
    fn clean_record(
        &mut self,
        directory: &Path,
        config: &BashOutputLogConfig,
        stats: &mut OutputLogMaintenanceStats,
    ) -> TalosResult<()> {
        if !directory
            .join("metadata.json")
            .try_exists()
            .map_err(io_error)?
        {
            return Ok(());
        }
        let Some(_capture) = try_private_lock(&directory.join("capture.lock"))? else {
            stats.active_logs += 1;
            return Ok(());
        };
        let Some(mut record) = self.record(directory)? else {
            return Ok(());
        };
        let size = self.size(directory)?;
        let actual = size.saturating_add(METADATA_RESERVE);
        if actual > record.reservation_bytes {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Bash output exceeds its recorded reservation.",
            ));
        }
        let trusted = quota::load(&self.root).ok();
        let recovered = record.log.capture_status == CaptureStatus::Capturing;
        if recovered {
            stats.recovered_logs += 1;
            record.log.capture_status = CaptureStatus::Interrupted;
            record.log.total_bytes = record.log.total_bytes.max(size);
            record.log.expires_at = now().saturating_add(i64::from(config.retention_days) * 86400);
        }
        if record.cleanup != CleanupState::Deleting
            && (record.reservation_bytes != actual || record.log.stored_bytes != size || recovered)
        {
            let old_charge = record.reservation_bytes;
            record.reservation_bytes = actual;
            record.log.stored_bytes = size;
            save_record(directory, &record)?;
            if trusted.is_some() {
                quota::release(&self.root, old_charge, actual)?;
            }
        }
        let pressure = quota::load(&self.root).is_ok_and(|used| {
            used.saturating_add(config.max_file_bytes.saturating_add(METADATA_RESERVE))
                > config.max_workspace_bytes
        });
        if record.cleanup == CleanupState::Deleting || record.log.expires_at <= now() || pressure {
            let charge = record.reservation_bytes;
            // Keep the empty directory until all TTL/accounting traversal is
            // finished. Removing entries from a retained ReadDir can skip later
            // records on filesystems such as APFS.
            maintenance::remove_log_files(directory, &mut record)?;
            // Ownership metadata is gone before decrement: a crash can leak
            // charge but cannot replay this deletion and release another log.
            if trusted.is_some() {
                quota::release(&self.root, charge, 0)?;
            }
            stats.deleted_logs += 1;
            stats.reclaimed_bytes += size;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "scan_tests.rs"]
mod tests;
