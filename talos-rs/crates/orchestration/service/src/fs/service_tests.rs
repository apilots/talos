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
    talos_fs::create_directory(&child, false).unwrap();
    std::env::set_var("HOME", temp.path());

    let snapshot = service().read_directory(&actor(), None).await.unwrap();

    assert_eq!(
        snapshot.path,
        talos_fs::canonicalize_path(temp.path()).unwrap()
    );
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

    assert!(error.is_fs_error_kind(TalosFsErrorKind::PathOutsideRoot));
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

    assert!(talos_fs::get_file_metadata(&response.path)
        .await
        .unwrap()
        .is_some_and(|metadata| metadata.is_dir));
    assert_eq!(response.path.file_name().unwrap(), "new-project");
}

#[cfg(unix)]
#[test]
fn project_status_marks_unreadable_directory_as_permission_denied() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    let directory = temp.path().join("private");
    talos_fs::create_directory(&directory, false).unwrap();
    let original_permissions = std::fs::metadata(&directory).unwrap().permissions();
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o000)).unwrap();

    let status = status_for_directory(&directory);

    std::fs::set_permissions(&directory, original_permissions).unwrap();
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
    assert!(error.is_fs_error_kind(TalosFsErrorKind::SymlinkNotSupported));
}
