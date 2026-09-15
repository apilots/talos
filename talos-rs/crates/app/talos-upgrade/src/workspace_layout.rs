use std::fs;
use std::path::{Path, PathBuf};

use crate::runner::{
    UpgradeContext, UpgradeFuture, UpgradeTask, UpgradeTaskDefinition, UpgradeTaskRegistration,
};
use semver::Version;
use talos_config::{validate_user_workspace_segment, Config, TalosPaths};
use talos_error::{ErrorCode, TalosError, TalosResult};

pub(crate) const WORKSPACE_LAYOUT_V1_UPGRADE_ID: &str = "202608290001_workspace_layout_v1";
const UPGRADE_BACKUP_DIR: &str = "upgrades";
const UPGRADE_BACKUP_SOURCE_DIR: &str = "source";

pub(crate) fn upgrade_task() -> Box<dyn UpgradeTask> {
    Box::new(UpgradeTaskRegistration::new(
        UpgradeTaskDefinition {
            id: WORKSPACE_LAYOUT_V1_UPGRADE_ID,
            revision: 1,
            target_version: Version::new(0, 1, 8),
            priority: 100,
        },
        run_workspace_layout_v1,
        cleanup_workspace_layout,
    ))
}

pub(crate) fn initialize_workspace_layout(config: &Config) -> TalosResult<PathBuf> {
    let prefix = config.workspace_root_prefix(config.default_workspace_id())?;
    fs::create_dir_all(&prefix)
        .map_err(|error| io_error("create current workspace prefix", &prefix, error))?;
    ensure_anonymous_workspace(&prefix)?;
    Ok(prefix)
}

pub(crate) fn legacy_installation_exists(context: &UpgradeContext<'_>) -> TalosResult<bool> {
    let prefix = current_workspace_prefix(context.config)?;
    let backup = backup_source(context.paths);
    validate_backup_boundary(&prefix, &backup)?;
    if backup.exists() {
        return ensure_directory(&backup, "workspace upgrade backup").map(|()| true);
    }
    legacy_source_has_state(&legacy_source(context.config, context.paths))
}

fn run_workspace_layout_v1<'a>(context: &'a UpgradeContext<'_>) -> UpgradeFuture<'a> {
    Box::pin(async move { migrate_workspace_layout_v0_to_v1(context) })
}

fn cleanup_workspace_layout<'a>(context: &'a UpgradeContext<'_>) -> UpgradeFuture<'a> {
    Box::pin(async move {
        let task_dir = context
            .paths
            .talos_home
            .join(UPGRADE_BACKUP_DIR)
            .join(WORKSPACE_LAYOUT_V1_UPGRADE_ID);
        let source = backup_source(context.paths);
        let recovered_root = context.paths.talos_home.join("recovered");
        let recovered = recovered_root.join(WORKSPACE_LAYOUT_V1_UPGRADE_ID);
        let prefix = current_workspace_prefix(context.config)?;
        validate_backup_boundary(&prefix, &source)?;
        validate_backup_boundary(&prefix, &recovered)?;
        if !task_dir
            .try_exists()
            .map_err(|error| io_error("inspect workspace cleanup", &task_dir, error))?
        {
            return Ok(());
        }
        ensure_cleanup_directory(&task_dir)?;
        if source
            .try_exists()
            .map_err(|error| io_error("inspect workspace source", &source, error))?
        {
            ensure_cleanup_directory(&source)?;
            // Success proves users were reconciled in full; live files may
            // since have changed or been removed by their owners.
            let mut entries = sorted_entries(&source)?;
            entries
                .retain(|entry| entry.file_name() != "users" && entry.file_name() != ".DS_Store");
            if !entries.is_empty() {
                fs::create_dir_all(&recovered_root).map_err(|error| {
                    io_error("create recovered data root", &recovered_root, error)
                })?;
                ensure_cleanup_directory(&recovered_root)?;
                fs::create_dir_all(&recovered).map_err(|error| {
                    io_error("create recovered workspace directory", &recovered, error)
                })?;
                ensure_cleanup_directory(&recovered)?;
                sync_directory(&recovered_root)?;
                sync_directory(&context.paths.talos_home)?;
            }
            for entry in entries {
                let target = recovered.join(entry.file_name());
                match fs::symlink_metadata(&target) {
                    Ok(_) => {
                        return Err(TalosError::new(
                            ErrorCode::Conflict,
                            "Recovered workspace data already exists; originals were retained.",
                        )
                        .context(format!("recovery target {}", target.display())));
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(io_error("inspect recovered target", &target, error)),
                }
                talos_fs::rename(&entry.path(), &target)?;
                sync_directory(&recovered)?;
                sync_directory(&source)?;
            }
        }
        // Repeat these syncs even after an interrupted rename, before removing
        // the source tree or publishing cleanup completion.
        if recovered
            .try_exists()
            .map_err(|error| io_error("inspect recovered workspace", &recovered, error))?
        {
            ensure_cleanup_directory(&recovered_root)?;
            ensure_cleanup_directory(&recovered)?;
            sync_directory(&recovered)?;
            sync_directory(&recovered_root)?;
            sync_directory(&context.paths.talos_home)?;
            tracing::info!(path = %recovered.display(), "unowned workspace originals recovered");
        }
        talos_fs::remove_directory(&source).await?;
        // Refuse to discard unknown task artifacts outside the owned source.
        fs::remove_dir(&task_dir).map_err(|error| {
            io_error("remove empty workspace upgrade directory", &task_dir, error)
        })?;
        Ok(())
    })
}

fn ensure_cleanup_directory(path: &Path) -> TalosResult<()> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| io_error("inspect workspace cleanup directory", path, error))?;
    if !metadata.is_dir() {
        return Err(TalosError::new(
            ErrorCode::Conflict,
            "Workspace cleanup requires a regular directory.",
        ));
    }
    Ok(())
}

fn sync_directory(path: &Path) -> TalosResult<()> {
    fs::File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|error| io_error("sync workspace directory", path, error))
}

fn migrate_workspace_layout_v0_to_v1(context: &UpgradeContext<'_>) -> TalosResult<()> {
    let prefix = current_workspace_prefix(context.config)?;
    let source = legacy_source(context.config, context.paths);
    let backup = backup_source(context.paths);
    validate_backup_boundary(&prefix, &backup)?;
    if !prepare_stable_backup(&source, &backup)? {
        return Ok(());
    }
    fs::create_dir_all(&prefix)
        .map_err(|error| io_error("create workspace migration target", &prefix, error))?;
    migrate_legacy_users(&backup, &prefix)?;
    sync_directory(&prefix)?;
    if let Some(parent) = prefix.parent() {
        sync_directory(parent)?;
    }
    Ok(())
}

fn current_workspace_prefix(config: &Config) -> TalosResult<PathBuf> {
    config.workspace_root_prefix(config.default_workspace_id())
}

fn legacy_source(config: &Config, paths: &TalosPaths) -> PathBuf {
    paths.legacy_workspace_dir_for_agent(&config.agent.name)
}

fn backup_source(paths: &TalosPaths) -> PathBuf {
    paths
        .talos_home
        .join(UPGRADE_BACKUP_DIR)
        .join(WORKSPACE_LAYOUT_V1_UPGRADE_ID)
        .join(UPGRADE_BACKUP_SOURCE_DIR)
}

fn validate_backup_boundary(prefix: &Path, backup: &Path) -> TalosResult<()> {
    if prefix.starts_with(backup) || backup.starts_with(prefix) {
        return Err(TalosError::new(
            ErrorCode::InvalidRequest,
            format!(
                "workspace prefix {} overlaps upgrade backup {}",
                prefix.display(),
                backup.display()
            ),
        ));
    }
    Ok(())
}

fn prepare_stable_backup(source: &Path, backup: &Path) -> TalosResult<bool> {
    if backup.exists() {
        ensure_directory(backup, "workspace upgrade backup")?;
        tracing::info!(
            backup = %backup.display(),
            "workspace layout upgrade resumed from stable backup"
        );
        return Ok(true);
    }
    if !legacy_source_has_state(source)? {
        return Ok(false);
    }
    let parent = backup.parent().ok_or_else(|| {
        TalosError::internal("workspace backup has no parent; technical details were withheld.")
            .context(format!(
                "workspace backup has no parent: {}",
                backup.display()
            ))
    })?;
    fs::create_dir_all(parent)
        .map_err(|error| io_error("create workspace upgrade backup directory", parent, error))?;
    fs::rename(source, backup)
        .map_err(|error| io_error("rename legacy workspace to stable backup", source, error))?;
    tracing::info!(
        source = %source.display(),
        backup = %backup.display(),
        "legacy workspace renamed to stable upgrade backup"
    );
    Ok(true)
}

fn legacy_source_has_state(source: &Path) -> TalosResult<bool> {
    if !source.exists() {
        return Ok(false);
    }
    ensure_directory(source, "legacy workspace")?;
    if source.join("users").exists()
        || [
            "SOUL.md",
            "IDENTITY.md",
            "USER.md",
            "HEARTBEAT.md",
            "MEMORY.md",
        ]
        .iter()
        .any(|name| source.join(name).exists())
        || ["sessions", "memory"]
            .iter()
            .any(|name| source.join(name).exists())
    {
        return Ok(true);
    }
    Ok(false)
}

fn migrate_legacy_users(backup: &Path, prefix: &Path) -> TalosResult<()> {
    let users = backup.join("users");
    if !users.exists() {
        return Ok(());
    }
    ensure_directory(&users, "legacy users directory")?;
    for entry in sorted_entries(&users)? {
        if entry.file_name() == ".DS_Store" || entry.file_name() == ".dream-memory-scope.lock" {
            continue;
        }
        let source = entry.path();
        if !source.is_dir() {
            return Err(TalosError::new(
                ErrorCode::Conflict,
                format!(
                    "legacy users entry is not a directory: {}",
                    source.display()
                ),
            ));
        }
        let user_id = entry.file_name().to_string_lossy().into_owned();
        validate_user_workspace_segment(&user_id)?;
        reconcile_tree(&source, &prefix.join(&user_id))?;
    }
    Ok(())
}

fn ensure_directory(path: &Path, label: &str) -> TalosResult<()> {
    if path.is_dir() {
        return Ok(());
    }
    Err(TalosError::new(
        ErrorCode::Conflict,
        format!("{label} is not a directory: {}", path.display()),
    ))
}

fn reconcile_tree(source: &Path, target: &Path) -> TalosResult<()> {
    let metadata = fs::metadata(source)
        .map_err(|error| io_error("read legacy workspace metadata", source, error))?;
    if metadata.is_dir() {
        if target.exists() && !target.is_dir() {
            return conflict(source, target);
        }
        fs::create_dir_all(target)
            .map_err(|error| io_error("create migrated workspace directory", target, error))?;
        for entry in sorted_entries(source)? {
            reconcile_tree(&entry.path(), &target.join(entry.file_name()))?;
        }
        sync_directory(target)?;
        return Ok(());
    }
    if target.exists() {
        return ensure_files_equal(source, target);
    }
    let parent = target.parent().ok_or_else(|| {
        TalosError::internal("workspace target has no parent; technical details were withheld.")
            .context(format!(
                "workspace target has no parent: {}",
                target.display()
            ))
    })?;
    fs::create_dir_all(parent)
        .map_err(|error| io_error("create migrated workspace parent", parent, error))?;
    fs::copy(source, target)
        .map_err(|error| io_error("copy legacy workspace file", source, error))?;
    fs::File::open(target)
        .and_then(|file| file.sync_all())
        .map_err(|error| io_error("sync migrated workspace file", target, error))?;
    Ok(())
}

fn ensure_files_equal(source: &Path, target: &Path) -> TalosResult<()> {
    if !source.is_file() || !target.is_file() {
        return conflict(source, target);
    }
    let source_bytes =
        fs::read(source).map_err(|error| io_error("read source file", source, error))?;
    let target_bytes =
        fs::read(target).map_err(|error| io_error("read target file", target, error))?;
    if source_bytes == target_bytes {
        fs::File::open(target)
            .and_then(|file| file.sync_all())
            .map_err(|error| io_error("sync reconciled workspace file", target, error))
    } else {
        conflict(source, target)
    }
}

fn conflict<T>(source: &Path, target: &Path) -> TalosResult<T> {
    Err(TalosError::new(
        ErrorCode::Conflict,
        format!(
            "workspace migration conflict between {} and {}",
            source.display(),
            target.display()
        ),
    ))
}

fn ensure_anonymous_workspace(prefix: &Path) -> TalosResult<()> {
    let anonymous = prefix.join("anonymous");
    fs::create_dir_all(&anonymous)
        .map_err(|error| io_error("create anonymous workspace", &anonymous, error))
}

fn sorted_entries(path: &Path) -> TalosResult<Vec<fs::DirEntry>> {
    let mut entries = fs::read_dir(path)
        .map_err(|error| io_error("read workspace directory", path, error))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| io_error("read workspace entry", path, error))?;
    entries.sort_by_key(fs::DirEntry::file_name);
    Ok(entries)
}

fn io_error(operation: &str, path: &Path, error: std::io::Error) -> TalosError {
    {
        let diagnostic = format!("{operation} {}: {error}", path.display());
        TalosError::internal_with_source(
            "Upgrade filesystem operation failed; technical details were withheld.",
            error,
        )
        .context(diagnostic)
    }
}

#[cfg(test)]
#[path = "workspace_layout_tests.rs"]
mod tests;
