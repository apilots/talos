use crate::model::{ModelSelection, TalosConfig};
use crate::talos;
use crate::user_action::UserPermissionMode;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use talos_error::TalosError;
use ts_rs::TS;

pub const DEFAULT_SESSION_TITLE: &str = "Untitled Session";

pub fn normalize_session_title(title: &str) -> Option<String> {
    let first_line = title.lines().next().unwrap_or_default().trim();
    let normalized = first_line
        .trim_matches('"')
        .trim_matches('\'')
        .trim_end_matches(['.', '!', '?', ':', ';', '。', '！', '？', '：', '；'])
        .trim();
    if normalized.is_empty() || normalized == DEFAULT_SESSION_TITLE {
        return None;
    }
    Some(
        normalized
            .chars()
            .take(48)
            .collect::<String>()
            .trim()
            .to_string(),
    )
    .filter(|value| !value.is_empty())
}

pub fn deterministic_session_title(first_user_text: &str) -> String {
    let compact = first_user_text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    normalize_session_title(&compact).unwrap_or_else(|| DEFAULT_SESSION_TITLE.to_string())
}

pub fn session_title_or_fallback(generated: String, first_user_text: &str) -> String {
    normalize_session_title(&generated)
        .unwrap_or_else(|| deterministic_session_title(first_user_text))
}

/// Session-owned usage combining cumulative token accounting with the latest context snapshot.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
pub struct SessionUsage {
    pub tokens: talos::TokenUsage,
    #[serde(default)]
    pub context: Option<ContextUsage>,
}

/// Last successfully measured model-context usage for a Session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ContextUsage {
    pub used_tokens: u32,
    pub context_window_tokens: u32,
    pub effective_prompt_budget_tokens: u32,
    #[ts(type = "number")]
    pub measured_at: i64,
}

impl ContextUsage {
    pub fn new(
        used_tokens: u32,
        context_window_tokens: u32,
        effective_prompt_budget_tokens: u32,
        measured_at: i64,
    ) -> Option<Self> {
        if context_window_tokens == 0 {
            return None;
        }
        Some(Self {
            used_tokens,
            context_window_tokens,
            effective_prompt_budget_tokens,
            measured_at,
        })
    }

    pub fn remaining_tokens(&self) -> u32 {
        self.context_window_tokens.saturating_sub(self.used_tokens)
    }

    pub fn used_percent(&self) -> f64 {
        f64::from(self.used_tokens) / f64::from(self.context_window_tokens) * 100.0
    }
}

/// Applies and validates per-session configuration overrides.
pub struct SessionConfigOverridePolicy;

impl SessionConfigOverridePolicy {
    /// Ensures overrides only contain complete provider/model pairs.
    pub fn validate_conf(conf: &SessionConf) -> crate::TalosResult<()> {
        if let Some(model_selection) = conf.model_selection.as_ref() {
            let model_ref = &model_selection.model_ref;
            if model_ref.provider.trim().is_empty() || model_ref.id.trim().is_empty() {
                return Err(talos_error::TalosError::new(
                    talos_error::ErrorCode::InvalidRequest,
                    "session model selection provider and id must not be empty".to_string(),
                ));
            }
        }
        Ok(())
    }

    /// Reads the explicit model and reasoning override stored on a session.
    pub fn extract_model_selection(
        conf: &SessionConf,
    ) -> crate::TalosResult<Option<ModelSelection>> {
        Self::validate_conf(conf)?;
        Ok(conf.model_selection.clone())
    }

    /// Builds a session override for a concrete model selection.
    pub fn single_model_selection_conf(model_selection: &ModelSelection) -> SessionConf {
        SessionConf {
            model_selection: Some(model_selection.clone()),
            user_permission_mode: None,
        }
    }

    /// Merges a partial session patch into the persisted override.
    pub fn merge_conf(base: &SessionConf, patch: &SessionConf) -> SessionConf {
        let mut merged = base.clone();
        if let Some(model_selection) = patch.model_selection.clone() {
            merged.model_selection = Some(model_selection);
        }
        if let Some(user_permission_mode) = patch.user_permission_mode {
            merged.user_permission_mode = Some(user_permission_mode);
        }
        merged
    }

    /// Resolves the effective product permission mode for a loaded Session.
    pub fn resolve_user_permission_mode(
        base_config: &TalosConfig,
        conf: &SessionConf,
    ) -> UserPermissionMode {
        conf.user_permission_mode.unwrap_or_else(|| {
            UserPermissionMode::from(
                &base_config
                    .agent
                    .kernel
                    .tools
                    .sandbox_policy
                    .user_permission_mode,
            )
        })
    }

    /// Applies session overrides on top of the current base runtime config.
    pub fn apply_to_config(
        base_config: &TalosConfig,
        conf: &SessionConf,
    ) -> crate::TalosResult<TalosConfig> {
        let mut config = base_config.clone();
        let model_selection = Self::extract_model_selection(conf)?;
        if let Some(provider) = model_selection
            .as_ref()
            .map(|selection| selection.model_ref.provider.clone())
        {
            config.llm.provider = provider;
        }
        let model = model_selection
            .map(|selection| selection.model_ref.id)
            .unwrap_or_else(|| base_config.llm.model.clone());
        config.llm.model = model;
        Ok(config)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct SessionListRequest {
    /// Omitted or null preserves the existing target selection.
    #[serde(default, rename = "nodeId")]
    #[ts(rename = "nodeId", optional)]
    pub node_id: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub offset: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct SessionListResponse {
    pub sessions: Vec<SessionInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct SessionInfo {
    #[serde(rename = "nodeId")]
    #[ts(rename = "nodeId")]
    pub node_id: String,
    pub id: String,
    pub title: String,
    pub user_id: String,
    pub source_type: talos::SessionSourceType,
    pub source_provider: talos::SessionSourceProvider,
    pub model_selection: ModelSelection,
    pub configured_default_mode: UserPermissionMode,
    pub effective_mode: UserPermissionMode,
    #[ts(type = "number")]
    pub created_at: u64,
    #[ts(type = "number")]
    pub last_activity: u64,
    #[ts(type = "number")]
    pub message_count: u64,
    #[serde(default)]
    pub usage: SessionUsage,
    #[serde(default)]
    #[ts(type = "string | null")]
    pub project_id: Option<String>,
    #[serde(default)]
    #[ts(type = "string | null")]
    pub current_directory: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct SessionGetRequest {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct SessionGetResponse {
    pub session: SessionInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct SessionDeleteRequest {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct SessionCloseRequest {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct SessionCompactRequest {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct SessionClearRequest {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct SessionActiveTurnRequest {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct SessionHistoryRequest {
    pub session_id: String,
    pub cursor: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct SessionPatchRequest {
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    pub conf: Option<SessionConf>,
    #[serde(default)]
    pub current_directory: Option<OptionalCurrentDirectoryPatch>,
}

impl SessionPatchRequest {
    pub fn into_patch(self) -> SessionPatch {
        SessionPatch {
            conf: self.conf,
            current_directory: self.current_directory,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct SessionUpdateResponse {
    pub session: SessionInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct SessionDeleteResponse {
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct SessionCloseResponse {
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct SessionClearResponse {
    pub session: SessionInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct SessionCompactResponse {
    pub session: SessionInfo,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
pub struct SessionPatch {
    #[serde(default)]
    pub conf: Option<SessionConf>,
    #[serde(default)]
    pub current_directory: Option<OptionalCurrentDirectoryPatch>,
}

impl SessionPatch {
    pub fn model_selection(&self) -> crate::TalosResult<Option<ModelSelection>> {
        let conf = self.conf.as_ref().cloned().unwrap_or_default();
        SessionConfigOverridePolicy::extract_model_selection(&conf)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OptionalCurrentDirectoryPatch {
    Set { path: PathBuf },
    Clear,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
/// Configuration override stored with a session.
#[serde(deny_unknown_fields)]
pub struct SessionConf {
    #[serde(default)]
    #[serde(rename = "modelSelection")]
    pub model_selection: Option<ModelSelection>,
    #[serde(default)]
    #[serde(rename = "userPermissionMode")]
    pub user_permission_mode: Option<UserPermissionMode>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
pub struct ProjectListRequest {
    /// Omitted or null preserves the existing target selection.
    #[serde(default, rename = "nodeId")]
    #[ts(rename = "nodeId", optional)]
    pub node_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
pub struct ProjectCreateRequest {
    /// Omitted or null preserves the existing target selection.
    #[serde(default, rename = "nodeId")]
    #[ts(rename = "nodeId", optional)]
    pub node_id: Option<String>,
    pub name: String,
    pub directory: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
pub struct ProjectReadRequest {
    pub project_id: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
pub struct ProjectPatchRequest {
    #[serde(default)]
    pub project_id: String,
    #[serde(default)]
    #[ts(optional)]
    pub name: Option<String>,
}

impl ProjectPatchRequest {
    pub fn into_patch(self) -> ProjectPatch {
        ProjectPatch { name: self.name }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
pub struct ProjectDeleteRequest {
    pub project_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
pub struct ProjectInfo {
    #[serde(rename = "nodeId")]
    #[ts(rename = "nodeId")]
    pub node_id: String,
    pub project_id: String,
    pub name: String,
    pub directory: PathBuf,
    #[serde(default)]
    pub status: ProjectStatus,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number")]
    pub updated_at: i64,
    #[ts(type = "number")]
    pub session_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
pub struct ProjectListResponse {
    pub projects: Vec<ProjectInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
pub struct ProjectReadResponse {
    pub project: ProjectInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
pub struct ProjectCreateResponse {
    pub project: ProjectInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
pub struct ProjectPatchResponse {
    pub project: ProjectInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
pub struct ProjectDeleteResponse {
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
pub struct ProjectStatus {
    pub state: ProjectStatusState,
    #[serde(default)]
    pub reason: Option<ProjectUnavailableReason>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum ProjectStatusState {
    Available,
    Missing,
    NotDirectory,
    PermissionDenied,
    #[serde(rename = "nodeUnavailable")]
    #[ts(rename = "nodeUnavailable")]
    NodeUnavailable,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum ProjectUnavailableReason {
    PathNotFound,
    NotDirectory,
    PermissionDenied,
    #[serde(rename = "nodeUnavailable")]
    #[ts(rename = "nodeUnavailable")]
    NodeUnavailable,
}

impl Default for ProjectStatus {
    fn default() -> Self {
        Self {
            state: ProjectStatusState::Available,
            reason: None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
pub struct ProjectPatch {
    #[serde(default)]
    #[ts(optional)]
    pub name: Option<String>,
}

pub fn normalize_current_directory(
    connection_mode: talos::TalosConnectionMode,
    current_directory: Option<PathBuf>,
) -> crate::TalosResult<Option<PathBuf>> {
    match connection_mode {
        talos::TalosConnectionMode::Remote => Ok(None),
        talos::TalosConnectionMode::Embedded => {
            let Some(path) = current_directory else {
                return Ok(None);
            };
            validate_project_directory_with_label(&path, "current directory")?;
            Ok(Some(path))
        }
    }
}

pub fn validate_project_directory(directory: &std::path::Path) -> crate::TalosResult<()> {
    validate_project_directory_with_label(directory, "project directory")
}

fn validate_project_directory_with_label(
    directory: &std::path::Path,
    label: &str,
) -> crate::TalosResult<()> {
    if !directory.is_absolute() {
        return Err(TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            format!(
                "{} must be an absolute path: {}",
                label,
                directory.display()
            ),
        ));
    }
    if !directory.exists() {
        return Err(TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            format!("{} does not exist: {}", label, directory.display()),
        ));
    }
    if !directory.is_dir() {
        return Err(TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            format!("{} is not a directory: {}", label, directory.display()),
        ));
    }
    Ok(())
}

/// Exhaustive persisted identity of a Primary or Subagent Session.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionTreeIdentity {
    #[default]
    Primary,
    Subagent {
        parent: crate::AgentTurnRef,
        tree_root_session_id: String,
        path: crate::AgentPath,
    },
}
