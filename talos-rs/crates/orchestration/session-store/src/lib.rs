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

    fn stable_session_key(context: &TalosContext, agent_id: &str, dm_scope: &str) -> String {
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

    fn resolve_dm_scope(dm_scope: Option<&str>) -> String {
        dm_scope.unwrap_or("per_channel_peer_topic").to_string()
    }

    fn new_session_id() -> String {
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

    fn normalized_optional_string(value: Option<String>) -> Option<String> {
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
pub struct MockSessionStore;

impl MockSessionStore {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl SessionStore for MockSessionStore {
    async fn create_session(
        &self,
        context: &TalosContext,
        _agent_id: &str,
        default_model: &str,
        requested_model: Option<&str>,
        conf: Option<SessionConf>,
        _dm_scope: Option<String>,
        project_id: Option<String>,
        current_directory: Option<PathBuf>,
    ) -> TalosResult<SessionRecord> {
        let base_conf = requested_model
            .map(SessionConfigOverridePolicy::single_model_conf)
            .unwrap_or_default();
        let effective_conf = match conf.as_ref() {
            Some(patch_conf) => SessionConfigOverridePolicy::merge_conf(&base_conf, patch_conf),
            None => base_conf,
        };
        Ok(SessionRecord {
            id: DatabaseSessionStore::new_session_id(),
            title: DEFAULT_SESSION_TITLE.to_string(),
            user_id: context.user_id.clone(),
            context: context.clone(),
            model: requested_model.unwrap_or(default_model).to_string(),
            created_at: 0,
            last_activity: 0,
            message_count: 0,
            usage: TalosUsage::default(),
            conf: effective_conf,
            project_id,
            current_directory,
        })
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
        let dm_scope = DatabaseSessionStore::resolve_dm_scope(dm_scope.as_deref());
        Ok(SessionRecord {
            id: DatabaseSessionStore::stable_session_key(context, agent_id, &dm_scope),
            title: DEFAULT_SESSION_TITLE.to_string(),
            user_id: context.user_id.clone(),
            context: context.clone(),
            model: requested_model.unwrap_or(default_model).to_string(),
            created_at: 0,
            last_activity: 0,
            message_count: 0,
            usage: TalosUsage::default(),
            conf: requested_model
                .map(SessionConfigOverridePolicy::single_model_conf)
                .unwrap_or_default(),
            project_id,
            current_directory,
        })
    }

    async fn get_session_record(&self, session_id: &str) -> TalosResult<Option<SessionRecord>> {
        Ok(Some(SessionRecord {
            id: session_id.to_string(),
            title: DEFAULT_SESSION_TITLE.to_string(),
            user_id: "mock-user".to_string(),
            context: TalosContext {
                source_type: TalosSourceType::System,
                provider: TalosProvider::System,
                user_id: "mock-user".to_string(),
                message_id: "message-mock".to_string(),
                trace_id: "trace-mock".to_string(),
                channel_id: Some("default".to_string()),
                thread_id: None,
                project_root: None,
                environment: None,
            },
            model: "default".to_string(),
            created_at: 0,
            last_activity: 0,
            message_count: 0,
            usage: TalosUsage::default(),
            conf: SessionConf::default(),
            project_id: None,
            current_directory: None,
        }))
    }

    async fn list_session_records(&self, _query: &SessionQuery) -> TalosResult<Vec<SessionRecord>> {
        Ok(Vec::new())
    }

    async fn count_session_records(&self, _query: &SessionQuery) -> TalosResult<usize> {
        Ok(0)
    }

    async fn patch_session_record(
        &self,
        session_id: &str,
        patch: &SessionPatch,
    ) -> TalosResult<SessionPatchAck> {
        let _ = session_id;
        let _ = patch.agent_model()?;
        Ok(SessionPatchAck { success: true })
    }

    async fn reset_session_record(&self, _session_id: &str) -> TalosResult<SessionPatchAck> {
        Ok(SessionPatchAck { success: true })
    }

    async fn delete_session_record(&self, _session_id: &str) -> TalosResult<SessionPatchAck> {
        Ok(SessionPatchAck { success: true })
    }

    async fn update_session_title(&self, _session_id: &str, _title: &str) -> TalosResult<()> {
        Ok(())
    }

    async fn record_session_progress(
        &self,
        _session_id: &str,
        _message_delta: u64,
        _usage: &TalosUsage,
        _model: Option<&str>,
    ) -> TalosResult<()> {
        Ok(())
    }

    async fn list_projects(&self) -> TalosResult<Vec<ProjectRecord>> {
        Ok(Vec::new())
    }

    async fn get_project(&self, _project_id: &str) -> TalosResult<Option<ProjectRecord>> {
        Ok(None)
    }

    async fn create_project(&self, name: &str, directory: &PathBuf) -> TalosResult<ProjectRecord> {
        Ok(ProjectRecord {
            project_id: "project-mock".to_string(),
            name: name.to_string(),
            user_id: String::new(),
            directory: directory.clone(),
            created_at: 0,
            updated_at: 0,
            session_count: 0,
        })
    }

    async fn create_project_for_owner(
        &self,
        name: &str,
        directory: &PathBuf,
        user_id: &str,
    ) -> TalosResult<ProjectRecord> {
        Ok(ProjectRecord {
            project_id: "project-mock".to_string(),
            name: name.to_string(),
            user_id: user_id.to_string(),
            directory: directory.clone(),
            created_at: 0,
            updated_at: 0,
            session_count: 0,
        })
    }

    async fn patch_project(
        &self,
        _project_id: &str,
        _patch: &ProjectPatch,
    ) -> TalosResult<Option<ProjectRecord>> {
        Ok(None)
    }

    async fn delete_project(&self, _project_id: &str) -> TalosResult<()> {
        Ok(())
    }
}

#[cfg(test)]
mod scope_tests {
    use super::*;
    use std::path::Path;
    use std::sync::Arc;
    use talos_storage::SqliteStorage;

    fn sample_context() -> TalosContext {
        TalosContext {
            source_type: TalosSourceType::Im,
            provider: TalosProvider::Telegram,
            user_id: "user-1".to_string(),
            message_id: "message-1".to_string(),
            trace_id: "trace-1".to_string(),
            channel_id: Some("group-1".to_string()),
            thread_id: Some("topic-1".to_string()),
            project_root: None,
            environment: None,
        }
    }

    #[test]
    fn test_stable_session_key_per_channel_peer_topic() {
        let key = DatabaseSessionStore::stable_session_key(
            &sample_context(),
            "agent-a",
            "per_channel_peer_topic",
        );
        assert!(key.contains("channel:group-1"));
        assert!(key.contains("thread:topic-1"));
    }

    #[test]
    fn test_stable_session_key_per_channel_peer() {
        let key = DatabaseSessionStore::stable_session_key(
            &sample_context(),
            "agent-a",
            "per_channel_peer",
        );
        assert!(key.contains("channel:group-1"));
        assert!(!key.contains("thread:"));
    }

    #[test]
    fn test_stable_session_key_main() {
        let key = DatabaseSessionStore::stable_session_key(&sample_context(), "agent-a", "main");
        assert!(!key.contains("channel:"));
        assert!(!key.contains("thread:"));
        assert!(key.contains("user:user-1"));
    }

    #[test]
    fn test_new_session_id_is_uuid_v7() {
        let session_id = DatabaseSessionStore::new_session_id();
        let parsed = uuid::Uuid::parse_str(&session_id).expect("must be valid uuid");
        assert_eq!(parsed.get_version_num(), 7);
    }

    #[test]
    fn test_new_session_id_is_unique() {
        let id1 = DatabaseSessionStore::new_session_id();
        let id2 = DatabaseSessionStore::new_session_id();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_normalized_optional_string_treats_blank_as_none() {
        assert_eq!(DatabaseSessionStore::normalized_optional_string(None), None);
        assert_eq!(
            DatabaseSessionStore::normalized_optional_string(Some(String::new())),
            None
        );
        assert_eq!(
            DatabaseSessionStore::normalized_optional_string(Some("   ".to_string())),
            None
        );
        assert_eq!(
            DatabaseSessionStore::normalized_optional_string(Some(" project-1 ".to_string())),
            Some("project-1".to_string())
        );
    }

    #[tokio::test]
    async fn test_record_session_progress_returns_not_found_for_missing_session() {
        let storage = Arc::new(SqliteStorage::new("sqlite::memory:").await.unwrap());
        let store = DatabaseSessionStore::new(storage).await.unwrap();

        let result = store
            .record_session_progress("missing-session", 1, &TalosUsage::default(), None)
            .await;

        assert!(matches!(
            result,
            Err(talos_error::TalosError::ResourceNotFound(message))
            if message == "session missing-session"
        ));
    }

    #[tokio::test]
    async fn test_create_session_sets_default_title() {
        let storage = Arc::new(SqliteStorage::new("sqlite::memory:").await.unwrap());
        let store = DatabaseSessionStore::new(storage).await.unwrap();

        let session = store
            .create_session(
                &sample_context(),
                "agent-a",
                "qwen-max",
                None,
                None,
                None,
                None,
                None,
            )
            .await
            .expect("create session should succeed");

        assert_eq!(session.title, DEFAULT_SESSION_TITLE);

        let loaded = store
            .get_session_record(&session.id)
            .await
            .expect("load session should succeed")
            .expect("session should exist");
        assert_eq!(loaded.title, DEFAULT_SESSION_TITLE);
    }

    #[tokio::test]
    async fn test_reset_session_record_clears_title_message_count_and_usage() {
        let storage = Arc::new(SqliteStorage::new("sqlite::memory:").await.unwrap());
        let store = DatabaseSessionStore::new(storage).await.unwrap();

        let session = store
            .create_session(
                &sample_context(),
                "agent-a",
                "qwen-max",
                Some("gpt-5.3"),
                None,
                None,
                None,
                None,
            )
            .await
            .expect("create session should succeed");
        store
            .update_session_title(&session.id, "Custom Title")
            .await
            .expect("title update should succeed");
        store
            .record_session_progress(
                &session.id,
                4,
                &TalosUsage {
                    input_tokens: 11,
                    output_tokens: 7,
                    total_tokens: 18,
                    cache_creation_input_tokens: Some(3),
                    cache_read_input_tokens: Some(5),
                },
                Some("gpt-5.4"),
            )
            .await
            .expect("progress update should succeed");

        let ack = store
            .reset_session_record(&session.id)
            .await
            .expect("reset should succeed");
        assert!(ack.success);

        let loaded = store
            .get_session_record(&session.id)
            .await
            .expect("load session should succeed")
            .expect("session should exist");
        assert_eq!(loaded.title, DEFAULT_SESSION_TITLE);
        assert_eq!(loaded.message_count, 0);
        assert_eq!(loaded.usage, TalosUsage::default());
        assert_eq!(loaded.model, "gpt-5.4");
        assert_eq!(
            loaded.conf,
            SessionConfigOverridePolicy::single_model_conf("gpt-5.3")
        );
    }

    #[tokio::test]
    async fn test_delete_session_record_removes_session() {
        let storage = Arc::new(SqliteStorage::new("sqlite::memory:").await.unwrap());
        let store = DatabaseSessionStore::new(storage).await.unwrap();

        let session = store
            .create_session(
                &sample_context(),
                "agent-a",
                "qwen-max",
                None,
                None,
                None,
                None,
                None,
            )
            .await
            .expect("create session should succeed");

        let ack = store
            .delete_session_record(&session.id)
            .await
            .expect("delete should succeed");
        assert!(ack.success);

        let loaded = store
            .get_session_record(&session.id)
            .await
            .expect("load session should succeed");
        assert!(loaded.is_none());
    }

    #[tokio::test]
    async fn test_patch_session_record_merges_tool_approval_policy() {
        let storage = Arc::new(SqliteStorage::new("sqlite::memory:").await.unwrap());
        let store = DatabaseSessionStore::new(storage).await.unwrap();

        let session = store
            .create_session(
                &sample_context(),
                "agent-a",
                "qwen-max",
                Some("gpt-5.3"),
                None,
                None,
                None,
                None,
            )
            .await
            .expect("create session should succeed");

        let ack = store
            .patch_session_record(
                &session.id,
                &SessionPatch {
                    conf: Some(SessionConf {
                        agent: None,
                        tools: Some(talos_types::SessionToolsConf {
                            approval_policy: Some(talos_types::ToolApprovalPolicy::Always),
                        }),
                    }),
                    project_id: None,
                    current_directory: None,
                },
            )
            .await
            .expect("patch session should succeed");
        assert!(ack.success);

        let loaded = store
            .get_session_record(&session.id)
            .await
            .expect("load session should succeed")
            .expect("session should exist");

        assert_eq!(
            SessionConfigOverridePolicy::extract_agent_model(&loaded.conf).unwrap(),
            Some("gpt-5.3".to_string())
        );
        assert_eq!(
            SessionConfigOverridePolicy::extract_tool_approval_policy(&loaded.conf).unwrap(),
            Some(talos_types::ToolApprovalPolicy::Always)
        );
    }

    #[tokio::test]
    async fn test_resolve_or_create_session_preserves_existing_project_binding_fields() {
        let storage = Arc::new(SqliteStorage::new("sqlite::memory:").await.unwrap());
        let store = DatabaseSessionStore::new(storage).await.unwrap();

        let initial = store
            .resolve_or_create_session(
                &sample_context(),
                "agent-a",
                "qwen-max",
                None,
                Some("per_channel_peer_topic".to_string()),
                Some("project-1".to_string()),
                Some(PathBuf::from("/repo/one")),
            )
            .await
            .expect("initial resolve_or_create should succeed");

        let resolved = store
            .resolve_or_create_session(
                &sample_context(),
                "agent-a",
                "qwen-max",
                None,
                Some("per_channel_peer_topic".to_string()),
                Some("project-2".to_string()),
                Some(PathBuf::from("/repo/two")),
            )
            .await
            .expect("second resolve_or_create should succeed");

        assert_eq!(resolved.id, initial.id);
        assert_eq!(resolved.project_id.as_deref(), Some("project-1"));
        assert_eq!(
            resolved.current_directory.as_deref(),
            Some(Path::new("/repo/one"))
        );

        let persisted = store
            .get_session_record(&initial.id)
            .await
            .expect("load session should succeed")
            .expect("session should exist");
        assert_eq!(persisted.project_id.as_deref(), Some("project-1"));
        assert_eq!(
            persisted.current_directory.as_deref(),
            Some(Path::new("/repo/one"))
        );
    }

    #[tokio::test]
    async fn test_list_session_records_filters_by_project_id() {
        let storage = Arc::new(SqliteStorage::new("sqlite::memory:").await.unwrap());
        let store = DatabaseSessionStore::new(storage).await.unwrap();

        let first = store
            .create_session(
                &sample_context(),
                "agent-a",
                "qwen-max",
                None,
                None,
                None,
                Some("project-1".to_string()),
                None,
            )
            .await
            .expect("create first session should succeed");
        let second = store
            .create_session(
                &sample_context(),
                "agent-a",
                "qwen-max",
                None,
                None,
                None,
                Some("project-2".to_string()),
                None,
            )
            .await
            .expect("create second session should succeed");

        let records = store
            .list_session_records(&SessionQuery {
                user_id: None,
                channel_id: None,
                provider: None,
                project_id: Some("project-1".to_string()),
                limit: None,
                offset: None,
            })
            .await
            .expect("list sessions should succeed");

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, first.id);
        assert_ne!(records[0].id, second.id);
    }

    #[tokio::test]
    async fn test_list_session_records_respects_limit_and_offset() {
        let storage = Arc::new(SqliteStorage::new("sqlite::memory:").await.unwrap());
        let store = DatabaseSessionStore::new(storage).await.unwrap();

        let first = store
            .create_session(
                &sample_context(),
                "agent-a",
                "qwen-max",
                None,
                None,
                None,
                None,
                None,
            )
            .await
            .expect("create first session should succeed");
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        let second = store
            .create_session(
                &sample_context(),
                "agent-a",
                "qwen-max",
                None,
                None,
                None,
                None,
                None,
            )
            .await
            .expect("create second session should succeed");
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        let third = store
            .create_session(
                &sample_context(),
                "agent-a",
                "qwen-max",
                None,
                None,
                None,
                None,
                None,
            )
            .await
            .expect("create third session should succeed");

        let records = store
            .list_session_records(&SessionQuery {
                user_id: None,
                channel_id: None,
                provider: None,
                project_id: None,
                limit: Some(1),
                offset: Some(1),
            })
            .await
            .expect("list paged sessions should succeed");

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, second.id);
        assert_ne!(records[0].id, first.id);
        assert_ne!(records[0].id, third.id);
        assert_eq!(
            store
                .count_session_records(&SessionQuery {
                    user_id: None,
                    channel_id: None,
                    provider: None,
                    project_id: None,
                    limit: Some(1),
                    offset: Some(1),
                })
                .await
                .expect("count sessions should succeed"),
            3
        );
    }

    #[tokio::test]
    async fn test_list_projects_returns_session_count() {
        let storage = Arc::new(SqliteStorage::new("sqlite::memory:").await.unwrap());
        let store = DatabaseSessionStore::new(storage).await.unwrap();

        let project = store
            .create_project("repo", &PathBuf::from("/repo"))
            .await
            .expect("create project should succeed");

        store
            .create_session(
                &sample_context(),
                "agent-a",
                "qwen-max",
                None,
                None,
                None,
                Some(project.project_id.clone()),
                None,
            )
            .await
            .expect("create bound session should succeed");

        let projects = store
            .list_projects()
            .await
            .expect("list projects should succeed");
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].project_id, project.project_id);
        assert_eq!(projects[0].session_count, 1);

        let loaded = store
            .get_project(&project.project_id)
            .await
            .expect("get project should succeed")
            .expect("project should exist");
        assert_eq!(loaded.session_count, 1);
    }
}
