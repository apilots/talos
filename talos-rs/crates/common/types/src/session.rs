use crate::model::TalosConfig;
use crate::talos;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use talos_error::TalosError;

/// Session record stored by the gateway.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    pub id: String,
    pub title: String,
    pub user_id: String,
    pub context: talos::TalosContext,
    pub model: String,
    pub created_at: u64,
    pub last_activity: u64,
    pub message_count: u64,
    #[serde(default)]
    pub usage: talos::TalosUsage,
    #[serde(default)]
    pub conf: SessionConf,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub current_directory: Option<PathBuf>,
}

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

impl SessionRecord {
    pub fn from_session(session: &TalosSession, base_model: &str) -> Self {
        let effective_model = session.config.llm.model.clone();
        let conf = SessionConfigOverridePolicy::build_conf_for_model(base_model, &effective_model);

        Self {
            id: session.context.session_id.clone(),
            title: session.title.clone(),
            user_id: session.user_id.clone(),
            context: talos::TalosContext {
                source_type: session.context.source_type.clone(),
                provider: session.context.provider.clone(),
                user_id: session.context.user_id.clone(),
                message_id: session.context.message_id.clone(),
                trace_id: session.context.trace_id.clone(),
                channel_id: session.context.channel_id.clone(),
                thread_id: session.context.thread_id.clone(),
                project_root: session.context.project_root.clone(),
                environment: session.context.environment.clone(),
            },
            model: effective_model,
            created_at: session.state.created_at,
            last_activity: session.state.last_activity,
            message_count: session.state.message_count,
            usage: session.state.usage.clone(),
            conf,
            project_id: session.project_id.clone(),
            current_directory: session.current_directory.clone(),
        }
    }

    pub fn has_real_title(&self) -> bool {
        let normalized = self.title.trim();
        !normalized.is_empty() && normalized != DEFAULT_SESSION_TITLE
    }
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

impl SessionContext {
    pub fn from_record(record: &SessionRecord, workspace_id: Option<String>) -> Self {
        Self {
            session_id: record.id.clone(),
            workspace_id,
            source_type: record.context.source_type.clone(),
            provider: record.context.provider.clone(),
            user_id: record.context.user_id.clone(),
            message_id: record.context.message_id.clone(),
            trace_id: record.context.trace_id.clone(),
            channel_id: record.context.channel_id.clone(),
            thread_id: record.context.thread_id.clone(),
            project_id: record.project_id.clone(),
            current_directory: record.current_directory.clone(),
            project_root: record.context.project_root.clone(),
            environment: record.context.environment.clone(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionState {
    pub created_at: u64,
    pub last_activity: u64,
    pub message_count: u64,
    #[serde(default)]
    pub usage: talos::TalosUsage,
}

impl SessionState {
    pub fn from_record(record: &SessionRecord) -> Self {
        Self {
            created_at: record.created_at,
            last_activity: record.last_activity,
            message_count: record.message_count,
            usage: record.usage.clone(),
        }
    }
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

pub struct SessionConfigOverridePolicy;

impl SessionConfigOverridePolicy {
    pub fn validate_conf(_conf: &SessionConf) -> crate::TalosResult<()> {
        Ok(())
    }

    pub fn extract_agent_model(conf: &SessionConf) -> crate::TalosResult<Option<String>> {
        Self::validate_conf(conf)?;
        Ok(conf.agent.as_ref().and_then(|agent| agent.model.clone()))
    }

    pub fn build_conf_for_model(base_model: &str, effective_model: &str) -> SessionConf {
        if effective_model == base_model {
            return SessionConf::default();
        }
        Self::single_model_conf(effective_model)
    }

    pub fn single_model_conf(model: &str) -> SessionConf {
        SessionConf {
            agent: Some(SessionAgentConf {
                model: Some(model.to_string()),
            }),
        }
    }

    pub fn merge_conf(base: &SessionConf, patch: &SessionConf) -> SessionConf {
        let mut merged = base.clone();

        if let Some(agent_patch) = patch.agent.as_ref() {
            let agent = merged.agent.get_or_insert_with(SessionAgentConf::default);
            if let Some(model) = agent_patch.model.clone() {
                agent.model = Some(model);
            }
        }

        merged
    }

    pub fn apply_to_config(
        base_config: &TalosConfig,
        record_model: &str,
        conf: &SessionConf,
    ) -> crate::TalosResult<TalosConfig> {
        let mut config = base_config.clone();
        let model = Self::extract_agent_model(conf)?.unwrap_or_else(|| record_model.to_string());
        config.agent.model = model.clone();
        config.llm.model = model;
        Ok(config)
    }
}

impl TalosSession {
    pub fn restore(
        record: &SessionRecord,
        base_config: &TalosConfig,
        workspace_id: Option<String>,
    ) -> crate::TalosResult<Self> {
        let config =
            SessionConfigOverridePolicy::apply_to_config(base_config, &record.model, &record.conf)?;

        Ok(Self {
            title: record.title.clone(),
            user_id: record.user_id.clone(),
            context: SessionContext::from_record(record, workspace_id),
            conf: record.conf.clone(),
            project_id: record.project_id.clone(),
            current_directory: record.current_directory.clone(),
            config,
            state: SessionState::from_record(record),
        })
    }

    pub fn from_record(
        record: &SessionRecord,
        base_config: &TalosConfig,
        workspace_id: Option<String>,
    ) -> crate::TalosResult<Self> {
        Self::restore(record, base_config, workspace_id)
    }

    pub fn has_real_title(&self) -> bool {
        let normalized = self.title.trim();
        !normalized.is_empty() && normalized != DEFAULT_SESSION_TITLE
    }

    pub fn apply_turn_progress(
        &mut self,
        assistant_model: &str,
        message_delta: u64,
        usage: &talos::TalosUsage,
    ) {
        self.state.last_activity = crate::now();
        self.state.message_count = self.state.message_count.saturating_add(message_delta);
        self.state.usage.accumulate(usage);
        self.config.agent.model = assistant_model.to_string();
        self.config.llm.model = assistant_model.to_string();
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionQuery {
    pub user_id: Option<String>,
    pub channel_id: Option<String>,
    pub provider: Option<String>,
    pub project_id: Option<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionPatch {
    #[serde(default)]
    pub conf: Option<SessionConf>,
    #[serde(default)]
    pub project_id: Option<OptionalProjectIdPatch>,
    #[serde(default)]
    pub current_directory: Option<OptionalCurrentDirectoryPatch>,
}

impl SessionPatch {
    pub fn agent_model(&self) -> crate::TalosResult<Option<String>> {
        let conf = self.conf.as_ref().cloned().unwrap_or_default();
        SessionConfigOverridePolicy::extract_agent_model(&conf)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OptionalProjectIdPatch {
    Set { project_id: String },
    Clear,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OptionalCurrentDirectoryPatch {
    Set { path: PathBuf },
    Clear,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionAgentConf {
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionConf {
    #[serde(default)]
    pub agent: Option<SessionAgentConf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectRecord {
    pub project_id: String,
    pub name: String,
    pub user_id: String,
    pub directory: PathBuf,
    pub created_at: i64,
    pub updated_at: i64,
    pub session_count: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectPatch {
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionPatchAck {
    pub success: bool,
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

#[async_trait]
pub trait SessionRuntime: Send + Sync {
    async fn resolve_target_agent(
        &self,
        _context: &talos::TalosContext,
    ) -> crate::TalosResult<String> {
        Ok("default".to_string())
    }

    async fn invalidate_cached_session(&self, _session_id: &str) -> crate::TalosResult<()> {
        Ok(())
    }

    async fn compact_session(&self, session_id: &str) -> crate::TalosResult<TalosSession>;

    async fn clear_session(&self, session_id: &str) -> crate::TalosResult<TalosSession>;

    async fn close_session(&self, session_id: &str) -> crate::TalosResult<()>;

    async fn delete_session(&self, session_id: &str) -> crate::TalosResult<SessionPatchAck>;

    async fn cancel_active_turn(&self, session_id: &str) -> crate::TalosResult<()>;

    async fn submit_dynamic_tool_result(
        &self,
        session_id: &str,
        call_id: &str,
        tool_name: &str,
        output: String,
        success: bool,
    ) -> crate::TalosResult<()>;
}

#[async_trait]
pub trait SessionService: Send + Sync {
    async fn resolve_target_agent(
        &self,
        _context: &talos::TalosContext,
    ) -> crate::TalosResult<String> {
        Ok("default".to_string())
    }

    async fn create_session(
        &self,
        context: talos::TalosContext,
        model: Option<String>,
        conf: Option<SessionConf>,
        project_id: Option<String>,
        current_directory: Option<PathBuf>,
    ) -> crate::TalosResult<TalosSession>;

    async fn list_sessions(&self, query: SessionQuery) -> crate::TalosResult<Vec<TalosSession>>;

    async fn count_sessions(&self, query: SessionQuery) -> crate::TalosResult<usize>;

    async fn get_session(&self, session_id: &str) -> crate::TalosResult<Option<TalosSession>>;

    async fn invalidate_cached_session(&self, _session_id: &str) -> crate::TalosResult<()> {
        Ok(())
    }

    async fn patch_session(
        &self,
        session_id: &str,
        patch: SessionPatch,
    ) -> crate::TalosResult<SessionPatchAck>;

    async fn compact_session(&self, session_id: &str) -> crate::TalosResult<TalosSession>;

    async fn clear_session(&self, session_id: &str) -> crate::TalosResult<TalosSession>;

    async fn close_session(&self, session_id: &str) -> crate::TalosResult<()>;

    async fn delete_session(&self, session_id: &str) -> crate::TalosResult<SessionPatchAck>;

    async fn cancel_active_turn(&self, session_id: &str) -> crate::TalosResult<()>;

    async fn submit_dynamic_tool_result(
        &self,
        session_id: &str,
        call_id: &str,
        tool_name: &str,
        output: String,
        success: bool,
    ) -> crate::TalosResult<()>;

    async fn list_projects(&self) -> crate::TalosResult<Vec<ProjectRecord>>;

    async fn get_project(&self, project_id: &str) -> crate::TalosResult<Option<ProjectRecord>>;

    async fn create_project(
        &self,
        name: String,
        directory: PathBuf,
        user_id: String,
    ) -> crate::TalosResult<ProjectRecord>;

    async fn patch_project(
        &self,
        project_id: &str,
        patch: ProjectPatch,
    ) -> crate::TalosResult<Option<ProjectRecord>>;

    async fn delete_project(&self, project_id: &str) -> crate::TalosResult<()>;
}
