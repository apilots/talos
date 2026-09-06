use super::*;
use std::sync::Arc;
use talos_config::AgentKernelConfig;

#[test]
fn sandbox_failures_retain_sources_and_exclude_private_details() {
    use std::error::Error;

    for (source, expected_code, expected_kind) in [
        (
            TalosSandboxError::FileSystemOperationFailed {
                reason: "private-sandbox-sentinel".to_string(),
            },
            talos_error::ErrorCode::InternalError,
            None,
        ),
        (
            TalosSandboxError::FileSystemAccessDenied {
                operation: "read",
                path: PathBuf::from("/private-sandbox-sentinel"),
            },
            talos_error::ErrorCode::ToolExecutionFailed,
            Some(talos_error::TalosToolErrorKind::PermissionRejection),
        ),
    ] {
        let error = map_sandbox_error(source.clone());
        assert_eq!(
            error
                .source()
                .and_then(|source| source.downcast_ref::<TalosSandboxError>()),
            Some(&source)
        );
        assert!(!error
            .to_api_error()
            .message
            .contains("private-sandbox-sentinel"));
        assert_eq!(error.error_code(), expected_code);
        assert_eq!(
            error
                .tool_failure_diagnostic()
                .map(|diagnostic| diagnostic.kind),
            expected_kind
        );
    }
}

#[test]
fn resolve_base_dir_prefers_explicit_project_root() {
    let config = AgentKernelConfig {
        workspace_dir: "/tmp/talos-workspace".to_string(),
        ..Default::default()
    };
    let context =
        crate::ToolContext::new_with_kernel_config("session-1", "user-1", Some(Arc::new(config)))
            .with_project_root("/tmp/project-root");

    let base_dir = resolve_base_dir(&context).expect("base dir should resolve");

    assert_eq!(base_dir, PathBuf::from("/tmp/project-root"));
}

#[test]
fn resolve_base_dir_falls_back_to_workspace_dir_without_project_root() {
    let config = AgentKernelConfig {
        workspace_dir: "/tmp/talos-workspace".to_string(),
        ..Default::default()
    };
    let context =
        crate::ToolContext::new_with_kernel_config("session-1", "user-1", Some(Arc::new(config)));

    let base_dir = resolve_base_dir(&context).expect("base dir should resolve");

    assert_eq!(base_dir, PathBuf::from("/tmp/talos-workspace"));
}

#[test]
fn resolve_base_dir_errors_without_project_root_or_workspace_dir() {
    let context = crate::ToolContext::new("session-1", "user-1");

    let error = resolve_base_dir(&context).expect_err("resolution should fail");

    assert!(error.to_string().contains("no project root is configured"));
}
