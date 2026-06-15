use std::fs;
use std::path::{Path, PathBuf};

use talos_config::{DirectoryPickerConfig, DirectoryPickerRoot};
use talos_error::{TalosError, TalosResult};
use talos_types::{
    FsDirectoryCreateResponse, FsDirectoryEntry, FsDirectorySnapshot, FsEntryKind, ProjectRecord,
    ProjectStatus, ProjectStatusState, ProjectUnavailableReason,
};

use crate::Actor;

#[derive(Clone)]
pub struct FsService {
    config: DirectoryPickerConfig,
}

impl FsService {
    pub fn new(config: DirectoryPickerConfig) -> Self {
        Self { config }
    }

    pub fn project_status(&self, project: ProjectRecord) -> ProjectRecord {
        ProjectRecord {
            status: status_for_directory(&project.directory),
            ..project
        }
    }

    pub async fn read_directory(
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

    pub async fn create_directory(
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
        if path.exists() {
            return Err(TalosError::Conflict(format!(
                "already_exists: directory already exists: {}",
                path.display()
            )));
        }
        fs::create_dir(&path).map_err(|err| {
            TalosError::StorageError(format!("create directory {}: {}", path.display(), err))
        })?;
        Ok(FsDirectoryCreateResponse { path })
    }

    fn ensure_enabled(&self) -> TalosResult<()> {
        if self.config.enabled {
            return Ok(());
        }
        Err(TalosError::ValidationError(
            "fs_disabled: directory picker is disabled".to_string(),
        ))
    }

    fn picker_root(&self) -> TalosResult<PathBuf> {
        match self.config.root {
            DirectoryPickerRoot::Home => {
                let home = std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .ok_or_else(|| TalosError::InvalidConfiguration("HOME is not set".into()))?;
                canonicalize_path(&home)
            }
        }
    }
}

fn status_for_directory(path: &Path) -> ProjectStatus {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => match fs::read_dir(path) {
            Ok(_) => ProjectStatus::default(),
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => ProjectStatus {
                state: ProjectStatusState::PermissionDenied,
                reason: Some(ProjectUnavailableReason::PermissionDenied),
            },
            Err(_) => ProjectStatus {
                state: ProjectStatusState::PermissionDenied,
                reason: Some(ProjectUnavailableReason::PermissionDenied),
            },
        },
        Ok(_) => ProjectStatus {
            state: ProjectStatusState::NotDirectory,
            reason: Some(ProjectUnavailableReason::NotDirectory),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => ProjectStatus {
            state: ProjectStatusState::Missing,
            reason: Some(ProjectUnavailableReason::PathNotFound),
        },
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => ProjectStatus {
            state: ProjectStatusState::PermissionDenied,
            reason: Some(ProjectUnavailableReason::PermissionDenied),
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
    Err(TalosError::ValidationError(format!(
        "invalid_path: {label} must be an absolute path: {}",
        path.display()
    )))
}

fn reject_symlink(path: &Path) -> TalosResult<()> {
    let metadata = fs::symlink_metadata(path).map_err(map_metadata_error)?;
    if metadata.file_type().is_symlink() {
        return Err(TalosError::ValidationError(format!(
            "symlink_not_supported: symlink directories are not supported: {}",
            path.display()
        )));
    }
    Ok(())
}

fn canonicalize_path(path: &Path) -> TalosResult<PathBuf> {
    fs::canonicalize(path).map_err(map_metadata_error)
}

fn ensure_directory(path: &Path) -> TalosResult<()> {
    let metadata = fs::metadata(path).map_err(map_metadata_error)?;
    if metadata.is_dir() {
        return Ok(());
    }
    Err(TalosError::ValidationError(format!(
        "not_directory: path is not a directory: {}",
        path.display()
    )))
}

fn ensure_readable(path: &Path) -> TalosResult<()> {
    fs::read_dir(path).map_err(|err| match err.kind() {
        std::io::ErrorKind::PermissionDenied => TalosError::AuthorizationDenied,
        _ => TalosError::StorageError(format!("read directory {}: {}", path.display(), err)),
    })?;
    Ok(())
}

fn ensure_writable(path: &Path) -> TalosResult<()> {
    if is_writable_directory(path) {
        return Ok(());
    }
    Err(TalosError::AuthorizationDenied)
}

fn is_writable_directory(path: &Path) -> bool {
    fs::metadata(path)
        .map(|metadata| !metadata.permissions().readonly())
        .unwrap_or(false)
}

fn ensure_under_root(path: &Path, root: &Path) -> TalosResult<()> {
    if is_under_root(path, root) {
        return Ok(());
    }
    Err(TalosError::ValidationError(format!(
        "path_outside_root: path is outside picker root: {}",
        path.display()
    )))
}

fn is_under_root(path: &Path, root: &Path) -> bool {
    path == root || path.starts_with(root)
}

fn list_child_directories(path: &Path, show_hidden: bool) -> TalosResult<Vec<FsDirectoryEntry>> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(path).map_err(map_metadata_error)? {
        let entry = entry.map_err(|err| {
            TalosError::StorageError(format!("read directory entry {}: {}", path.display(), err))
        })?;
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy().to_string();
        let hidden = name.starts_with('.');
        if hidden && !show_hidden {
            continue;
        }
        let entry_path = entry.path();
        let symlink_metadata = fs::symlink_metadata(&entry_path).map_err(map_metadata_error)?;
        let is_symlink = symlink_metadata.file_type().is_symlink();
        let is_directory = is_symlink || symlink_metadata.is_dir();
        if !is_directory {
            continue;
        }
        entries.push(FsDirectoryEntry {
            name,
            path: entry_path.clone(),
            kind: FsEntryKind::Directory,
            readable: !is_symlink && fs::read_dir(&entry_path).is_ok(),
            writable: !is_symlink && is_writable_directory(&entry_path),
            hidden,
            is_symlink,
        });
    }
    entries.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
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
        return Err(TalosError::ValidationError(format!(
            "invalid_name: invalid directory name: {name}"
        )));
    }
    Ok(())
}

fn map_metadata_error(error: std::io::Error) -> TalosError {
    match error.kind() {
        std::io::ErrorKind::NotFound => {
            TalosError::ResourceNotFound(format!("path_not_found: {}", error))
        }
        std::io::ErrorKind::PermissionDenied => TalosError::AuthorizationDenied,
        _ => TalosError::StorageError(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn actor() -> Actor {
        Actor::regular("user-1")
    }

    fn service() -> FsService {
        FsService::new(DirectoryPickerConfig::default())
    }

    #[tokio::test]
    async fn read_directory_defaults_to_home_root() {
        let _guard = env_lock();
        let temp = tempfile::tempdir().unwrap();
        let child = temp.path().join("project");
        fs::create_dir(&child).unwrap();
        std::env::set_var("HOME", temp.path());

        let snapshot = service().read_directory(&actor(), None).await.unwrap();

        assert_eq!(snapshot.path, fs::canonicalize(temp.path()).unwrap());
        assert!(snapshot.entries.iter().any(|entry| entry.name == "project"));
        assert!(snapshot.parent.is_none());
    }

    #[tokio::test]
    async fn read_directory_rejects_path_outside_home_root() {
        let _guard = env_lock();
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::env::set_var("HOME", home.path());

        let error = service()
            .read_directory(&actor(), Some(outside.path().to_path_buf()))
            .await
            .unwrap_err();

        assert!(
            error.to_string().contains("path_outside_root"),
            "unexpected error: {error}"
        );
    }

    #[tokio::test]
    async fn create_directory_creates_single_child_under_home_root() {
        let _guard = env_lock();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("HOME", home.path());

        let response = service()
            .create_directory(
                &actor(),
                home.path().to_path_buf(),
                "new-project".to_string(),
            )
            .await
            .unwrap();

        assert!(response.path.is_dir());
        assert_eq!(response.path.file_name().unwrap(), "new-project");
    }

    #[cfg(unix)]
    #[test]
    fn project_status_marks_unreadable_directory_as_permission_denied() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("private");
        fs::create_dir(&directory).unwrap();
        let original_permissions = fs::metadata(&directory).unwrap().permissions();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o000)).unwrap();

        let status = status_for_directory(&directory);

        fs::set_permissions(&directory, original_permissions).unwrap();
        assert_eq!(status.state, ProjectStatusState::PermissionDenied);
        assert_eq!(
            status.reason,
            Some(ProjectUnavailableReason::PermissionDenied)
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn read_directory_rejects_symlink_directory() {
        let _guard = env_lock();
        let home = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        let link = home.path().join("linked");
        std::os::unix::fs::symlink(target.path(), &link).unwrap();
        std::env::set_var("HOME", home.path());

        let snapshot = service().read_directory(&actor(), None).await.unwrap();
        let linked = snapshot
            .entries
            .iter()
            .find(|entry| entry.name == "linked")
            .expect("symlink directory should be visible");
        assert!(linked.is_symlink);

        let error = service()
            .read_directory(&actor(), Some(link))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("symlink_not_supported"));
    }
}
