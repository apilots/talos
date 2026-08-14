use std::collections::BTreeSet;

use talos_types::UserPermissionMode;
use tempfile::TempDir;

use super::*;
use crate::TalosSandboxError;
use crate::{
    AbsolutePathBuf, FileSystemPermissionLevel, NetworkSandboxPolicy, RequestedToolPermissions,
    SandboxProviderPlatform, ToolGovernanceConstraints, ToolInvocationIdentity, ToolSandbox,
    ToolSandboxDecision, ToolSandboxEnvironment, ToolSandboxOperationKind, ToolSandboxRequest,
};

fn file_system_handle(
    workspace: &TempDir,
    mode: UserPermissionMode,
    file_system: FileSystemPermissionLevel,
    denied_roots: Vec<PathBuf>,
) -> ToolFileSystemHandle {
    let workspace_root = AbsolutePathBuf::new(
        std::fs::canonicalize(workspace.path()).expect("workspace should canonicalize"),
    )
    .expect("workspace should be absolute");
    let denied_roots = denied_roots
        .into_iter()
        .map(|path| {
            AbsolutePathBuf::new(std::fs::canonicalize(path).expect("denied root should exist"))
                .expect("denied root should be absolute")
        })
        .collect();
    let environment = ToolSandboxEnvironment::local(vec![workspace_root], denied_roots)
        .expect("environment should be valid");
    let network = if mode == UserPermissionMode::FullAccess {
        NetworkSandboxPolicy::Enabled
    } else {
        NetworkSandboxPolicy::Restricted
    };
    let request = ToolSandboxRequest {
        invocation: ToolInvocationIdentity::new("file-call", "file-tool"),
        user_mode: mode,
        governance: ToolGovernanceConstraints::default(),
        requested_permissions: RequestedToolPermissions::new(
            file_system,
            network,
            None,
            BTreeSet::from([ToolSandboxOperationKind::FileSystem]),
        )
        .expect("permissions should be valid"),
        approval_grant: None,
        command_risk_assessment: None,
        environment,
    };
    let sandbox =
        ToolSandbox::from_providers(Some(SandboxProviderPlatform::MacosSeatbelt), Vec::new());
    let ToolSandboxDecision::Allowed(permit) =
        sandbox.evaluate(request).expect("request should evaluate")
    else {
        panic!("request should be allowed");
    };
    permit
        .file_system()
        .expect("filesystem operation should be granted")
}

#[tokio::test]
async fn read_only_handle_reads_workspace_and_rejects_write() {
    let workspace = tempfile::tempdir().expect("workspace should create");
    let path = workspace.path().join("input.txt");
    std::fs::write(&path, "input").expect("fixture should write");
    let handle = file_system_handle(
        &workspace,
        UserPermissionMode::AskForApproval,
        FileSystemPermissionLevel::ReadOnly,
        Vec::new(),
    );

    assert_eq!(
        handle
            .read_to_string(&path)
            .await
            .expect("workspace read should succeed"),
        "input"
    );
    assert!(matches!(
        handle.write(&path, b"changed").await,
        Err(TalosSandboxError::FileSystemAccessDenied {
            operation: "write",
            ..
        })
    ));
    assert_eq!(
        std::fs::read_to_string(path).expect("fixture should remain readable"),
        "input"
    );
}

#[tokio::test]
async fn workspace_write_allows_nested_create_and_rejects_outside_write() {
    let parent = tempfile::tempdir().expect("parent should create");
    let workspace_path = parent.path().join("workspace");
    std::fs::create_dir(&workspace_path).expect("workspace should create");
    let workspace = TempDir::new_in(&workspace_path).expect("nested workspace should create");
    let outside = parent.path().join("outside.txt");
    let handle = file_system_handle(
        &workspace,
        UserPermissionMode::AskForApproval,
        FileSystemPermissionLevel::WorkspaceWrite,
        Vec::new(),
    );
    let nested = workspace.path().join("new/deep/file.txt");

    handle
        .create_directory_all(nested.parent().expect("nested path should have parent"))
        .await
        .expect("workspace directory create should succeed");
    handle
        .write(&nested, b"created")
        .await
        .expect("workspace write should succeed");
    assert_eq!(
        std::fs::read_to_string(nested).expect("created file should read"),
        "created"
    );
    assert!(matches!(
        handle.write(&outside, b"denied").await,
        Err(TalosSandboxError::FileSystemAccessDenied {
            operation: "write",
            ..
        })
    ));
    assert!(!outside.exists());
}

#[tokio::test]
async fn denied_root_overrides_workspace_access() {
    let workspace = tempfile::tempdir().expect("workspace should create");
    let denied = workspace.path().join("private");
    std::fs::create_dir(&denied).expect("denied root should create");
    let path = denied.join("secret.txt");
    std::fs::write(&path, "secret").expect("fixture should write");
    let handle = file_system_handle(
        &workspace,
        UserPermissionMode::AskForApproval,
        FileSystemPermissionLevel::WorkspaceWrite,
        vec![denied],
    );

    assert!(matches!(
        handle.read(&path).await,
        Err(TalosSandboxError::FileSystemAccessDenied {
            operation: "read",
            ..
        })
    ));
    assert!(matches!(
        handle.write(&path, b"changed").await,
        Err(TalosSandboxError::FileSystemAccessDenied {
            operation: "write",
            ..
        })
    ));
}

#[tokio::test]
async fn parent_traversal_cannot_escape_workspace() {
    let parent = tempfile::tempdir().expect("parent should create");
    let workspace_path = parent.path().join("workspace");
    std::fs::create_dir(&workspace_path).expect("workspace should create");
    let workspace = TempDir::new_in(&workspace_path).expect("nested workspace should create");
    let target = workspace.path().join("../../escaped.txt");
    let handle = file_system_handle(
        &workspace,
        UserPermissionMode::AskForApproval,
        FileSystemPermissionLevel::WorkspaceWrite,
        Vec::new(),
    );

    assert!(matches!(
        handle.write(&target, b"denied").await,
        Err(TalosSandboxError::FileSystemAccessDenied {
            operation: "write",
            ..
        })
    ));
    assert!(!parent.path().join("escaped.txt").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn symlinked_parent_cannot_escape_workspace() {
    let workspace = tempfile::tempdir().expect("workspace should create");
    let outside = tempfile::tempdir().expect("outside should create");
    std::os::unix::fs::symlink(outside.path(), workspace.path().join("link"))
        .expect("symlink should create");
    let target = workspace.path().join("link/escaped.txt");
    let handle = file_system_handle(
        &workspace,
        UserPermissionMode::AskForApproval,
        FileSystemPermissionLevel::WorkspaceWrite,
        Vec::new(),
    );

    assert!(matches!(
        handle.write(&target, b"denied").await,
        Err(TalosSandboxError::FileSystemAccessDenied {
            operation: "write",
            ..
        })
    ));
    assert!(!outside.path().join("escaped.txt").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn broken_symlink_target_fails_closed() {
    let workspace = tempfile::tempdir().expect("workspace should create");
    let outside = tempfile::tempdir().expect("outside should create");
    let missing = outside.path().join("missing-directory");
    std::os::unix::fs::symlink(&missing, workspace.path().join("link"))
        .expect("symlink should create");
    let target = workspace.path().join("link/escaped.txt");
    let handle = file_system_handle(
        &workspace,
        UserPermissionMode::AskForApproval,
        FileSystemPermissionLevel::WorkspaceWrite,
        Vec::new(),
    );

    assert!(handle.write(&target, b"denied").await.is_err());
    assert!(!missing.exists());
}

#[tokio::test]
async fn rename_cannot_move_workspace_file_outside() {
    let parent = tempfile::tempdir().expect("parent should create");
    let workspace_path = parent.path().join("workspace");
    std::fs::create_dir(&workspace_path).expect("workspace should create");
    let workspace = TempDir::new_in(&workspace_path).expect("nested workspace should create");
    let source = workspace.path().join("source.txt");
    let target = parent.path().join("outside.txt");
    std::fs::write(&source, "source").expect("fixture should write");
    let handle = file_system_handle(
        &workspace,
        UserPermissionMode::AskForApproval,
        FileSystemPermissionLevel::WorkspaceWrite,
        Vec::new(),
    );

    assert!(matches!(
        handle.rename(&source, &target).await,
        Err(TalosSandboxError::FileSystemAccessDenied {
            operation: "write",
            ..
        })
    ));
    assert!(source.exists());
    assert!(!target.exists());
}

#[tokio::test]
async fn full_access_handle_uses_direct_filesystem() {
    let workspace = tempfile::tempdir().expect("workspace should create");
    let outside = tempfile::tempdir().expect("outside should create");
    let target = outside.path().join("direct.txt");
    let handle = file_system_handle(
        &workspace,
        UserPermissionMode::FullAccess,
        FileSystemPermissionLevel::FullAccess,
        Vec::new(),
    );

    assert_eq!(handle.enforcement(), ToolSandboxEnforcement::None);
    handle
        .write(&target, b"direct")
        .await
        .expect("full access should use direct filesystem");
    assert_eq!(
        std::fs::read_to_string(target).expect("direct file should read"),
        "direct"
    );
}
