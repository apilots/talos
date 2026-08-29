use super::*;

fn paths() -> crate::TalosPaths {
    crate::TalosPaths::from_home(PathBuf::from("/tmp/talos-home"))
}

#[test]
fn default_config_resolves_required_workspace_for_user() {
    let config = WorkspaceConfig::default();

    let workspace = config
        .resolve(
            &paths(),
            DEFAULT_WORKSPACE_ID,
            "user_0123456789abcdef0123456789abcdef",
        )
        .expect("cloud user id should resolve");

    assert_eq!(
        workspace,
        EffectiveWorkspace {
            workspace_id: DEFAULT_WORKSPACE_ID.to_string(),
            user_id: "user_0123456789abcdef0123456789abcdef".to_string(),
            root: PathBuf::from("/tmp/talos-home/workspace-talos")
                .join("user_0123456789abcdef0123456789abcdef"),
        }
    );
}

#[test]
fn resolves_anonymous_as_a_regular_persistent_user_scope() {
    let workspace = WorkspaceConfig::default()
        .resolve(&paths(), DEFAULT_WORKSPACE_ID, "anonymous")
        .expect("anonymous should resolve");

    assert_eq!(
        workspace.root,
        PathBuf::from("/tmp/talos-home/workspace-talos/anonymous")
    );
}

#[test]
fn resolves_non_default_configured_workspace() {
    let config = WorkspaceConfig {
        default_workspace_id: "team".to_string(),
        definitions: vec![WorkspaceDefinition::new("team", "workspace-team")],
    };

    let workspace = config.resolve(&paths(), "team", "alice").unwrap();

    assert_eq!(workspace.workspace_id, "team");
    assert_eq!(
        workspace.root,
        PathBuf::from("/tmp/talos-home/workspace-team/alice")
    );
}

#[test]
fn rejects_unsupported_workspace_id() {
    let error = WorkspaceConfig::default()
        .resolve(&paths(), "other", "alice")
        .expect_err("unsupported workspace should fail");

    assert!(error.is_code(ErrorCode::InvalidRequest));
}

#[test]
fn rejects_unsafe_user_id() {
    let error = WorkspaceConfig::default()
        .resolve(&paths(), DEFAULT_WORKSPACE_ID, "../alice")
        .expect_err("unsafe user id should fail");

    assert!(error.is_code(ErrorCode::InvalidRequest));
}

#[test]
fn rejects_duplicate_workspace_ids() {
    let config = WorkspaceConfig {
        default_workspace_id: "default".to_string(),
        definitions: vec![
            WorkspaceDefinition::new("default", "workspace-a"),
            WorkspaceDefinition::new("default", "workspace-b"),
        ],
    };

    let error = config.validate(&paths()).unwrap_err();

    assert!(error.to_string().contains("duplicate"));
}

#[test]
fn rejects_missing_default_definition() {
    let config = WorkspaceConfig {
        default_workspace_id: "missing".to_string(),
        definitions: vec![WorkspaceDefinition::new("default", "workspace-talos")],
    };

    let error = config.validate(&paths()).unwrap_err();

    assert!(error.to_string().contains("is not defined"));
}

#[test]
fn rejects_unsafe_or_overlapping_workspace_roots() {
    let unsafe_config = WorkspaceConfig {
        default_workspace_id: "default".to_string(),
        definitions: vec![WorkspaceDefinition::new("default", "../workspace")],
    };
    assert!(unsafe_config.validate(&paths()).is_err());

    let overlapping_config = WorkspaceConfig {
        default_workspace_id: "default".to_string(),
        definitions: vec![
            WorkspaceDefinition::new("default", "workspace-talos"),
            WorkspaceDefinition::new("child", "workspace-talos/child"),
        ],
    };
    assert!(overlapping_config.validate(&paths()).is_err());
}

#[test]
fn rejects_inconsistent_session_user() {
    let error = validate_session_workspace_user("user_1", "user_2")
        .expect_err("session user mismatch should fail");

    assert!(error.is_internal());
}
