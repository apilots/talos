use super::*;

fn actor() -> Actor {
    Actor::regular("user-1")
}

fn service(home: &Path) -> FsService {
    FsService::new(DirectoryPickerConfig::default(), Some(home.to_path_buf()))
}

#[tokio::test]
async fn directory_roots_remain_isolated_between_service_instances() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let first_service = service(first.path());
    let second_service = service(second.path());
    let actor = actor();
    let (first_snapshot, second_snapshot) = tokio::join!(
        first_service.read_directory(&actor, None),
        second_service.read_directory(&actor, None),
    );
    assert_eq!(
        (first_snapshot.unwrap().path, second_snapshot.unwrap().path),
        (
            talos_fs::canonicalize_path(first.path()).unwrap(),
            talos_fs::canonicalize_path(second.path()).unwrap(),
        )
    );
}

#[tokio::test]
async fn read_directory_defaults_to_home_root() {
    let temp = tempfile::tempdir().unwrap();
    let child = temp.path().join("project");
    talos_fs::create_directory(&child, false).unwrap();

    let snapshot = service(temp.path())
        .read_directory(&actor(), None)
        .await
        .unwrap();

    assert_eq!(
        snapshot.path,
        talos_fs::canonicalize_path(temp.path()).unwrap()
    );
    assert!(snapshot.entries.iter().any(|entry| entry.name == "project"));
    assert!(snapshot.parent.is_none());
}

#[tokio::test]
async fn read_directory_rejects_path_outside_home_root() {
    let home = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();

    let error = service(home.path())
        .read_directory(&actor(), Some(outside.path().to_path_buf()))
        .await
        .unwrap_err();

    assert!(error.is_code(ErrorCode::PathOutsideRoot));
}

#[tokio::test]
async fn create_directory_creates_single_child_under_home_root() {
    let home = tempfile::tempdir().unwrap();

    let response = service(home.path())
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
    let home = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let link = home.path().join("linked");
    std::os::unix::fs::symlink(target.path(), &link).unwrap();

    let snapshot = service(home.path())
        .read_directory(&actor(), None)
        .await
        .unwrap();
    let linked = snapshot
        .entries
        .iter()
        .find(|entry| entry.name == "linked")
        .expect("symlink directory should be visible");
    assert!(linked.is_symlink);

    let error = service(home.path())
        .read_directory(&actor(), Some(link))
        .await
        .unwrap_err();
    assert!(error.is_code(ErrorCode::SymlinkNotSupported));
}
