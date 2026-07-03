use crate::model::TalosConfig;
use crate::talos;
use crate::turn_api::TimelineActiveTurnSnapshot;
use crate::ModelRef;
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

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionContext {
    pub session_id: String,
    #[serde(default)]
    pub workspace_id: Option<String>,
    pub source_type: talos::TalosSourceType,
    pub provider: talos::TalosProvider,
    pub user_id: String,
    pub message_id: String,
    pub trace_id: String,
    #[serde(default)]
    pub channel_id: Option<String>,
    #[serde(default)]
    pub thread_id: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub current_directory: Option<PathBuf>,
    #[serde(skip_serializing, skip_deserializing)]
    pub project_root: Option<PathBuf>,
    #[serde(default)]
    pub environment: Option<talos::TalosSessionEnvironment>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionState {
    pub created_at: u64,
    pub last_activity: u64,
    pub message_count: u64,
    #[serde(default)]
    pub usage: talos::TokenUsage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TalosSession {
    pub title: String,
    pub user_id: String,
    pub context: SessionContext,
    #[serde(default)]
    pub conf: SessionConf,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub current_directory: Option<PathBuf>,
    pub config: TalosConfig,
    pub state: SessionState,
}

/// Applies and validates per-session configuration overrides.
pub struct SessionConfigOverridePolicy;

impl SessionConfigOverridePolicy {
    /// Ensures overrides only contain complete provider/model pairs.
    pub fn validate_conf(conf: &SessionConf) -> crate::TalosResult<()> {
        if let Some(model_ref) = conf.model_ref.as_ref() {
            if model_ref.provider.trim().is_empty() || model_ref.id.trim().is_empty() {
                return Err(talos_error::TalosError::ValidationError(
                    "session model_ref provider and id must not be empty".to_string(),
                ));
            }
        }
        Ok(())
    }

    /// Reads the explicit provider/model override stored on a session.
    pub fn extract_model_ref(conf: &SessionConf) -> crate::TalosResult<Option<ModelRef>> {
        Self::validate_conf(conf)?;
        Ok(conf.model_ref.clone())
    }

    /// Builds a session override for a concrete provider/model pair.
    pub fn single_model_ref_conf(model_ref: &ModelRef) -> SessionConf {
        SessionConf {
            model_ref: Some(model_ref.clone()),
        }
    }

    /// Merges a partial session patch into the persisted override.
    pub fn merge_conf(base: &SessionConf, patch: &SessionConf) -> SessionConf {
        let mut merged = base.clone();
        if let Some(model_ref) = patch.model_ref.clone() {
            merged.model_ref = Some(model_ref);
        }
        merged
    }

    /// Applies session overrides on top of the current base runtime config.
    pub fn apply_to_config(
        base_config: &TalosConfig,
        conf: &SessionConf,
    ) -> crate::TalosResult<TalosConfig> {
        let mut config = base_config.clone();
        let model_ref = Self::extract_model_ref(conf)?;
        if let Some(provider) = model_ref
            .as_ref()
            .map(|model_ref| model_ref.provider.clone())
        {
            config.llm.provider = provider;
        }
        let model = model_ref
            .map(|model_ref| model_ref.id)
            .unwrap_or_else(|| base_config.llm.model.clone());
        config.llm.model = model;
        Ok(config)
    }
}

impl TalosSession {
    pub fn has_real_title(&self) -> bool {
        let normalized = self.title.trim();
        !normalized.is_empty() && normalized != DEFAULT_SESSION_TITLE
    }

    pub fn apply_turn_progress(
        &mut self,
        assistant_model: &str,
        message_delta: u64,
        usage: &talos::TokenUsage,
    ) {
        self.state.last_activity = crate::now();
        self.state.message_count = self.state.message_count.saturating_add(message_delta);
        self.state.usage.accumulate(usage);
        self.config.llm.model = assistant_model.to_string();
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
pub struct SessionListRequest {
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub offset: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema, TS)]
pub struct SessionListResponse {
    pub sessions: Vec<SessionInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema, TS)]
pub struct SessionInfo {
    pub id: String,
    pub title: String,
    pub user_id: String,
    pub source_type: talos::TalosSourceType,
    pub provider: talos::TalosProvider,
    pub model_ref: ModelRef,
    #[ts(type = "number")]
    pub created_at: u64,
    #[ts(type = "number")]
    pub last_activity: u64,
    #[ts(type = "number")]
    pub message_count: u64,
    #[serde(default)]
    pub usage: talos::TokenUsage,
    #[serde(default)]
    #[ts(optional)]
    pub project_id: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub current_directory: Option<PathBuf>,
}

impl From<&TalosSession> for SessionInfo {
    fn from(session: &TalosSession) -> Self {
        Self {
            id: session.context.session_id.clone(),
            title: session.title.clone(),
            user_id: session.user_id.clone(),
            source_type: session.context.source_type.clone(),
            provider: session.context.provider.clone(),
            model_ref: ModelRef {
                provider: session.config.llm.provider.clone(),
                id: session.config.llm.model.clone(),
            },
            created_at: session.state.created_at,
            last_activity: session.state.last_activity,
            message_count: session.state.message_count,
            usage: session.state.usage.clone(),
            project_id: session.project_id.clone(),
            current_directory: session.current_directory.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
pub struct SessionGetRequest {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema, TS)]
pub struct SessionGetResponse {
    pub session: SessionInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
pub struct SessionDeleteRequest {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
pub struct SessionCloseRequest {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
pub struct SessionCompactRequest {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
pub struct SessionClearRequest {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
pub struct SessionActiveTurnRequest {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema, TS)]
pub struct SessionActiveTurnResponse {
    pub active_turn: TimelineActiveTurnSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
pub struct SessionHistoryRequest {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema, TS)]
pub struct SessionHistoryResponse {
    pub history: talos::SessionHistorySnapshot,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
pub struct SessionPatchRequest {
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    pub conf: Option<SessionConf>,
    #[serde(default)]
    pub project_id: Option<OptionalProjectIdPatch>,
    #[serde(default)]
    pub current_directory: Option<OptionalCurrentDirectoryPatch>,
}

impl SessionPatchRequest {
    pub fn into_patch(self) -> SessionPatch {
        SessionPatch {
            conf: self.conf,
            project_id: self.project_id,
            current_directory: self.current_directory,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema, TS)]
pub struct SessionUpdateResponse {
    pub session: SessionInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
pub struct SessionDeleteResponse {
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
pub struct SessionCloseResponse {
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema, TS)]
pub struct SessionClearResponse {
    pub session: SessionInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema, TS)]
pub struct SessionCompactResponse {
    pub session: SessionInfo,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
pub struct SessionPatch {
    #[serde(default)]
    pub conf: Option<SessionConf>,
    #[serde(default)]
    pub project_id: Option<OptionalProjectIdPatch>,
    #[serde(default)]
    pub current_directory: Option<OptionalCurrentDirectoryPatch>,
}

impl SessionPatch {
    pub fn model_ref(&self) -> crate::TalosResult<Option<ModelRef>> {
        let conf = self.conf.as_ref().cloned().unwrap_or_default();
        SessionConfigOverridePolicy::extract_model_ref(&conf)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OptionalProjectIdPatch {
    Set { project_id: String },
    Clear,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OptionalCurrentDirectoryPatch {
    Set { path: PathBuf },
    Clear,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
/// Configuration override stored with a session.
pub struct SessionConf {
    #[serde(default)]
    pub model_ref: Option<ModelRef>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
pub struct ProjectListRequest {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
pub struct ProjectCreateRequest {
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
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum ProjectUnavailableReason {
    PathNotFound,
    NotDirectory,
    PermissionDenied,
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

pub fn connection_mode_from_context(context: &talos::TalosContext) -> talos::TalosConnectionMode {
    context
        .environment
        .as_ref()
        .map(|environment| environment.connection_mode)
        .unwrap_or_default()
}

pub fn normalize_current_directory(
    context: &talos::TalosContext,
    current_directory: Option<PathBuf>,
) -> crate::TalosResult<Option<PathBuf>> {
    match connection_mode_from_context(context) {
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

pub fn resolve_effective_project_root(
    session: &TalosSession,
    project_directory: Option<PathBuf>,
    workspace_root: Option<PathBuf>,
) -> Option<PathBuf> {
    if let Some(directory) = project_directory {
        return Some(directory);
    }

    if matches!(
        session
            .context
            .environment
            .as_ref()
            .map(|environment| environment.connection_mode),
        Some(talos::TalosConnectionMode::Embedded)
    ) {
        if let Some(current_directory) = session.current_directory.clone() {
            return Some(current_directory);
        }
    }

    workspace_root
}

fn validate_project_directory_with_label(
    directory: &std::path::Path,
    label: &str,
) -> crate::TalosResult<()> {
    if !directory.is_absolute() {
        return Err(TalosError::ValidationError(format!(
            "{} must be an absolute path: {}",
            label,
            directory.display()
        )));
    }
    if !directory.exists() {
        return Err(TalosError::ValidationError(format!(
            "{} does not exist: {}",
            label,
            directory.display()
        )));
    }
    if !directory.is_dir() {
        return Err(TalosError::ValidationError(format!(
            "{} is not a directory: {}",
            label,
            directory.display()
        )));
    }
    Ok(())
}
