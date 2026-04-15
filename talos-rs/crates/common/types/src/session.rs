use crate::model::TalosConfig;
use crate::talos;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Session record stored by the gateway.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    pub id: String,
    pub title: String,
    pub context: talos::TalosContext,
    pub model: String,
    pub created_at: u64,
    pub last_activity: u64,
    pub message_count: u64,
    #[serde(default)]
    pub usage: talos::TalosUsage,
    #[serde(default)]
    pub conf: HashMap<String, serde_json::Value>,
}

pub const DEFAULT_SESSION_TITLE: &str = "Untitled Session";

impl SessionRecord {
    pub fn from_session(session: &TalosSession, base_model: &str) -> Self {
        let effective_model = session.config.llm.model.clone();
        let conf = SessionConfigOverridePolicy::build_conf_for_model(base_model, &effective_model);

        Self {
            id: session.context.session_id.clone(),
            title: session.title.clone(),
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
    pub context: SessionContext,
    pub config: TalosConfig,
    pub state: SessionState,
}

pub struct SessionConfigOverridePolicy;

impl SessionConfigOverridePolicy {
    pub const AGENT_MODEL_KEY: &'static str = "agent.model";

    pub fn validate_conf(conf: &HashMap<String, serde_json::Value>) -> crate::TalosResult<()> {
        for key in conf.keys() {
            if key != Self::AGENT_MODEL_KEY {
                return Err(crate::TalosError::ValidationError(format!(
                    "unsupported session config key: {}",
                    key
                )));
            }
        }

        if let Some(value) = conf.get(Self::AGENT_MODEL_KEY) {
            if !value.is_string() {
                return Err(crate::TalosError::ValidationError(
                    "agent.model must be a string".to_string(),
                ));
            }
        }

        Ok(())
    }

    pub fn extract_agent_model(
        conf: &HashMap<String, serde_json::Value>,
    ) -> crate::TalosResult<Option<String>> {
        Self::validate_conf(conf)?;
        Ok(conf
            .get(Self::AGENT_MODEL_KEY)
            .and_then(|value| value.as_str())
            .map(ToString::to_string))
    }

    pub fn build_conf_for_model(
        base_model: &str,
        effective_model: &str,
    ) -> HashMap<String, serde_json::Value> {
        if effective_model == base_model {
            return HashMap::new();
        }

        let mut conf = HashMap::new();
        conf.insert(
            Self::AGENT_MODEL_KEY.to_string(),
            serde_json::Value::String(effective_model.to_string()),
        );
        conf
    }

    pub fn single_model_conf(model: &str) -> HashMap<String, serde_json::Value> {
        let mut conf = HashMap::new();
        conf.insert(
            Self::AGENT_MODEL_KEY.to_string(),
            serde_json::Value::String(model.to_string()),
        );
        conf
    }

    pub fn apply_to_config(
        base_config: &TalosConfig,
        record_model: &str,
        conf: &HashMap<String, serde_json::Value>,
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
            context: SessionContext::from_record(record, workspace_id),
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
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionQuery {
    pub user_id: Option<String>,
    pub channel_id: Option<String>,
    pub provider: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionPatch {
    #[serde(default)]
    pub conf: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub project_root: Option<SessionProjectRootPatch>,
}

impl SessionPatch {
    pub fn agent_model(&self) -> crate::TalosResult<Option<String>> {
        SessionConfigOverridePolicy::extract_agent_model(&self.conf)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionProjectRootPatch {
    Set { path: PathBuf },
    Clear,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionPatchAck {
    pub success: bool,
}

#[async_trait]
pub trait SessionService: Send + Sync {
    async fn create_session(
        &self,
        context: talos::TalosContext,
        model: Option<String>,
    ) -> crate::TalosResult<TalosSession>;

    async fn list_sessions(&self, query: SessionQuery) -> crate::TalosResult<Vec<TalosSession>>;

    async fn get_session(&self, session_id: &str) -> crate::TalosResult<Option<TalosSession>>;

    async fn patch_session(
        &self,
        session_id: &str,
        patch: SessionPatch,
    ) -> crate::TalosResult<SessionPatchAck>;

    async fn close_session(&self, session_id: &str) -> crate::TalosResult<()>;

    async fn cancel_active_turn(&self, session_id: &str) -> crate::TalosResult<()>;

    async fn submit_tool_approval(
        &self,
        session_id: &str,
        call_id: &str,
        tool_name: &str,
        approved: bool,
        reason: Option<String>,
    ) -> crate::TalosResult<()>;

    async fn submit_dynamic_tool_result(
        &self,
        session_id: &str,
        call_id: &str,
        tool_name: &str,
        output: String,
        success: bool,
    ) -> crate::TalosResult<()>;

    async fn get_session_turns(
        &self,
        session_id: &str,
    ) -> crate::TalosResult<Vec<talos::TalosMessageResponse>>;
}
