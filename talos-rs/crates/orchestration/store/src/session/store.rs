use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use talos_error::{TalosError, TalosResult};
use talos_storage::{Row, Storage};
use talos_types::talos::{TalosContext, TokenUsage};
use talos_types::{AgentPath, SessionConf, DEFAULT_SESSION_TITLE};

use super::{
    ActiveTurnStatus, SessionQuery, SessionRecord, SessionSummary, SessionTreeIdentity,
    SessionTreeScope,
};

#[async_trait]
pub trait SessionStore: Send + Sync {
    async fn insert_session_record(&self, record: &SessionRecord, scope: &str) -> TalosResult<()>;

    async fn get_session_record(&self, session_id: &str) -> TalosResult<Option<SessionRecord>>;

    async fn get_subagent_record(
        &self,
        tree_root_session_id: &str,
        agent_path: &AgentPath,
    ) -> TalosResult<Option<SessionRecord>>;

    async fn list_session_records(&self, query: &SessionQuery) -> TalosResult<Vec<SessionRecord>>;

    async fn count_session_records(&self, query: &SessionQuery) -> TalosResult<usize>;

    /// Returns current Session and Message counts for one owner.
    async fn summarize_sessions(&self, user_id: &str) -> TalosResult<SessionSummary>;

    /// Returns whether one Session tree contains a non-terminal child Turn.
    async fn has_active_subagent_turn(&self, tree_root_session_id: &str) -> TalosResult<bool>;

    async fn update_session_record(&self, record: &SessionRecord) -> TalosResult<bool>;

    /// Advances the contiguous session range already processed into long-term memory.
    async fn advance_memory_processed_until_seq(
        &self,
        session_id: &str,
        expected_until_seq: u64,
        next_until_seq: u64,
    ) -> TalosResult<()>;

    async fn delete_session_record(&self, session_id: &str) -> TalosResult<bool>;

    /// Deletes one Primary Session and every Session in its Agent tree.
    async fn delete_session_tree(&self, tree_root_session_id: &str) -> TalosResult<bool>;
}

pub struct DatabaseSessionStore {
    pub(crate) storage: Arc<dyn Storage>,
}

impl DatabaseSessionStore {
    pub async fn new(storage: Arc<dyn Storage>) -> TalosResult<Self> {
        Ok(Self { storage })
    }

    async fn fetch_session_row(&self, session_key: &str) -> TalosResult<Option<Row>> {
        let sql = "SELECT session_key, agent_id, parent_session_id, parent_turn_id, tree_root_session_id, agent_path, title, user_id, channel, thread_id, created_at, last_activity, context_json, model, message_count, latest_turn_seq, memory_processed_until_seq, usage_json, conf_json, workspace_id, project_id, current_directory FROM sessions WHERE session_key = ?";
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
        let parent_session_id =
            Self::normalized_optional_string(row.get_string("parent_session_id"));
        let parent_turn_id = Self::normalized_optional_string(row.get_string("parent_turn_id"));
        let tree_root_session_id =
            Self::normalized_optional_string(row.get_string("tree_root_session_id"));
        let agent_path = Self::normalized_optional_string(row.get_string("agent_path"))
            .map(|path| {
                path.parse::<AgentPath>().map_err(|error| {
                    TalosError::internal(format!("session '{id}' has invalid agent path: {error}"))
                })
            })
            .transpose()?;
        let tree_identity = match (
            parent_session_id,
            parent_turn_id,
            tree_root_session_id,
            agent_path,
        ) {
            (None, None, None, None) => SessionTreeIdentity::Primary,
            (Some(session_id), Some(turn_id), Some(tree_root_session_id), Some(path)) => {
                SessionTreeIdentity::Subagent {
                    parent: talos_types::AgentTurnRef {
                        session_id,
                        turn_id,
                    },
                    tree_root_session_id,
                    path,
                }
            }
            _ => {
                return Err(TalosError::internal(format!(
                    "session '{id}' has incomplete subagent tree identity"
                )));
            }
        };
        let conf = Self::parse_conf_json(row.get_string("conf_json"), &id)?;
        let agent_id =
            Self::normalized_optional_string(row.get_string("agent_id")).ok_or_else(|| {
                TalosError::internal(format!("session '{id}' is missing its agent binding"))
            })?;

        Ok(SessionRecord {
            id,
            agent_id,
            tree_identity,
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
            latest_turn_seq: row
                .get_i64("latest_turn_seq")
                .unwrap_or_default()
                .try_into()
                .unwrap_or_default(),
            memory_processed_until_seq: row
                .get_i64("memory_processed_until_seq")
                .unwrap_or_default()
                .try_into()
                .unwrap_or_default(),
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
            talos_error::TalosError::internal(format!(
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
        serde_json::to_string(conf).map_err(|e| talos_error::TalosError::internal(e.to_string()))
    }

    fn build_context_json(context: &TalosContext) -> TalosResult<String> {
        serde_json::to_string(context).map_err(|e| talos_error::TalosError::internal(e.to_string()))
    }

    fn build_usage_json(usage: &TokenUsage) -> TalosResult<String> {
        serde_json::to_string(usage).map_err(|e| talos_error::TalosError::internal(e.to_string()))
    }

    fn append_tree_scope(
        scope: &SessionTreeScope,
        clauses: &mut Vec<&'static str>,
        params: &mut Vec<String>,
    ) {
        match scope {
            SessionTreeScope::Primary => clauses.push("NULLIF(parent_session_id, '') IS NULL"),
            SessionTreeScope::SubagentTree {
                tree_root_session_id,
            } => {
                clauses.push("tree_root_session_id = ?");
                params.push(tree_root_session_id.clone());
            }
            SessionTreeScope::All => {}
        }
    }

    fn record_update_row(record: &SessionRecord) -> TalosResult<Row> {
        Ok(Row::new(
            vec![
                "last_activity".to_string(),
                "title".to_string(),
                "context_json".to_string(),
                "model".to_string(),
                "message_count".to_string(),
                "latest_turn_seq".to_string(),
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
                Value::Number(serde_json::Number::from(record.latest_turn_seq)),
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
    async fn insert_session_record(&self, record: &SessionRecord, scope: &str) -> TalosResult<()> {
        let (parent_session_id, parent_turn_id, tree_root_session_id, agent_path) =
            match &record.tree_identity {
                SessionTreeIdentity::Primary => (None, None, None, None),
                SessionTreeIdentity::Subagent {
                    parent,
                    tree_root_session_id,
                    path,
                } => (
                    Some(parent.session_id.clone()),
                    Some(parent.turn_id.clone()),
                    Some(tree_root_session_id.clone()),
                    Some(path.to_string()),
                ),
            };
        let row = Row::new(
            vec![
                "session_key".to_string(),
                "user_id".to_string(),
                "channel".to_string(),
                "scope".to_string(),
                "peer_id".to_string(),
                "agent_id".to_string(),
                "parent_session_id".to_string(),
                "parent_turn_id".to_string(),
                "tree_root_session_id".to_string(),
                "agent_path".to_string(),
                "account_id".to_string(),
                "thread_id".to_string(),
                "created_at".to_string(),
                "last_activity".to_string(),
                "title".to_string(),
                "context_json".to_string(),
                "model".to_string(),
                "message_count".to_string(),
                "latest_turn_seq".to_string(),
                "memory_processed_until_seq".to_string(),
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
                Value::String(record.agent_id.clone()),
                parent_session_id.map(Value::String).unwrap_or(Value::Null),
                parent_turn_id.map(Value::String).unwrap_or(Value::Null),
                tree_root_session_id
                    .map(Value::String)
                    .unwrap_or(Value::Null),
                agent_path.map(Value::String).unwrap_or(Value::Null),
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
                Value::Number(serde_json::Number::from(record.latest_turn_seq)),
                Value::Number(serde_json::Number::from(record.memory_processed_until_seq)),
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

    async fn get_subagent_record(
        &self,
        tree_root_session_id: &str,
        agent_path: &AgentPath,
    ) -> TalosResult<Option<SessionRecord>> {
        let rows = self
            .storage
            .query(
                "SELECT session_key, agent_id, parent_session_id, parent_turn_id, tree_root_session_id, agent_path, title, user_id, channel, thread_id, created_at, last_activity, context_json, model, message_count, latest_turn_seq, memory_processed_until_seq, usage_json, conf_json, workspace_id, project_id, current_directory FROM sessions WHERE tree_root_session_id = ? AND agent_path = ?",
                &[tree_root_session_id, agent_path.as_str()],
            )
            .await?;
        rows.first().map(Self::parse_session_record).transpose()
    }

    async fn list_session_records(&self, query: &SessionQuery) -> TalosResult<Vec<SessionRecord>> {
        let mut sql = "SELECT session_key, agent_id, parent_session_id, parent_turn_id, tree_root_session_id, agent_path, title, user_id, channel, thread_id, created_at, last_activity, context_json, model, message_count, latest_turn_seq, memory_processed_until_seq, usage_json, conf_json, workspace_id, project_id, current_directory FROM sessions".to_string();
        let mut clauses = Vec::new();
        let mut params = Vec::new();
        Self::append_tree_scope(&query.tree_scope, &mut clauses, &mut params);
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
        if query.memory_pending_only {
            clauses.push("latest_turn_seq > memory_processed_until_seq");
        }
        if let Some(after) = &query.after {
            clauses.push("(last_activity < ? OR (last_activity = ? AND session_key > ?))");
            params.push(after.last_activity.to_string());
            params.push(after.last_activity.to_string());
            params.push(after.session_id.clone());
        }
        if !clauses.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&clauses.join(" AND "));
        }
        sql.push_str(" ORDER BY last_activity DESC, session_key ASC");
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
        Self::append_tree_scope(&query.tree_scope, &mut clauses, &mut params);
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
        if query.memory_pending_only {
            clauses.push("latest_turn_seq > memory_processed_until_seq");
        }
        if let Some(after) = &query.after {
            clauses.push("(last_activity < ? OR (last_activity = ? AND session_key > ?))");
            params.push(after.last_activity.to_string());
            params.push(after.last_activity.to_string());
            params.push(after.session_id.clone());
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

    async fn summarize_sessions(&self, user_id: &str) -> TalosResult<SessionSummary> {
        let rows = self
            .storage
            .query(
                "SELECT COUNT(*) AS session_count, COALESCE(SUM(message_count), 0) AS message_count FROM sessions WHERE user_id = ? AND NULLIF(parent_session_id, '') IS NULL",
                &[user_id],
            )
            .await?;
        let row = rows.first().ok_or_else(|| {
            TalosError::internal("summarize sessions returned no aggregate row".to_string())
        })?;
        let session_count = row.get_i64("session_count").ok_or_else(|| {
            TalosError::internal("summarize sessions returned an invalid session count".to_string())
        })?;
        let message_count = row.get_i64("message_count").ok_or_else(|| {
            TalosError::internal("summarize sessions returned an invalid message count".to_string())
        })?;
        Ok(SessionSummary {
            session_count: u64::try_from(session_count).map_err(|_| {
                TalosError::internal(
                    "summarize sessions returned a negative session count".to_string(),
                )
            })?,
            message_count: u64::try_from(message_count).map_err(|_| {
                TalosError::internal(
                    "summarize sessions returned a negative message count".to_string(),
                )
            })?,
        })
    }

    async fn has_active_subagent_turn(&self, tree_root_session_id: &str) -> TalosResult<bool> {
        let rows = self
            .storage
            .query(
                "SELECT 1 AS active FROM sessions AS child INNER JOIN active_turns AS turn ON turn.session_id = child.session_key WHERE child.tree_root_session_id = ? AND turn.status IN (?, ?, ?) LIMIT 1",
                &[
                    tree_root_session_id,
                    ActiveTurnStatus::Running.as_str(),
                    ActiveTurnStatus::WaitingUserAction.as_str(),
                    ActiveTurnStatus::Cancelling.as_str(),
                ],
            )
            .await?;
        Ok(!rows.is_empty())
    }

    async fn update_session_record(&self, record: &SessionRecord) -> TalosResult<bool> {
        let mut record = record.clone();
        if let Some(current) = self.get_session_record(&record.id).await? {
            record.latest_turn_seq = record.latest_turn_seq.max(current.latest_turn_seq);
        }
        let row = Self::record_update_row(&record)?;
        let affected = self
            .storage
            .update("sessions", &row, "session_key = ?", &[record.id.as_str()])
            .await?;
        Ok(affected > 0)
    }

    async fn advance_memory_processed_until_seq(
        &self,
        session_id: &str,
        expected_until_seq: u64,
        next_until_seq: u64,
    ) -> TalosResult<()> {
        if next_until_seq < expected_until_seq {
            return Err(TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                "memory cursor cannot move backwards".to_string(),
            ));
        }
        let record = self.get_session_record(session_id).await?.ok_or_else(|| {
            TalosError::public(
                talos_error::ErrorCode::ResourceNotFound,
                session_id.to_string(),
            )
        })?;
        let row = Row::new(
            vec![
                "memory_processed_until_seq".to_string(),
                "latest_turn_seq".to_string(),
            ],
            vec![
                Value::Number(serde_json::Number::from(next_until_seq)),
                Value::Number(serde_json::Number::from(
                    record.latest_turn_seq.max(next_until_seq),
                )),
            ],
        );
        let expected = expected_until_seq.to_string();
        let affected = self
            .storage
            .update(
                "sessions",
                &row,
                "session_key = ? AND memory_processed_until_seq = ?",
                &[session_id, expected.as_str()],
            )
            .await?;
        if affected > 0 {
            return Ok(());
        }
        let current = self
            .get_session_record(session_id)
            .await?
            .ok_or_else(|| {
                TalosError::public(
                    talos_error::ErrorCode::ResourceNotFound,
                    session_id.to_string(),
                )
            })?
            .memory_processed_until_seq;
        if current >= next_until_seq {
            return Ok(());
        }
        Err(TalosError::public(talos_error::ErrorCode::Conflict, format!(
            "memory cursor conflict for session '{session_id}': expected {expected_until_seq}, current {current}"
        )))
    }

    async fn delete_session_record(&self, session_id: &str) -> TalosResult<bool> {
        let affected = self
            .storage
            .delete("sessions", "session_key = ?", &[session_id])
            .await?;
        Ok(affected > 0)
    }

    async fn delete_session_tree(&self, tree_root_session_id: &str) -> TalosResult<bool> {
        let affected = self
            .storage
            .delete(
                "sessions",
                "session_key = ? OR tree_root_session_id = ?",
                &[tree_root_session_id, tree_root_session_id],
            )
            .await?;
        Ok(affected > 0)
    }
}
