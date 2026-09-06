use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use talos_error::{ErrorCode, TalosError, TalosResult};

pub const DEFAULT_WORKSPACE_ID: &str = "default";
const DEFAULT_WORKSPACE_ROOT_PREFIX: &str = "workspace-talos";

/// Product workspace configuration and its default routing target.
///
/// This object is serialized in the product configuration. Runtime layers must
/// resolve definitions through `Config` instead of inventing Workspace IDs or
/// filesystem paths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceConfig {
    pub default_workspace_id: String,
    pub definitions: Vec<WorkspaceDefinition>,
}

impl Default for WorkspaceConfig {
    fn default() -> Self {
        Self {
            default_workspace_id: DEFAULT_WORKSPACE_ID.to_string(),
            definitions: vec![WorkspaceDefinition::new(
                DEFAULT_WORKSPACE_ID,
                DEFAULT_WORKSPACE_ROOT_PREFIX,
            )],
        }
    }
}

impl WorkspaceConfig {
    pub fn definition(&self, workspace_id: &str) -> Option<&WorkspaceDefinition> {
        self.definitions
            .iter()
            .find(|definition| definition.workspace_id == workspace_id)
    }

    pub fn default_definition(&self) -> TalosResult<&WorkspaceDefinition> {
        self.definition(&self.default_workspace_id).ok_or_else(|| {
            TalosError::new(
                ErrorCode::InvalidRequest,
                format!(
                    "workspaces.default_workspace_id '{}' is not defined",
                    self.default_workspace_id
                ),
            )
        })
    }

    pub fn resolve(
        &self,
        paths: &crate::TalosPaths,
        workspace_id: &str,
        user_id: &str,
    ) -> TalosResult<EffectiveWorkspace> {
        let definition = self.definition(workspace_id).ok_or_else(|| {
            TalosError::new(
                ErrorCode::InvalidRequest,
                format!("unsupported workspace_id: {workspace_id}"),
            )
        })?;
        definition.resolve(paths, user_id)
    }

    pub fn root_prefix(
        &self,
        paths: &crate::TalosPaths,
        workspace_id: &str,
    ) -> TalosResult<PathBuf> {
        self.definition(workspace_id)
            .map(|definition| definition.resolved_root_prefix(paths))
            .ok_or_else(|| {
                TalosError::new(
                    ErrorCode::InvalidRequest,
                    format!("unsupported workspace_id: {workspace_id}"),
                )
            })
    }

    pub fn validate(&self, paths: &crate::TalosPaths) -> TalosResult<()> {
        if self.definitions.is_empty() {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "workspaces.definitions must contain at least one workspace".to_string(),
            ));
        }
        validate_workspace_id(&self.default_workspace_id)?;

        let mut ids = HashSet::new();
        let mut roots: Vec<(&str, PathBuf)> = Vec::with_capacity(self.definitions.len());
        for definition in &self.definitions {
            validate_workspace_id(&definition.workspace_id)?;
            if !ids.insert(definition.workspace_id.as_str()) {
                return Err(TalosError::new(
                    ErrorCode::InvalidRequest,
                    format!(
                        "duplicate workspaces.definitions workspace_id: {}",
                        definition.workspace_id
                    ),
                ));
            }
            validate_root_prefix(&definition.root_prefix)?;
            let root = definition.resolved_root_prefix(paths);
            if let Some((other_id, _)) = roots
                .iter()
                .find(|(_, other)| root.starts_with(other) || other.starts_with(&root))
            {
                return Err(TalosError::new(
                    ErrorCode::InvalidRequest,
                    format!(
                        "workspace root_prefix overlaps between '{}' and '{}'",
                        other_id, definition.workspace_id
                    ),
                ));
            }
            roots.push((&definition.workspace_id, root));
        }
        self.default_definition()?;
        Ok(())
    }
}

/// One configured logical Workspace and its filesystem prefix.
///
/// `root_prefix` is configuration owned by the product. It contains no User or
/// Agent identity and is resolved relative to `TALOS_HOME` when not absolute.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceDefinition {
    pub workspace_id: String,
    pub root_prefix: PathBuf,
}

impl WorkspaceDefinition {
    pub fn new(workspace_id: impl Into<String>, root_prefix: impl Into<PathBuf>) -> Self {
        Self {
            workspace_id: workspace_id.into(),
            root_prefix: root_prefix.into(),
        }
    }

    pub fn resolved_root_prefix(&self, paths: &crate::TalosPaths) -> PathBuf {
        if self.root_prefix.is_absolute() {
            self.root_prefix.clone()
        } else {
            paths.talos_home.join(&self.root_prefix)
        }
    }

    fn resolve(&self, paths: &crate::TalosPaths, user_id: &str) -> TalosResult<EffectiveWorkspace> {
        let safe_user_id = validate_user_workspace_segment(user_id)?;
        Ok(EffectiveWorkspace {
            workspace_id: self.workspace_id.clone(),
            user_id: safe_user_id.to_string(),
            root: self.resolved_root_prefix(paths).join(safe_user_id),
        })
    }
}

/// Authorized runtime workspace for one logical workspace and trusted user.
///
/// Runtime layers must pass this object through unchanged instead of deriving a
/// path from Agent configuration or accepting a client-provided path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectiveWorkspace {
    pub workspace_id: String,
    pub user_id: String,
    pub root: PathBuf,
}

fn validate_workspace_id(workspace_id: &str) -> TalosResult<()> {
    let trimmed = workspace_id.trim();
    if trimmed.is_empty()
        || trimmed != workspace_id
        || !trimmed
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        return Err(TalosError::new(
            ErrorCode::InvalidRequest,
            format!("invalid workspace_id: {workspace_id}"),
        ));
    }
    Ok(())
}

fn validate_root_prefix(root_prefix: &Path) -> TalosResult<()> {
    if root_prefix.as_os_str().is_empty()
        || root_prefix
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
    {
        return Err(TalosError::new(
            ErrorCode::InvalidRequest,
            format!("workspace root_prefix is unsafe: {}", root_prefix.display()),
        ));
    }
    Ok(())
}

pub fn validate_user_workspace_segment(user_id: &str) -> TalosResult<&str> {
    let trimmed = user_id.trim();
    if trimmed.is_empty() {
        return Err(TalosError::new(
            ErrorCode::InvalidRequest,
            "user_id cannot be empty for workspace resolution".to_string(),
        ));
    }
    if trimmed != user_id {
        return Err(TalosError::new(
            ErrorCode::InvalidRequest,
            "user_id must not contain leading or trailing whitespace".to_string(),
        ));
    }
    if !trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        return Err(TalosError::new(
            ErrorCode::InvalidRequest,
            format!("user_id is not path-safe: {user_id}"),
        ));
    }
    Ok(trimmed)
}

pub fn validate_session_workspace_user<'a>(
    session_user_id: &'a str,
    context_user_id: &str,
) -> TalosResult<&'a str> {
    if session_user_id != context_user_id {
        return Err(TalosError::internal(
            "session user_id does not match session context user_id",
        ));
    }
    validate_user_workspace_segment(session_user_id)
}

#[cfg(test)]
#[path = "workspace_tests.rs"]
mod tests;
