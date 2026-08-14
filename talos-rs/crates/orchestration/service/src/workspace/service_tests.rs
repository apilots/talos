use super::*;
use std::path::PathBuf;
use talos_types::{
    talos::{TalosProvider, TalosSourceType},
    SessionContext, SessionState, TalosSession,
};

fn test_config() -> TalosConfig {
    let mut config = TalosConfig::default();
    config.agent.kernel.workspace_dir = "/tmp/talos-workspace".to_string();
    config
}

fn test_config_with_workspace(path: &std::path::Path) -> TalosConfig {
    let mut config = TalosConfig::default();
    config.agent.kernel.workspace_dir = path.to_string_lossy().to_string();
    config
}

#[test]
fn resolves_cloud_user_workspace() {
    let resolver = EffectiveWorkspaceResolver::new(test_config());
    let actor = Actor::regular("user_0123456789abcdef0123456789abcdef")
        .with_username(Some("alice".to_string()));

    let workspace = resolver
        .resolve_for_actor(&actor)
        .expect("cloud user id should resolve");

    assert_eq!(
        workspace.user_workspace_dir,
        PathBuf::from("/tmp/talos-workspace")
            .join("users")
            .join("user_0123456789abcdef0123456789abcdef")
    );
    assert_eq!(
        workspace.config.agent.kernel.workspace_dir,
        workspace.user_workspace_dir.to_string_lossy().to_string()
    );
}

#[test]
fn resolves_anonymous_workspace() {
    let resolver = EffectiveWorkspaceResolver::new(test_config());
    let actor = Actor::regular("anonymous");

    let workspace = resolver
        .resolve_for_actor(&actor)
        .expect("anonymous should resolve");

    assert!(workspace
        .user_workspace_dir
        .ends_with(PathBuf::from("users").join("anonymous")));
}

#[test]
fn rejects_unsafe_user_id() {
    let resolver = EffectiveWorkspaceResolver::new(test_config());
    let error = resolver
        .resolve_for_actor(&Actor::regular("../alice"))
        .expect_err("unsafe user id should fail");

    assert!(matches!(error, TalosError::ValidationError(_)));
}

#[test]
fn rejects_inconsistent_session_user() {
    let mut session = test_session("user_1");
    session.context.user_id = "user_2".to_string();

    let error =
        ensure_session_user_consistency(&session).expect_err("session user mismatch should fail");

    assert!(matches!(error, TalosError::InternalError(_)));
}

#[tokio::test]
async fn profile_snapshot_is_not_rewritten_when_identity_matches() {
    let temp_dir =
        std::env::temp_dir().join(format!("talos-profile-test-{}", uuid::Uuid::new_v4()));
    let resolver = EffectiveWorkspaceResolver::new(test_config_with_workspace(&temp_dir));
    let actor = Actor::regular("user_1").with_username(Some("alice".to_string()));
    let workspace = resolver
        .resolve_for_actor(&actor)
        .expect("workspace should resolve");
    let profile_path = workspace.user_workspace_dir.join("profile.json");
    talos_fs::create_directory(&workspace.user_workspace_dir, true)
        .expect("user workspace should be created");
    talos_fs::write_file(
        &profile_path,
        r#"{"user_id":"user_1","username":"alice","updated_at_ms":1}"#,
    )
    .await
    .expect("profile should be seeded");

    resolver
        .write_profile_snapshot(&workspace)
        .await
        .expect("profile write should succeed");

    let profile: serde_json::Value = serde_json::from_str(
        &talos_fs::read_file(&profile_path)
            .await
            .expect("profile should be readable")
            .expect("profile should exist"),
    )
    .expect("profile should be json");
    assert_eq!(profile["updated_at_ms"], 1);
    let _ = talos_fs::remove_directory(&temp_dir).await;
}

#[tokio::test]
async fn profile_snapshot_updates_when_username_changes() {
    let temp_dir =
        std::env::temp_dir().join(format!("talos-profile-test-{}", uuid::Uuid::new_v4()));
    let resolver = EffectiveWorkspaceResolver::new(test_config_with_workspace(&temp_dir));
    let actor = Actor::regular("user_1").with_username(Some("alice-renamed".to_string()));
    let workspace = resolver
        .resolve_for_actor(&actor)
        .expect("workspace should resolve");
    let profile_path = workspace.user_workspace_dir.join("profile.json");
    talos_fs::create_directory(&workspace.user_workspace_dir, true)
        .expect("user workspace should be created");
    talos_fs::write_file(
        &profile_path,
        r#"{"user_id":"user_1","username":"alice","updated_at_ms":1}"#,
    )
    .await
    .expect("profile should be seeded");

    resolver
        .write_profile_snapshot(&workspace)
        .await
        .expect("profile write should succeed");

    let profile: serde_json::Value = serde_json::from_str(
        &talos_fs::read_file(&profile_path)
            .await
            .expect("profile should be readable")
            .expect("profile should exist"),
    )
    .expect("profile should be json");
    assert_eq!(profile["user_id"], "user_1");
    assert_eq!(profile["username"], "alice-renamed");
    assert_ne!(profile["updated_at_ms"], 1);
    let _ = talos_fs::remove_directory(&temp_dir).await;
}

fn test_session(user_id: &str) -> TalosSession {
    TalosSession {
        title: "Test".to_string(),
        user_id: user_id.to_string(),
        context: SessionContext {
            session_id: "session-1".to_string(),
            source_type: TalosSourceType::Http,
            provider: TalosProvider::Http,
            user_id: user_id.to_string(),
            message_id: "message-1".to_string(),
            trace_id: "trace-1".to_string(),
            ..SessionContext::default()
        },
        conf: Default::default(),
        project_id: None,
        current_directory: None,
        config: test_config(),
        effective_user_permission_mode: talos_types::UserPermissionMode::FullAccess,
        state: SessionState::default(),
    }
}
