use async_trait::async_trait;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use talos_error::TalosResult;
use talos_storage::{Row, Storage};
use talos_types::talos::TalosUsage;
use talos_types::talos::{TalosContext, TalosProvider, TalosSourceType};
use talos_types::{
    OptionalCurrentDirectoryPatch, OptionalProjectIdPatch, ProjectPatch, ProjectRecord,
    SessionConf, SessionConfigOverridePolicy, SessionPatch, SessionPatchAck, SessionQuery,
    SessionRecord, DEFAULT_SESSION_TITLE,
};

pub mod active_turn;
pub mod event_service;
pub mod events;
pub mod idempotency;
mod mock;

#[cfg(test)]
mod scope_tests;

pub use active_turn::{
    ActiveTurnRecord, ActiveTurnStatus, ActiveTurnStore, ActiveTurnUpdate,
    PendingDynamicToolResult, PendingToolApproval, PendingUserAction,
};
pub use event_service::{SessionEventRetentionConfig, SessionEventService};
pub use events::{NewSessionEvent, SessionEventId, SessionEventRecord, SessionEventStore};
pub use idempotency::{
    IdempotencyRecord, IdempotencyReservation, IdempotencyStatus, MessageCommandAck,
    MessageIdempotencyStore,
};
pub use mock::MockSessionStore;

#[async_trait]
pub trait SessionStore: Send + Sync {
    async fn create_session(
        &self,
        context: &TalosContext,
        agent_id: &str,
        default_model: &str,
        requested_model: Option<&str>,
        conf: Option<SessionConf>,
        dm_scope: Option<String>,
        project_id: Option<String>,
        current_directory: Option<PathBuf>,
    ) -> TalosResult<SessionRecord>;

    async fn resolve_or_create_session(
        &self,
        context: &TalosContext,
        agent_id: &str,
        default_model: &str,
        requested_model: Option<&str>,
        dm_scope: Option<String>,
        project_id: Option<String>,
        current_directory: Option<PathBuf>,
    ) -> TalosResult<SessionRecord>;

    async fn get_session_record(&self, session_id: &str) -> TalosResult<Option<SessionRecord>>;

    async fn list_session_records(&self, query: &SessionQuery) -> TalosResult<Vec<SessionRecord>>;

    async fn count_session_records(&self, query: &SessionQuery) -> TalosResult<usize>;

    async fn patch_session_record(
        &self,
        session_id: &str,
        patch: &SessionPatch,
    ) -> TalosResult<SessionPatchAck>;

    async fn reset_session_record(&self, session_id: &str) -> TalosResult<SessionPatchAck>;

    async fn delete_session_record(&self, session_id: &str) -> TalosResult<SessionPatchAck>;

    async fn update_session_title(&self, session_id: &str, title: &str) -> TalosResult<()>;

    async fn record_session_progress(
        &self,
        session_id: &str,
        message_delta: u64,
        usage: &TalosUsage,
        model: Option<&str>,
    ) -> TalosResult<()>;

    async fn list_projects(&self) -> TalosResult<Vec<ProjectRecord>>;

    async fn get_project(&self, project_id: &str) -> TalosResult<Option<ProjectRecord>>;

    async fn create_project(&self, name: &str, directory: &PathBuf) -> TalosResult<ProjectRecord>;

    async fn create_project_for_owner(
        &self,
        name: &str,
        directory: &PathBuf,
        user_id: &str,
    ) -> TalosResult<ProjectRecord>;

    async fn patch_project(
        &self,
        project_id: &str,
        patch: &ProjectPatch,
    ) -> TalosResult<Option<ProjectRecord>>;

    async fn delete_project(&self, project_id: &str) -> TalosResult<()>;
}

pub struct DatabaseSessionStore {
    storage: Arc<dyn Storage>,
}

impl DatabaseSessionStore {
    pub async fn new(storage: Arc<dyn Storage>) -> TalosResult<Self> {
        // Initialize session-related tables if they don't exist
        storage.init_schema().await?;
        Ok(Self { storage })
    }

    pub(crate) fn stable_session_key(
        context: &TalosContext,
        agent_id: &str,
        dm_scope: &str,
    ) -> String {
        let thread = context
            .thread_id
            .clone()
            .unwrap_or_else(|| "_main".to_string());
        match dm_scope {
            "main" => format!(
                "agent:{}:provider:{}:user:{}",
                agent_id, context.provider, context.user_id
            ),
            "per_channel_peer" => format!(
                "agent:{}:provider:{}:channel:{}:user:{}",
                agent_id,
                context.provider,
                context.channel_id.as_deref().unwrap_or("_default"),
                context.user_id
            ),
            _ => format!(
                "agent:{}:provider:{}:channel:{}:user:{}:thread:{}",
                agent_id,
                context.provider,
                context.channel_id.as_deref().unwrap_or("_default"),
                context.user_id,
                thread
            ),
        }
    }

    pub(crate) fn resolve_dm_scope(dm_scope: Option<&str>) -> String {
        dm_scope.unwrap_or("per_channel_peer_topic").to_string()
    }

    pub(crate) fn new_session_id() -> String {
        uuid::Uuid::now_v7().to_string()
    }

    async fn fetch_session_row(&self, session_key: &str) -> TalosResult<Option<Row>> {
        let sql = "SELECT session_key, title, user_id, channel, thread_id, created_at, last_activity, context_json, model, message_count, usage_json, conf_json, project_id, current_directory FROM sessions WHERE session_key = ?";
        let rows = self.storage.query(sql, &[session_key]).await?;
        Ok(rows.into_iter().next())
    }

    fn parse_session_record(row: &Row) -> SessionRecord {
        let context = row
            .get_string("context_json")
            .and_then(|m| serde_json::from_str::<TalosContext>(&m).ok())
            .unwrap_or_else(|| {
                let context = TalosContext::system(row.get_string("user_id").unwrap_or_default());
                let context = match row.get_string("channel") {
                    Some(channel_id) => context.with_channel_id(channel_id),
                    None => context,
                };
                match row.get_string("thread_id") {
                    Some(thread_id) => context.with_thread_id(thread_id),
                    None => context,
                }
            });

        let model = row
            .get_string("model")
            .unwrap_or_else(|| "default".to_string());
        let message_count = row
            .get_i64("message_count")
            .unwrap_or_default()
            .try_into()
            .unwrap_or_default();
        let usage = row
            .get_string("usage_json")
            .and_then(|raw| serde_json::from_str::<TalosUsage>(&raw).ok())
            .unwrap_or_default();

        SessionRecord {
            id: row.get_string("session_key").unwrap_or_default(),
            title: row
                .get_string("title")
                .filter(|title| !title.trim().is_empty())
                .unwrap_or_else(|| DEFAULT_SESSION_TITLE.to_string()),
            user_id: row.get_string("user_id").unwrap_or_default(),
            context,
            model,
            created_at: row
                .get_i64("created_at")
                .unwrap_or_default()
                .try_into()
                .unwrap_or_default(),
            last_activity: row
                .get_i64("last_activity")
                .unwrap_or_default()
                .try_into()
                .unwrap_or_default(),
            message_count,
            usage,
            conf: Self::parse_conf_json(row.get_string("conf_json")),
            project_id: Self::normalized_optional_string(row.get_string("project_id")),
            current_directory: Self::normalized_optional_string(
                row.get_string("current_directory"),
            )
            .map(PathBuf::from),
        }
    }

    fn parse_conf_json(conf_json: Option<String>) -> SessionConf {
        conf_json
            .and_then(|v| serde_json::from_str::<SessionConf>(&v).ok())
            .unwrap_or_default()
    }

    pub(crate) fn normalized_optional_string(value: Option<String>) -> Option<String> {
        value.and_then(|raw| {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
    }

    fn build_conf_json(conf: &SessionConf) -> TalosResult<String> {
        serde_json::to_string(conf)
            .map_err(|e| talos_error::TalosError::SerializationError(e.to_string()))
    }

    fn build_context_json(context: &TalosContext) -> TalosResult<String> {
        serde_json::to_string(context)
            .map_err(|e| talos_error::TalosError::SerializationError(e.to_string()))
    }

    fn build_usage_json(usage: &TalosUsage) -> TalosResult<String> {
        serde_json::to_string(usage)
            .map_err(|e| talos_error::TalosError::SerializationError(e.to_string()))
    }

    fn parse_project_record(row: &Row) -> ProjectRecord {
        ProjectRecord {
            project_id: row.get_string("project_id").unwrap_or_default(),
            name: row.get_string("name").unwrap_or_default(),
            user_id: row.get_string("user_id").unwrap_or_default(),
            directory: row
                .get_string("directory")
                .map(PathBuf::from)
                .unwrap_or_default(),
            created_at: row.get_i64("created_at").unwrap_or_default(),
            updated_at: row.get_i64("updated_at").unwrap_or_default(),
            session_count: row.get_i64("session_count").unwrap_or_default(),
        }
    }

    async fn count_sessions_bound_to_project(&self, project_id: &str) -> TalosResult<i64> {
        let sql = "SELECT COUNT(*) AS count FROM sessions WHERE project_id = ?";
        let rows = self.storage.query(sql, &[project_id]).await?;
        Ok(rows
            .into_iter()
            .next()
            .and_then(|row| row.get_i64("count"))
            .unwrap_or_default())
    }
}

#[async_trait]
impl SessionStore for DatabaseSessionStore {
    async fn create_session(
        &self,
        context: &TalosContext,
        agent_id: &str,
        default_model: &str,
        requested_model: Option<&str>,
        conf: Option<SessionConf>,
        _dm_scope: Option<String>,
        project_id: Option<String>,
        current_directory: Option<PathBuf>,
    ) -> TalosResult<SessionRecord> {
        let session_id = Self::new_session_id();
        let now = chrono::Utc::now().timestamp_millis();
        let model = requested_model.unwrap_or(default_model).to_string();
        let base_conf = if requested_model.is_some() {
            SessionConfigOverridePolicy::single_model_conf(&model)
        } else {
            SessionConf::default()
        };
        let effective_conf = match conf.as_ref() {
            Some(patch_conf) => SessionConfigOverridePolicy::merge_conf(&base_conf, patch_conf),
            None => base_conf,
        };

        let record = SessionRecord {
            id: session_id.clone(),
            title: DEFAULT_SESSION_TITLE.to_string(),
            user_id: context.user_id.clone(),
            context: context.clone(),
            model: model.clone(),
            created_at: now.try_into().unwrap_or_default(),
            last_activity: now.try_into().unwrap_or_default(),
            message_count: 0,
            usage: TalosUsage::default(),
            conf: effective_conf.clone(),
            project_id: project_id.clone(),
            current_directory: current_directory.clone(),
        };

        let context_json = Self::build_context_json(context)?;
        let usage_json = Self::build_usage_json(&record.usage)?;
        let conf_json = Self::build_conf_json(&effective_conf)?;

        let row = Row::new(
            vec![
                "session_key".to_string(),
                "user_id".to_string(),
                "channel".to_string(),
                "scope".to_string(),
                "peer_id".to_string(),
                "agent_id".to_string(),
                "account_id".to_string(),
                "thread_id".to_string(),
                "created_at".to_string(),
                "last_activity".to_string(),
                "title".to_string(),
                "context_json".to_string(),
                "model".to_string(),
                "message_count".to_string(),
                "usage_json".to_string(),
                "conf_json".to_string(),
                "project_id".to_string(),
                "current_directory".to_string(),
            ],
            vec![
                Value::String(session_id),
                Value::String(context.user_id.clone()),
                context
                    .channel_id
                    .clone()
                    .map(Value::String)
                    .unwrap_or(Value::Null),
                Value::String("api".to_string()),
                Value::String(context.user_id.clone()),
                Value::String(agent_id.to_string()),
                Value::Null,
                context
                    .thread_id
                    .clone()
                    .map(Value::String)
                    .unwrap_or(Value::Null),
                Value::Number(now.into()),
                Value::Number(now.into()),
                Value::String(record.title.clone()),
                Value::String(context_json),
                Value::String(model),
                Value::Number(0.into()),
                Value::String(usage_json),
                Value::String(conf_json),
                project_id.map(Value::String).unwrap_or(Value::Null),
                current_directory
                    .map(|path| Value::String(path.display().to_string()))
                    .unwrap_or(Value::Null),
            ],
        );
        self.storage.insert("sessions", &row).await?;

        Ok(record)
    }

    async fn resolve_or_create_session(
        &self,
        context: &TalosContext,
        agent_id: &str,
        default_model: &str,
        requested_model: Option<&str>,
        dm_scope: Option<String>,
        project_id: Option<String>,
        current_directory: Option<PathBuf>,
    ) -> TalosResult<SessionRecord> {
        let dm_scope = Self::resolve_dm_scope(dm_scope.as_deref());
        let session_key = Self::stable_session_key(context, agent_id, &dm_scope);
        let now = chrono::Utc::now().timestamp_millis();

        if let Some(existing) = self.fetch_session_row(&session_key).await? {
            let mut record = Self::parse_session_record(&existing);
            let mut conf = Self::parse_conf_json(existing.get_string("conf_json"));
            if let Some(model) = requested_model {
                record.model = model.to_string();
                conf = SessionConfigOverridePolicy::merge_conf(
                    &conf,
                    &SessionConfigOverridePolicy::single_model_conf(model),
                );
            }
            record.conf = conf.clone();
            record.last_activity = now.try_into().unwrap_or_default();

            let context_json = Self::build_context_json(&record.context)?;
            let conf_json = Self::build_conf_json(&conf)?;

            let update_row = Row::new(
                vec![
                    "last_activity".to_string(),
                    "title".to_string(),
                    "context_json".to_string(),
                    "model".to_string(),
                    "message_count".to_string(),
                    "usage_json".to_string(),
                    "conf_json".to_string(),
                    "project_id".to_string(),
                    "current_directory".to_string(),
                ],
                vec![
                    Value::Number(now.into()),
                    Value::String(record.title.clone()),
                    Value::String(context_json),
                    Value::String(record.model.clone()),
                    Value::Number(serde_json::Number::from(record.message_count)),
                    Value::String(Self::build_usage_json(&record.usage)?),
                    Value::String(conf_json),
                    record
                        .project_id
                        .clone()
                        .map(Value::String)
                        .unwrap_or(Value::Null),
                    record
                        .current_directory
                        .clone()
                        .map(|path| Value::String(path.display().to_string()))
                        .unwrap_or(Value::Null),
                ],
            );
            self.storage
                .update(
                    "sessions",
                    &update_row,
                    "session_key = ?",
                    &[session_key.as_str()],
                )
                .await?;

            return Ok(record);
        }

        let model = requested_model.unwrap_or(default_model).to_string();
        let conf = if requested_model.is_some() {
            SessionConfigOverridePolicy::single_model_conf(&model)
        } else {
            SessionConf::default()
        };
        let record = SessionRecord {
            id: session_key.clone(),
            title: DEFAULT_SESSION_TITLE.to_string(),
            user_id: context.user_id.clone(),
            context: context.clone(),
            model: model.clone(),
            created_at: now.try_into().unwrap_or_default(),
            last_activity: now.try_into().unwrap_or_default(),
            message_count: 0,
            usage: TalosUsage::default(),
            conf: conf.clone(),
            project_id: project_id.clone(),
            current_directory: current_directory.clone(),
        };

        let context_json = Self::build_context_json(context)?;
        let usage_json = Self::build_usage_json(&record.usage)?;
        let conf_json = Self::build_conf_json(&conf)?;

        let row = Row::new(
            vec![
                "session_key".to_string(),
                "user_id".to_string(),
                "channel".to_string(),
                "scope".to_string(),
                "peer_id".to_string(),
                "agent_id".to_string(),
                "account_id".to_string(),
                "thread_id".to_string(),
                "created_at".to_string(),
                "last_activity".to_string(),
                "title".to_string(),
                "context_json".to_string(),
                "model".to_string(),
                "message_count".to_string(),
                "usage_json".to_string(),
                "conf_json".to_string(),
                "project_id".to_string(),
                "current_directory".to_string(),
            ],
            vec![
                Value::String(session_key),
                Value::String(context.user_id.clone()),
                context
                    .channel_id
                    .clone()
                    .map(Value::String)
                    .unwrap_or(Value::Null),
                Value::String(dm_scope),
                Value::String(context.user_id.clone()),
                Value::String(agent_id.to_string()),
                Value::Null,
                context
                    .thread_id
                    .clone()
                    .map(Value::String)
                    .unwrap_or(Value::Null),
                Value::Number(now.into()),
                Value::Number(now.into()),
                Value::String(record.title.clone()),
                Value::String(context_json),
                Value::String(model),
                Value::Number(0.into()),
                Value::String(usage_json),
                Value::String(conf_json),
                project_id.map(Value::String).unwrap_or(Value::Null),
                current_directory
                    .map(|path| Value::String(path.display().to_string()))
                    .unwrap_or(Value::Null),
            ],
        );
        self.storage.insert("sessions", &row).await?;
        Ok(record)
    }

    async fn get_session_record(&self, session_id: &str) -> TalosResult<Option<SessionRecord>> {
        let row = self.fetch_session_row(session_id).await?;
        Ok(row.as_ref().map(Self::parse_session_record))
    }

    async fn list_session_records(&self, query: &SessionQuery) -> TalosResult<Vec<SessionRecord>> {
        let mut sql = "SELECT session_key, title, user_id, channel, thread_id, created_at, last_activity, context_json, model, message_count, usage_json, conf_json, project_id, current_directory FROM sessions".to_string();
        let mut clauses = Vec::new();
        let mut params = Vec::new();
        if let Some(user_id) = query.user_id.as_deref() {
            clauses.push("user_id = ?");
            params.push(user_id.to_string());
        }
        if let Some(channel_id) = query.channel_id.as_deref() {
            clauses.push("channel = ?");
            params.push(channel_id.to_string());
        }
        if let Some(project_id) = query.project_id.as_deref() {
            clauses.push("project_id = ?");
            params.push(project_id.to_string());
        }
        if !clauses.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&clauses.join(" AND "));
        }
        sql.push_str(" ORDER BY last_activity DESC");
        if let Some(limit) = query.limit {
            sql.push_str(" LIMIT ?");
            params.push(limit.to_string());
        } else if query.offset.is_some() {
            sql.push_str(" LIMIT -1");
        }
        if let Some(offset) = query.offset {
            sql.push_str(" OFFSET ?");
            params.push(offset.to_string());
        }

        let params_ref = params.iter().map(String::as_str).collect::<Vec<_>>();
        let rows = self.storage.query(&sql, &params_ref).await?;
        let mut records = Vec::new();
        for row in rows {
            let record = Self::parse_session_record(&row);
            if query
                .provider
                .as_ref()
                .is_some_and(|v| record.context.provider.as_str() != v)
            {
                continue;
            }
            records.push(record);
        }
        Ok(records)
    }

    async fn count_session_records(&self, query: &SessionQuery) -> TalosResult<usize> {
        if query.provider.is_some() {
            let mut count_query = query.clone();
            count_query.limit = None;
            count_query.offset = None;
            return Ok(self.list_session_records(&count_query).await?.len());
        }

        let mut sql = "SELECT COUNT(*) as total FROM sessions".to_string();
        let mut clauses = Vec::new();
        let mut params = Vec::new();
        if let Some(user_id) = query.user_id.as_deref() {
            clauses.push("user_id = ?");
            params.push(user_id.to_string());
        }
        if let Some(channel_id) = query.channel_id.as_deref() {
            clauses.push("channel = ?");
            params.push(channel_id.to_string());
        }
        if let Some(project_id) = query.project_id.as_deref() {
            clauses.push("project_id = ?");
            params.push(project_id.to_string());
        }
        if !clauses.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&clauses.join(" AND "));
        }

        let params_ref = params.iter().map(String::as_str).collect::<Vec<_>>();
        let rows = self.storage.query(&sql, &params_ref).await?;
        let Some(row) = rows.first() else {
            return Ok(0);
        };
        Ok(row.get_i64("total").unwrap_or_default().max(0) as usize)
    }

    async fn patch_session_record(
        &self,
        session_id: &str,
        patch: &SessionPatch,
    ) -> TalosResult<SessionPatchAck> {
        if let Some(conf) = patch.conf.as_ref() {
            SessionConfigOverridePolicy::validate_conf(conf)?;
        }
        let Some(row) = self.fetch_session_row(session_id).await? else {
            return Ok(SessionPatchAck { success: false });
        };
        let mut record = Self::parse_session_record(&row);
        let mut conf = Self::parse_conf_json(row.get_string("conf_json"));

        let patch_conf = patch.conf.as_ref().cloned().unwrap_or_default();
        conf = SessionConfigOverridePolicy::merge_conf(&conf, &patch_conf);
        if let Some(model) = SessionConfigOverridePolicy::extract_agent_model(&conf)? {
            record.model = model.clone();
        }
        if let Some(project_id_patch) = &patch.project_id {
            match project_id_patch {
                OptionalProjectIdPatch::Set { project_id } => {
                    record.project_id = Some(project_id.clone());
                }
                OptionalProjectIdPatch::Clear => {
                    record.project_id = None;
                }
            }
        }
        if let Some(current_directory_patch) = &patch.current_directory {
            match current_directory_patch {
                OptionalCurrentDirectoryPatch::Set { path } => {
                    record.current_directory = Some(path.clone());
                }
                OptionalCurrentDirectoryPatch::Clear => {
                    record.current_directory = None;
                }
            }
        }
        record.last_activity = chrono::Utc::now()
            .timestamp_millis()
            .try_into()
            .unwrap_or_default();
        let context_json = Self::build_context_json(&record.context)?;
        let conf_json = Self::build_conf_json(&conf)?;

        let update_row = Row::new(
            vec![
                "last_activity".to_string(),
                "title".to_string(),
                "context_json".to_string(),
                "model".to_string(),
                "message_count".to_string(),
                "usage_json".to_string(),
                "conf_json".to_string(),
                "project_id".to_string(),
                "current_directory".to_string(),
            ],
            vec![
                Value::Number(serde_json::Number::from(record.last_activity)),
                Value::String(record.title.clone()),
                Value::String(context_json),
                Value::String(record.model.clone()),
                Value::Number(serde_json::Number::from(record.message_count)),
                Value::String(Self::build_usage_json(&record.usage)?),
                Value::String(conf_json),
                record
                    .project_id
                    .clone()
                    .map(Value::String)
                    .unwrap_or(Value::Null),
                record
                    .current_directory
                    .clone()
                    .map(|path| Value::String(path.display().to_string()))
                    .unwrap_or(Value::Null),
            ],
        );
        self.storage
            .update("sessions", &update_row, "session_key = ?", &[session_id])
            .await?;
        Ok(SessionPatchAck { success: true })
    }

    async fn update_session_title(&self, session_id: &str, title: &str) -> TalosResult<()> {
        let normalized = title.trim();
        if normalized.is_empty() {
            return Err(talos_error::TalosError::ValidationError(
                "session title must not be empty".to_string(),
            ));
        }

        let update_row = Row::new(
            vec!["title".to_string()],
            vec![Value::String(normalized.to_string())],
        );
        let affected = self
            .storage
            .update("sessions", &update_row, "session_key = ?", &[session_id])
            .await?;
        if affected == 0 {
            return Err(talos_error::TalosError::ResourceNotFound(format!(
                "session {}",
                session_id
            )));
        }
        Ok(())
    }

    async fn reset_session_record(&self, session_id: &str) -> TalosResult<SessionPatchAck> {
        let now = chrono::Utc::now().timestamp_millis();
        let Some(mut record) = self.get_session_record(session_id).await? else {
            return Ok(SessionPatchAck { success: false });
        };

        record.title = DEFAULT_SESSION_TITLE.to_string();
        record.last_activity = now.try_into().unwrap_or_default();
        record.message_count = 0;
        record.usage = TalosUsage::default();

        let row = self.fetch_session_row(session_id).await?;
        let conf = Self::parse_conf_json(row.and_then(|stored| stored.get_string("conf_json")));
        let conf_json = Self::build_conf_json(&conf)?;
        let context_json = Self::build_context_json(&record.context)?;
        let update_row = Row::new(
            vec![
                "last_activity".to_string(),
                "title".to_string(),
                "context_json".to_string(),
                "model".to_string(),
                "message_count".to_string(),
                "usage_json".to_string(),
                "conf_json".to_string(),
                "project_id".to_string(),
                "current_directory".to_string(),
            ],
            vec![
                Value::Number(now.into()),
                Value::String(record.title.clone()),
                Value::String(context_json),
                Value::String(record.model.clone()),
                Value::Number(0.into()),
                Value::String(Self::build_usage_json(&record.usage)?),
                Value::String(conf_json),
                record
                    .project_id
                    .clone()
                    .map(Value::String)
                    .unwrap_or(Value::Null),
                record
                    .current_directory
                    .clone()
                    .map(|path| Value::String(path.display().to_string()))
                    .unwrap_or(Value::Null),
            ],
        );
        let affected = self
            .storage
            .update("sessions", &update_row, "session_key = ?", &[session_id])
            .await?;

        Ok(SessionPatchAck {
            success: affected > 0,
        })
    }

    async fn delete_session_record(&self, session_id: &str) -> TalosResult<SessionPatchAck> {
        self.storage
            .delete("session_events", "session_id = ?", &[session_id])
            .await?;
        self.storage
            .delete("active_turns", "session_id = ?", &[session_id])
            .await?;
        self.storage
            .delete("message_idempotency_keys", "session_id = ?", &[session_id])
            .await?;
        let affected = self
            .storage
            .delete("sessions", "session_key = ?", &[session_id])
            .await?;
        Ok(SessionPatchAck {
            success: affected > 0,
        })
    }

    async fn record_session_progress(
        &self,
        session_id: &str,
        message_delta: u64,
        usage: &TalosUsage,
        model: Option<&str>,
    ) -> TalosResult<()> {
        let now = chrono::Utc::now().timestamp_millis();
        let Some(mut record) = self.get_session_record(session_id).await? else {
            return Err(talos_error::TalosError::ResourceNotFound(format!(
                "session {}",
                session_id
            )));
        };

        record.last_activity = now.try_into().unwrap_or_default();
        record.message_count = record.message_count.saturating_add(message_delta);
        record.usage.accumulate(usage);
        if let Some(model) = model {
            record.model = model.to_string();
        }

        let row = self.fetch_session_row(session_id).await?;
        let conf = Self::parse_conf_json(row.and_then(|stored| stored.get_string("conf_json")));
        let conf_json = Self::build_conf_json(&conf)?;
        let context_json = Self::build_context_json(&record.context)?;

        let update_row = Row::new(
            vec![
                "last_activity".to_string(),
                "title".to_string(),
                "context_json".to_string(),
                "model".to_string(),
                "message_count".to_string(),
                "usage_json".to_string(),
                "conf_json".to_string(),
                "project_id".to_string(),
                "current_directory".to_string(),
            ],
            vec![
                Value::Number(now.into()),
                Value::String(record.title.clone()),
                Value::String(context_json),
                Value::String(record.model.clone()),
                Value::Number(serde_json::Number::from(record.message_count)),
                Value::String(Self::build_usage_json(&record.usage)?),
                Value::String(conf_json),
                record
                    .project_id
                    .clone()
                    .map(Value::String)
                    .unwrap_or(Value::Null),
                record
                    .current_directory
                    .clone()
                    .map(|path| Value::String(path.display().to_string()))
                    .unwrap_or(Value::Null),
            ],
        );
        self.storage
            .update("sessions", &update_row, "session_key = ?", &[session_id])
            .await?;

        Ok(())
    }

    async fn list_projects(&self) -> TalosResult<Vec<ProjectRecord>> {
        let sql = "SELECT p.project_id, p.name, p.user_id, p.directory, p.created_at, p.updated_at, COUNT(s.session_key) AS session_count FROM projects p LEFT JOIN sessions s ON s.project_id = p.project_id GROUP BY p.project_id, p.name, p.user_id, p.directory, p.created_at, p.updated_at ORDER BY p.created_at DESC";
        let rows = self.storage.query(sql, &[]).await?;
        Ok(rows.iter().map(Self::parse_project_record).collect())
    }

    async fn get_project(&self, project_id: &str) -> TalosResult<Option<ProjectRecord>> {
        let sql =
            "SELECT p.project_id, p.name, p.user_id, p.directory, p.created_at, p.updated_at, COUNT(s.session_key) AS session_count FROM projects p LEFT JOIN sessions s ON s.project_id = p.project_id WHERE p.project_id = ? GROUP BY p.project_id, p.name, p.user_id, p.directory, p.created_at, p.updated_at";
        let rows = self.storage.query(sql, &[project_id]).await?;
        Ok(rows
            .into_iter()
            .next()
            .map(|row| Self::parse_project_record(&row)))
    }

    async fn create_project(&self, name: &str, directory: &PathBuf) -> TalosResult<ProjectRecord> {
        self.create_project_for_owner(name, directory, "").await
    }

    async fn create_project_for_owner(
        &self,
        name: &str,
        directory: &PathBuf,
        user_id: &str,
    ) -> TalosResult<ProjectRecord> {
        let now = chrono::Utc::now().timestamp_millis();
        let record = ProjectRecord {
            project_id: uuid::Uuid::now_v7().to_string(),
            name: name.to_string(),
            user_id: user_id.to_string(),
            directory: directory.clone(),
            created_at: now,
            updated_at: now,
            session_count: 0,
        };
        let row = Row::new(
            vec![
                "project_id".to_string(),
                "name".to_string(),
                "user_id".to_string(),
                "directory".to_string(),
                "created_at".to_string(),
                "updated_at".to_string(),
            ],
            vec![
                Value::String(record.project_id.clone()),
                Value::String(record.name.clone()),
                Value::String(record.user_id.clone()),
                Value::String(record.directory.display().to_string()),
                Value::Number(now.into()),
                Value::Number(now.into()),
            ],
        );
        self.storage.insert("projects", &row).await?;
        Ok(record)
    }

    async fn patch_project(
        &self,
        project_id: &str,
        patch: &ProjectPatch,
    ) -> TalosResult<Option<ProjectRecord>> {
        let Some(mut record) = self.get_project(project_id).await? else {
            return Ok(None);
        };
        if let Some(name) = patch.name.as_ref() {
            record.name = name.clone();
        }
        record.updated_at = chrono::Utc::now().timestamp_millis();
        let row = Row::new(
            vec!["name".to_string(), "updated_at".to_string()],
            vec![
                Value::String(record.name.clone()),
                Value::Number(record.updated_at.into()),
            ],
        );
        self.storage
            .update("projects", &row, "project_id = ?", &[project_id])
            .await?;
        Ok(Some(record))
    }

    async fn delete_project(&self, project_id: &str) -> TalosResult<()> {
        let bound_count = self.count_sessions_bound_to_project(project_id).await?;
        if bound_count > 0 {
            return Err(talos_error::TalosError::ValidationError(format!(
                "project {} still has bound sessions",
                project_id
            )));
        }
        self.storage
            .execute("DELETE FROM projects WHERE project_id = ?", &[project_id])
            .await?;
        Ok(())
    }
}

// Mock implementation for testing
