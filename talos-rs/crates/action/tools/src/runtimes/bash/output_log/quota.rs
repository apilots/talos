//! Constant-size cross-process admission; only maintenance reconstructs usage.
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Snapshot {
    version: u8,
    pub used_bytes: u64,
    revision: u64,
}

pub(super) fn admission_lock(root: &Path, fresh: bool) -> TalosResult<FileLock> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        let lock = workspace_lock(root)?;
        if fresh || root.join("quota.json").try_exists().map_err(io_error)? {
            return Ok(lock);
        }
        // A concurrent creator or the Worker's initial pass may be preparing
        // accounting. Never hold its lock while waiting, and never scan here.
        drop(lock);
        if std::time::Instant::now() >= deadline {
            return Err(TalosError::new(
                ErrorCode::ServiceUnavailable,
                "Bash output accounting awaits Worker maintenance.",
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

pub(super) fn load(root: &Path) -> TalosResult<u64> {
    Ok(snapshot(root)?.used_bytes)
}

pub(super) fn snapshot(root: &Path) -> TalosResult<Snapshot> {
    let path = root.join("quota.json");
    let metadata = fs::symlink_metadata(&path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            TalosError::new(
                ErrorCode::ServiceUnavailable,
                "Bash output accounting awaits Worker maintenance.",
            )
        } else {
            io_error(error)
        }
    })?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > METADATA_RESERVE
    {
        return Err(TalosError::new(
            ErrorCode::InvalidRequest,
            "Bash output accounting must be a bounded regular file.",
        ));
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(io_error)?
        .take(METADATA_RESERVE + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    let value: Snapshot = serde_json::from_slice(&bytes).map_err(|error| {
        TalosError::internal_with_source("Bash output accounting is invalid.", error)
    })?;
    if value.version != 2 {
        return Err(TalosError::new(
            ErrorCode::ServiceUnavailable,
            "Bash output accounting requires Worker recovery.",
        ));
    }
    Ok(value)
}

pub(super) fn save(root: &Path, used_bytes: u64) -> TalosResult<()> {
    // Reject links even though atomic replacement itself never follows one.
    if fs::symlink_metadata(root.join("quota.json"))
        .is_ok_and(|metadata| !metadata.is_file() || metadata.file_type().is_symlink())
    {
        return Err(TalosError::new(
            ErrorCode::PermissionDenied,
            "Bash output accounting cannot replace a non-regular file.",
        ));
    }
    let revision = snapshot(root)
        .map_or(0, |value| value.revision)
        .checked_add(1)
        .ok_or_else(|| TalosError::internal("Bash output accounting revision overflowed."))?;
    talos_fs::std_fs().write_json_pretty_with(
        &root.join("quota.json"),
        &Snapshot {
            version: 2,
            used_bytes,
            revision,
        },
        WriteOptions::default().with_permissions(FilePermissions::unix(0o600)),
    )
}

pub(super) fn reserve(root: &Path, fresh: bool, bytes: u64, limit: u64) -> TalosResult<()> {
    let used = if fresh && !root.join("quota.json").try_exists().map_err(io_error)? {
        0
    } else {
        load(root)?
    };
    let next = used
        .checked_add(bytes)
        .filter(|next| *next <= limit)
        .ok_or_else(|| {
            TalosError::new(
                ErrorCode::UsageLimitExceeded,
                "Bash output Workspace quota is full; Worker maintenance reclaims terminal logs.",
            )
        })?;
    // Persist before creating a log. A crash may overcount, never undercount.
    save(root, next)
}

pub(super) fn release(root: &Path, reserved: u64, actual: u64) -> TalosResult<()> {
    let used = load(root)?;
    let used = used
        .checked_sub(reserved)
        .and_then(|value| value.checked_add(actual))
        .ok_or_else(|| TalosError::internal("Bash output accounting requires Worker recovery."))?;
    save(root, used)
}
