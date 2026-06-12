use std::path::PathBuf;

use chrono::Utc;
use serde_json::{json, Value};
use talos_config::{resolve_effective_kernel_workspace, validate_session_workspace_user};
use talos_error::{TalosError, TalosResult};
use talos_types::{TalosConfig, TalosSession};

use crate::access::Actor;
use crate::file::write_atomic;

#[derive(Clone)]
pub struct EffectiveWorkspaceResolver {
    base_config: TalosConfig,
}

#[derive(Debug, Clone)]
pub struct EffectiveAgentWorkspace {
    pub user_id: String,
    pub username: Option<String>,
    pub base_workspace_dir: PathBuf,
    pub user_workspace_dir: PathBuf,
    pub config: TalosConfig,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AgentWorkspaceInfo {
    pub user_id: String,
    pub username: Option<String>,
    pub base_workspace_dir: String,
    pub user_workspace_dir: String,
}

impl From<EffectiveAgentWorkspace> for AgentWorkspaceInfo {
    fn from(workspace: EffectiveAgentWorkspace) -> Self {
        Self {
            user_id: workspace.user_id,
            username: workspace.username,
            base_workspace_dir: workspace.base_workspace_dir.to_string_lossy().to_string(),
            user_workspace_dir: workspace.user_workspace_dir.to_string_lossy().to_string(),
        }
    }
}

impl EffectiveWorkspaceResolver {
    pub fn new(base_config: TalosConfig) -> Self {
        Self { base_config }
    }

    pub fn resolve_for_actor(&self, actor: &Actor) -> TalosResult<EffectiveAgentWorkspace> {
        self.resolve_for_user_id(&actor.user_id, actor.username.clone())
    }

    pub fn resolve_for_session(
        &self,
        session: &TalosSession,
    ) -> TalosResult<EffectiveAgentWorkspace> {
        ensure_session_user_consistency(session)?;
        self.resolve_for_user_id(&session.user_id, None)
    }

    pub async fn write_profile_snapshot(
        &self,
        workspace: &EffectiveAgentWorkspace,
    ) -> TalosResult<()> {
        let path = workspace.user_workspace_dir.join("profile.json");
        if profile_snapshot_matches(&path, workspace).await? {
            return Ok(());
        }
        let payload = json!({
            "user_id": workspace.user_id,
            "username": workspace.username,
            "updated_at_ms": Utc::now().timestamp_millis(),
        });
        write_atomic(
            &path,
            &serde_json::to_string_pretty(&payload)
                .map_err(|err| TalosError::InternalError(err.to_string()))?,
        )
        .await
    }

    fn resolve_for_user_id(
        &self,
        user_id: &str,
        username: Option<String>,
    ) -> TalosResult<EffectiveAgentWorkspace> {
        let resolved = resolve_effective_kernel_workspace(&self.base_config.agent.kernel, user_id)?;
        let mut config = self.base_config.clone();
        config.agent.kernel = resolved.kernel_config.clone();

        Ok(EffectiveAgentWorkspace {
            user_id: resolved.user_id,
            username,
            base_workspace_dir: resolved.base_workspace_dir,
            user_workspace_dir: resolved.user_workspace_dir,
            config,
        })
    }
}

pub fn ensure_session_user_consistency(session: &TalosSession) -> TalosResult<()> {
    validate_session_workspace_user(&session.user_id, &session.context.user_id)?;
    Ok(())
}

async fn profile_snapshot_matches(
    path: &std::path::Path,
    workspace: &EffectiveAgentWorkspace,
) -> TalosResult<bool> {
    let content = match tokio::fs::read_to_string(path).await {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => {
            return Err(TalosError::StorageError(format!(
                "read profile snapshot {}: {}",
                path.display(),
                err
            )));
        }
    };
    let value: Value = match serde_json::from_str(&content) {
        Ok(value) => value,
        Err(_) => return Ok(false),
    };
    let username_matches = match &workspace.username {
        Some(username) => value.get("username").and_then(Value::as_str) == Some(username.as_str()),
        None => value.get("username").is_some_and(Value::is_null),
    };
    Ok(
        value.get("user_id").and_then(Value::as_str) == Some(workspace.user_id.as_str())
            && username_matches,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
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

        let error = ensure_session_user_consistency(&session)
            .expect_err("session user mismatch should fail");

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
        std::fs::create_dir_all(&workspace.user_workspace_dir)
            .expect("user workspace should be created");
        std::fs::write(
            &profile_path,
            r#"{"user_id":"user_1","username":"alice","updated_at_ms":1}"#,
        )
        .expect("profile should be seeded");

        resolver
            .write_profile_snapshot(&workspace)
            .await
            .expect("profile write should succeed");

        let profile: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(&profile_path).expect("profile should be readable"),
        )
        .expect("profile should be json");
        assert_eq!(profile["updated_at_ms"], 1);
        let _ = std::fs::remove_dir_all(temp_dir);
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
        std::fs::create_dir_all(&workspace.user_workspace_dir)
            .expect("user workspace should be created");
        std::fs::write(
            &profile_path,
            r#"{"user_id":"user_1","username":"alice","updated_at_ms":1}"#,
        )
        .expect("profile should be seeded");

        resolver
            .write_profile_snapshot(&workspace)
            .await
            .expect("profile write should succeed");

        let profile: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(&profile_path).expect("profile should be readable"),
        )
        .expect("profile should be json");
        assert_eq!(profile["user_id"], "user_1");
        assert_eq!(profile["username"], "alice-renamed");
        assert_ne!(profile["updated_at_ms"], 1);
        let _ = std::fs::remove_dir_all(temp_dir);
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
            state: SessionState::default(),
        }
    }
}
