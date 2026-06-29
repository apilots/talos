use chrono::Utc;
use serde_json::{json, Value};
use talos_config::{resolve_effective_kernel_workspace, validate_session_workspace_user};
use talos_error::{TalosError, TalosResult};
use talos_types::{TalosConfig, TalosSession};

use super::types::EffectiveAgentWorkspace;
use crate::access::Actor;

#[derive(Clone)]
pub struct EffectiveWorkspaceResolver {
    base_config: TalosConfig,
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
        talos_fs::write_file(
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
    let Some(content) = talos_fs::read_file(path).await? else {
        return Ok(false);
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
#[path = "service_tests.rs"]
mod tests;
