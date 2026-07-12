use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use talos_error::TalosResult;
use talos_storage::{Row, Storage};
use talos_types::talos::{TalosContext, TokenUsage};
use talos_types::{SessionConf, DEFAULT_SESSION_TITLE};

use super::{SessionQuery, SessionRecord};

#[async_trait]
pub trait SessionStore: Send + Sync {
    async fn insert_session_record(
        &self,
        record: &SessionRecord,
        agent_id: &str,
        scope: &str,
    ) -> TalosResult<()>;

    async fn get_session_record(&self, session_id: &str) -> TalosResult<Option<SessionRecord>>;

    async fn list_session_records(&self, query: &SessionQuery) -> TalosResult<Vec<SessionRecord>>;

    async fn count_session_records(&self, query: &SessionQuery) -> TalosResult<usize>;

    async fn update_session_record(&self, record: &SessionRecord) -> TalosResult<bool>;

    async fn delete_session_record(&self, session_id: &str) -> TalosResult<bool>;
}

pub struct DatabaseSessionStore {
    pub(crate) storage: Arc<dyn Storage>,
}

impl DatabaseSessionStore {
    pub async fn new(storage: Arc<dyn Storage>) -> TalosResult<Self> {
        storage.init_schema().await?;
        Ok(Self { storage })
    }

    async fn fetch_session_row(&self, session_key: &str) -> TalosResult<Option<Row>> {
        let sql = "SELECT session_key, title, user_id, channel, thread_id, created_at, last_activity, context_json, model, message_count, usage_json, conf_json, workspace_id, project_id, current_directory FROM sessions WHERE session_key = ?";
        let rows = self.storage.query(sql, &[session_key]).await?;
        Ok(rows.into_iter().next())
    }

    fn parse_session_record(row: &Row) -> TalosResult<SessionRecord> {
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
            .and_then(|raw| serde_json::from_str::<TokenUsage>(&raw).ok())
            .unwrap_or_default();

        let id = row.get_string("session_key").unwrap_or_default();
        let conf = Self::parse_conf_json(row.get_string("conf_json"), &id)?;

        Ok(SessionRecord {
            id,
            workspace_id: Self::normalized_optional_string(row.get_string("workspace_id")),
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
            conf,
            project_id: Self::normalized_optional_string(row.get_string("project_id")),
            current_directory: Self::normalized_optional_string(
                row.get_string("current_directory"),
            )
            .map(std::path::PathBuf::from),
        })
    }

    fn parse_conf_json(conf_json: Option<String>, session_id: &str) -> TalosResult<SessionConf> {
        let Some(conf_json) = conf_json else {
            return Ok(SessionConf::default());
        };
        serde_json::from_str(&conf_json).map_err(|error| {
            talos_error::TalosError::SerializationError(format!(
                "parse session '{session_id}' conf_json: {error}"
            ))
        })
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

    fn build_usage_json(usage: &TokenUsage) -> TalosResult<String> {
        serde_json::to_string(usage)
            .map_err(|e| talos_error::TalosError::SerializationError(e.to_string()))
    }

    fn record_update_row(record: &SessionRecord) -> TalosResult<Row> {
        Ok(Row::new(
            vec![
                "last_activity".to_string(),
                "title".to_string(),
                "context_json".to_string(),
                "model".to_string(),
                "message_count".to_string(),
                "usage_json".to_string(),
                "conf_json".to_string(),
                "workspace_id".to_string(),
                "project_id".to_string(),
                "current_directory".to_string(),
            ],
            vec![
                Value::Number(serde_json::Number::from(record.last_activity)),
                Value::String(record.title.clone()),
                Value::String(Self::build_context_json(&record.context)?),
                Value::String(record.model.clone()),
                Value::Number(serde_json::Number::from(record.message_count)),
                Value::String(Self::build_usage_json(&record.usage)?),
                Value::String(Self::build_conf_json(&record.conf)?),
                record
                    .workspace_id
                    .clone()
                    .map(Value::String)
                    .unwrap_or(Value::Null),
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
        ))
    }
}

#[async_trait]
impl SessionStore for DatabaseSessionStore {
    async fn insert_session_record(
        &self,
        record: &SessionRecord,
        agent_id: &str,
        scope: &str,
    ) -> TalosResult<()> {
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
                "workspace_id".to_string(),
                "project_id".to_string(),
                "current_directory".to_string(),
            ],
            vec![
                Value::String(record.id.clone()),
                Value::String(record.user_id.clone()),
                record
                    .context
                    .channel_id
                    .clone()
                    .map(Value::String)
                    .unwrap_or(Value::Null),
                Value::String(scope.to_string()),
                Value::String(record.user_id.clone()),
                Value::String(agent_id.to_string()),
                Value::Null,
                record
                    .context
                    .thread_id
                    .clone()
                    .map(Value::String)
                    .unwrap_or(Value::Null),
                Value::Number(serde_json::Number::from(record.created_at)),
                Value::Number(serde_json::Number::from(record.last_activity)),
                Value::String(record.title.clone()),
                Value::String(Self::build_context_json(&record.context)?),
                Value::String(record.model.clone()),
                Value::Number(serde_json::Number::from(record.message_count)),
                Value::String(Self::build_usage_json(&record.usage)?),
                Value::String(Self::build_conf_json(&record.conf)?),
                record
                    .workspace_id
                    .clone()
                    .map(Value::String)
                    .unwrap_or(Value::Null),
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
        self.storage.insert("sessions", &row).await?;
        Ok(())
    }

    async fn get_session_record(&self, session_id: &str) -> TalosResult<Option<SessionRecord>> {
        let row = self.fetch_session_row(session_id).await?;
        row.as_ref().map(Self::parse_session_record).transpose()
    }

    async fn list_session_records(&self, query: &SessionQuery) -> TalosResult<Vec<SessionRecord>> {
        let mut sql = "SELECT session_key, title, user_id, channel, thread_id, created_at, last_activity, context_json, model, message_count, usage_json, conf_json, workspace_id, project_id, current_directory FROM sessions".to_string();
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
            let record = Self::parse_session_record(&row)?;
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

    async fn update_session_record(&self, record: &SessionRecord) -> TalosResult<bool> {
        let row = Self::record_update_row(record)?;
        let affected = self
            .storage
            .update("sessions", &row, "session_key = ?", &[record.id.as_str()])
            .await?;
        Ok(affected > 0)
    }

    async fn delete_session_record(&self, session_id: &str) -> TalosResult<bool> {
        self.storage
            .delete("timeline_events", "session_id = ?", &[session_id])
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
        Ok(affected > 0)
    }
}
