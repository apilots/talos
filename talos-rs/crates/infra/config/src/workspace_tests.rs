use super::*;

fn test_kernel_config() -> AgentKernelConfig {
    let mut config = AgentKernelConfig::default();
    config.workspace_dir = "/tmp/talos-workspace".to_string();
    config
}

#[test]
fn resolves_user_workspace_from_base_prefix() {
    let workspace = resolve_effective_kernel_workspace(
        &test_kernel_config(),
        "user_0123456789abcdef0123456789abcdef",
    )
    .expect("cloud user id should resolve");

    assert_eq!(
        workspace.user_workspace_dir,
        PathBuf::from("/tmp/talos-workspace")
            .join("users")
            .join("user_0123456789abcdef0123456789abcdef")
    );
    assert_eq!(
        workspace.kernel_config.workspace_dir,
        workspace.user_workspace_dir.to_string_lossy().to_string()
    );
}

#[test]
fn resolves_anonymous_workspace() {
    let workspace = resolve_effective_kernel_workspace(&test_kernel_config(), "anonymous")
        .expect("anonymous should resolve");

    assert!(workspace
        .user_workspace_dir
        .ends_with(PathBuf::from("users").join("anonymous")));
}

#[test]
fn rejects_unsafe_user_id() {
    let error = resolve_effective_kernel_workspace(&test_kernel_config(), "../alice")
        .expect_err("unsafe user id should fail");

    assert!(matches!(error, TalosError::ValidationError(_)));
}

#[test]
fn rejects_inconsistent_session_user() {
    let error = validate_session_workspace_user("user_1", "user_2")
        .expect_err("session user mismatch should fail");

    assert!(matches!(error, TalosError::InternalError(_)));
}
