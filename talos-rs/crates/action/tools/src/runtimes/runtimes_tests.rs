use super::*;
use std::sync::Arc;
use talos_config::AgentKernelConfig;

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
