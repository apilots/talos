use std::path::{Path, PathBuf};

use talos_config::{DirectoryPickerConfig, DirectoryPickerRoot};
use talos_error::{TalosError, TalosFsErrorKind, TalosResult};
use talos_fs::FileSystem;
use talos_types::{
    FsDirectoryCreateResponse, FsDirectoryEntry, FsDirectorySnapshot, FsEntryKind, ProjectStatus,
    ProjectStatusState, ProjectUnavailableReason,
};

use crate::Actor;

#[derive(Clone)]
pub(crate) struct FsService {
    config: DirectoryPickerConfig,
}

impl FsService {
    pub(crate) fn new(config: DirectoryPickerConfig) -> Self {
        Self { config }
    }

    pub(crate) fn project_status_for_directory(&self, directory: &Path) -> ProjectStatus {
        status_for_directory(directory)
    }

    pub(crate) async fn read_directory(
        &self,
        _actor: &Actor,
        path: Option<PathBuf>,
    ) -> TalosResult<FsDirectorySnapshot> {
        self.ensure_enabled()?;
        let root = self.picker_root()?;
        let path = path.unwrap_or_else(|| root.clone());
        validate_absolute(&path, "path")?;
        reject_symlink(&path)?;
        let path = canonicalize_path(&path)?;
        ensure_under_root(&path, &root)?;
        ensure_directory(&path)?;
        ensure_readable(&path)?;

        let entries = list_child_directories(&path, self.config.show_hidden)?;
        let parent = if path == root {
            None
        } else {
            path.parent()
                .map(Path::to_path_buf)
                .filter(|parent| is_under_root(parent, &root))
        };

        Ok(FsDirectorySnapshot {
            path: path.clone(),
            parent,
            entries,
            readable: true,
            writable: is_writable_directory(&path),
        })
    }

    pub(crate) async fn create_directory(
        &self,
        _actor: &Actor,
        parent: PathBuf,
        name: String,
    ) -> TalosResult<FsDirectoryCreateResponse> {
        self.ensure_enabled()?;
        let root = self.picker_root()?;
        validate_absolute(&parent, "parent")?;
        reject_symlink(&parent)?;
        validate_directory_name(&name)?;
        let parent = canonicalize_path(&parent)?;
        ensure_under_root(&parent, &root)?;
        ensure_directory(&parent)?;
        ensure_writable(&parent)?;
        let path = parent.join(name);
        if talos_fs::std_fs().get_file_metadata(&path)?.is_some() {
            return Err(fs_error(
                TalosFsErrorKind::AlreadyExists,
                format!("directory already exists: {}", path.display()),
            ));
        }
        talos_fs::create_directory(&path, false)?;
        Ok(FsDirectoryCreateResponse { path })
    }

    fn ensure_enabled(&self) -> TalosResult<()> {
        if self.config.enabled {
            return Ok(());
        }
        Err(fs_error(
            TalosFsErrorKind::FsDisabled,
            "directory picker is disabled",
        ))
    }

    fn picker_root(&self) -> TalosResult<PathBuf> {
        match self.config.root {
            DirectoryPickerRoot::Home => {
                let home = std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .ok_or_else(|| TalosError::InvalidConfiguration("HOME is not set".into()))?;
                talos_fs::canonicalize_path(&home)
            }
        }
    }
}

fn status_for_directory(path: &Path) -> ProjectStatus {
    match talos_fs::std_fs().get_file_metadata(path) {
        Ok(Some(metadata)) if metadata.is_dir => match talos_fs::read_directory(path) {
            Ok(_) => ProjectStatus::default(),
            Err(_) => ProjectStatus {
                state: ProjectStatusState::PermissionDenied,
                reason: Some(ProjectUnavailableReason::PermissionDenied),
            },
        },
        Ok(Some(_)) => ProjectStatus {
            state: ProjectStatusState::NotDirectory,
            reason: Some(ProjectUnavailableReason::NotDirectory),
        },
        Ok(None) => ProjectStatus {
            state: ProjectStatusState::Missing,
            reason: Some(ProjectUnavailableReason::PathNotFound),
        },
        Err(_) => ProjectStatus {
            state: ProjectStatusState::PermissionDenied,
            reason: Some(ProjectUnavailableReason::PermissionDenied),
        },
    }
}

fn validate_absolute(path: &Path, label: &str) -> TalosResult<()> {
    if path.is_absolute() {
        return Ok(());
    }
    Err(fs_error(
        TalosFsErrorKind::InvalidPath,
        format!("{label} must be an absolute path: {}", path.display()),
    ))
}

fn reject_symlink(path: &Path) -> TalosResult<()> {
    let metadata = path_metadata_required(path)?;
    if metadata.is_symlink {
        return Err(fs_error(
            TalosFsErrorKind::SymlinkNotSupported,
            format!("symlink directories are not supported: {}", path.display()),
        ));
    }
    Ok(())
}

fn canonicalize_path(path: &Path) -> TalosResult<PathBuf> {
    talos_fs::canonicalize_path(path)
}

fn ensure_directory(path: &Path) -> TalosResult<()> {
    let metadata = path_metadata_required(path)?;
    if metadata.is_dir {
        return Ok(());
    }
    Err(fs_error(
        TalosFsErrorKind::NotDirectory,
        format!("path is not a directory: {}", path.display()),
    ))
}

fn ensure_readable(path: &Path) -> TalosResult<()> {
    talos_fs::read_directory(path).map_err(|_| TalosError::AuthorizationDenied)?;
    Ok(())
}

fn ensure_writable(path: &Path) -> TalosResult<()> {
    if is_writable_directory(path) {
        return Ok(());
    }
    Err(TalosError::AuthorizationDenied)
}

fn is_writable_directory(path: &Path) -> bool {
    talos_fs::std_fs()
        .get_file_metadata(path)
        .map(|metadata| metadata.is_some_and(|metadata| !metadata.readonly))
        .unwrap_or(false)
}

fn ensure_under_root(path: &Path, root: &Path) -> TalosResult<()> {
    if is_under_root(path, root) {
        return Ok(());
    }
    Err(fs_error(
        TalosFsErrorKind::PathOutsideRoot,
        format!("path is outside picker root: {}", path.display()),
    ))
}

fn is_under_root(path: &Path, root: &Path) -> bool {
    path == root || path.starts_with(root)
}

fn list_child_directories(path: &Path, show_hidden: bool) -> TalosResult<Vec<FsDirectoryEntry>> {
    let mut entries = Vec::new();
    let Some(directory_entries) = talos_fs::read_directory(path)? else {
        return Err(fs_error(
            TalosFsErrorKind::PathNotFound,
            path.display().to_string(),
        ));
    };
    for entry in directory_entries {
        let Some(file_name) = entry.path.file_name() else {
            continue;
        };
        let name = file_name.to_string_lossy().to_string();
        let hidden = name.starts_with('.');
        if hidden && !show_hidden {
            continue;
        }
        let entry_path = entry.path;
        let is_symlink = entry.metadata.is_symlink;
        let is_directory = is_symlink || entry.metadata.is_dir;
        if !is_directory {
            continue;
        }
        entries.push(FsDirectoryEntry {
            name,
            path: entry_path.clone(),
            kind: FsEntryKind::Directory,
            readable: !is_symlink && talos_fs::read_directory(&entry_path).is_ok(),
            writable: !is_symlink && is_writable_directory(&entry_path),
            hidden,
            is_symlink,
        });
    }
    entries.sort_by_key(|left| left.name.to_lowercase());
    Ok(entries)
}

fn validate_directory_name(name: &str) -> TalosResult<()> {
    let trimmed = name.trim();
    if trimmed.is_empty()
        || trimmed == "."
        || trimmed == ".."
        || trimmed.contains(std::path::MAIN_SEPARATOR)
        || trimmed.contains('/')
    {
        return Err(fs_error(
            TalosFsErrorKind::InvalidName,
            format!("invalid directory name: {name}"),
        ));
    }
    Ok(())
}

fn path_metadata_required(path: &Path) -> TalosResult<talos_fs::FileMetadata> {
    talos_fs::std_fs()
        .get_file_metadata(path)?
        .ok_or_else(|| fs_error(TalosFsErrorKind::PathNotFound, path.display().to_string()))
}

fn fs_error(kind: TalosFsErrorKind, message: impl Into<String>) -> TalosError {
    TalosError::FsError {
        kind,
        message: message.into(),
    }
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
