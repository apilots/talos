use std::path::{Path, PathBuf};

use tokio::fs;

use super::{ToolDirectoryEntry, ToolFileMetadata};
use crate::manager::ToolSandboxContext;
use crate::{AbsolutePathBuf, FileSystemSandboxPolicy, SandboxCapability, TalosSandboxError};

const MAX_DIRECTORY_ENTRIES: usize = 100_000;

pub(super) fn require_direct_access(context: &ToolSandboxContext) -> Result<(), TalosSandboxError> {
    if context.effective_policy.sandbox != SandboxCapability::Disabled
        || context.effective_policy.permissions.file_system
            != FileSystemSandboxPolicy::DangerFullAccess
    {
        return Err(TalosSandboxError::SandboxBypassDenied);
    }
    Ok(())
}

pub(super) fn authorize_read(
    path: &Path,
    context: &ToolSandboxContext,
) -> Result<PathBuf, TalosSandboxError> {
    let resolved = resolve_existing_path(path)?;
    let (readable, denied) = match &context.effective_policy.permissions.file_system {
        FileSystemSandboxPolicy::ReadOnly(scope) => (&scope.readable_roots, &scope.denied_roots),
        FileSystemSandboxPolicy::WorkspaceWrite(scope) => {
            (&scope.readable_roots, &scope.denied_roots)
        }
        FileSystemSandboxPolicy::DangerFullAccess => {
            return Err(TalosSandboxError::SandboxBypassDenied);
        }
    };
    authorize_path(&resolved, readable, denied, "read")?;
    Ok(resolved)
}

pub(super) fn authorize_write(
    path: &Path,
    context: &ToolSandboxContext,
) -> Result<PathBuf, TalosSandboxError> {
    let resolved = resolve_existing_path(path)?;
    let FileSystemSandboxPolicy::WorkspaceWrite(scope) =
        &context.effective_policy.permissions.file_system
    else {
        return Err(access_denied(&resolved, "write"));
    };
    authorize_path(
        &resolved,
        &scope.writable_roots,
        &scope.denied_roots,
        "write",
    )?;
    Ok(resolved)
}

fn authorize_path(
    path: &Path,
    allowed_roots: &[AbsolutePathBuf],
    denied_roots: &[AbsolutePathBuf],
    operation: &'static str,
) -> Result<(), TalosSandboxError> {
    if denied_roots
        .iter()
        .any(|root| path.starts_with(root.as_path()))
        || !allowed_roots
            .iter()
            .any(|root| path.starts_with(root.as_path()))
    {
        return Err(access_denied(path, operation));
    }
    Ok(())
}

// Adapted from Codex `exec-server/src/local_file_system.rs::resolve_existing_path`
// at commit a26bc337cf61cbca9cd9ac25b25c88c32186eb20.
fn resolve_existing_path(path: &Path) -> Result<PathBuf, TalosSandboxError> {
    if !path.is_absolute() {
        return Err(file_system_setup_error(format!(
            "filesystem path is not absolute: {}",
            path.display()
        )));
    }
    let mut unresolved_suffix = Vec::new();
    let mut existing_path = path;
    while path_is_missing(existing_path)? {
        let Some(file_name) = existing_path.file_name() else {
            break;
        };
        unresolved_suffix.push(file_name.to_os_string());
        existing_path = existing_path.parent().ok_or_else(|| {
            file_system_setup_error(format!(
                "filesystem path has no existing ancestor: {}",
                path.display()
            ))
        })?;
    }
    let mut resolved = std::fs::canonicalize(existing_path).map_err(file_system_error)?;
    for file_name in unresolved_suffix.iter().rev() {
        resolved.push(file_name);
    }
    Ok(resolved)
}

fn path_is_missing(path: &Path) -> Result<bool, TalosSandboxError> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(file_system_error(error)),
    }
}

pub(super) async fn metadata(path: &Path) -> Result<ToolFileMetadata, TalosSandboxError> {
    let metadata = fs::symlink_metadata(path)
        .await
        .map_err(file_system_error)?;
    let file_type = metadata.file_type();
    Ok(ToolFileMetadata {
        is_file: file_type.is_file(),
        is_directory: file_type.is_dir(),
        is_symlink: file_type.is_symlink(),
        len: metadata.len(),
    })
}

pub(super) async fn read_directory(
    path: &Path,
) -> Result<Vec<ToolDirectoryEntry>, TalosSandboxError> {
    let mut directory = fs::read_dir(path).await.map_err(file_system_error)?;
    let mut entries = Vec::new();
    while let Some(entry) = directory.next_entry().await.map_err(file_system_error)? {
        if entries.len() == MAX_DIRECTORY_ENTRIES {
            return Err(file_system_setup_error(format!(
                "directory entry limit exceeded: {MAX_DIRECTORY_ENTRIES}"
            )));
        }
        entries.push(ToolDirectoryEntry {
            path: entry.path(),
            file_name: entry.file_name(),
        });
    }
    Ok(entries)
}

fn access_denied(path: &Path, operation: &'static str) -> TalosSandboxError {
    TalosSandboxError::FileSystemAccessDenied {
        operation,
        path: path.to_path_buf(),
    }
}

fn file_system_setup_error(reason: String) -> TalosSandboxError {
    TalosSandboxError::FileSystemOperationFailed { reason }
}

pub(super) fn file_system_error(error: std::io::Error) -> TalosSandboxError {
    file_system_setup_error(error.to_string())
}
