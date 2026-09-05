use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use talos_agent_api::AgentEventPayload;
use talos_error::{TalosError, TalosResult};
use talos_storage::{Row, Storage};
use talos_types::talos::{SessionSourceProvider, SessionSourceType, TalosSessionEnvironment};
use talos_types::{AgentPath, ContextUsage, SessionConf, SessionUsage, DEFAULT_SESSION_TITLE};

use super::{
    AccumulateSessionUsageParams, AgentExecutionStore, PatchSessionParams, SessionQuery,
    SessionRecord, SessionSummary, SessionTreeIdentity, SessionTreeScope,
};

#[async_trait]
pub trait SessionStore: Send + Sync {
    async fn insert_session_record(&self, record: &SessionRecord) -> TalosResult<()>;

    async fn get_session_record(&self, session_id: &str) -> TalosResult<Option<SessionRecord>>;

    /// Sets an untitled Session's generated title only while the originating Turn still owns it.
    async fn set_generated_title(
        &self,
        identity: &talos_agent_api::AgentTurnRef,
        title: &str,
    ) -> TalosResult<bool>;

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

    /// Returns whether any Session currently owns a non-terminal Agent Turn.
    async fn has_nonterminal_turns(&self) -> TalosResult<bool>;

    /// Atomically rejects active Sessions and replaces only settings-owned columns.
    async fn patch_session(&self, params: PatchSessionParams)
        -> TalosResult<Option<SessionRecord>>;

    /// Atomically accumulates one token delta without replacing other Session summaries.
    async fn accumulate_session_usage(
        &self,
        params: AccumulateSessionUsageParams,
    ) -> TalosResult<Option<SessionRecord>>;

    /// Atomically removes canonical conversation facts and resets one idle Session summary.
    async fn clear_session_history(&self, session_id: &str, cleared_at: u64) -> TalosResult<bool>;

    /// Atomically replaces the latest context snapshot without overwriting other Session fields.
    async fn update_context_usage(
        &self,
        session_id: &str,
        context_usage: &ContextUsage,
    ) -> TalosResult<bool>;

    /// Advances the contiguous session range already processed into long-term memory.
    async fn advance_memory_processed_until_event_seq(
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
        let sql = "SELECT session_key, agent_id, parent_session_id, parent_turn_id, tree_root_session_id, agent_path, title, user_id, source_type, source_provider, channel_id, thread_id, environment_json, created_at, last_activity, message_count, latest_turn_seq, memory_processed_until_event_seq, usage_json, conf_json, workspace_id, project_id, current_directory FROM sessions WHERE session_key = ?";
        let rows = self.storage.query(sql, &[session_key]).await?;
        Ok(rows.into_iter().next())
    }

    fn parse_session_record(row: &Row) -> TalosResult<SessionRecord> {
        let message_count = row
            .get_i64("message_count")
            .unwrap_or_default()
            .try_into()
            .unwrap_or_default();
        let id = row.get_string("session_key").unwrap_or_default();
        let usage = Self::parse_required_json::<SessionUsage>(
            row.get_string("usage_json"),
            &id,
            "usage_json",
        )?;
        let source_type = Self::parse_required_enum::<SessionSourceType>(
            row.get_string("source_type"),
            &id,
            "source_type",
        )?;
        let source_provider = Self::parse_required_enum::<SessionSourceProvider>(
            row.get_string("source_provider"),
            &id,
            "source_provider",
        )?;
        let environment = Self::parse_required_json::<Option<TalosSessionEnvironment>>(
            row.get_string("environment_json"),
            &id,
            "environment_json",
        )?;

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
        let conf = Self::parse_required_json::<SessionConf>(
            row.get_string("conf_json"),
            &id,
            "conf_json",
        )?;
        let agent_id =
            Self::normalized_optional_string(row.get_string("agent_id")).ok_or_else(|| {
                TalosError::internal(format!("session '{id}' is missing its agent binding"))
            })?;

        let workspace_id = Self::normalized_optional_string(row.get_string("workspace_id"))
            .ok_or_else(|| {
                TalosError::internal(format!("session '{id}' is missing its workspace binding"))
            })?;

        Ok(SessionRecord {
            id,
            agent_id,
            tree_identity,
            workspace_id,
            title: row
                .get_string("title")
                .filter(|title| !title.trim().is_empty())
                .unwrap_or_else(|| DEFAULT_SESSION_TITLE.to_string()),
            user_id: row.get_string("user_id").unwrap_or_default(),
            source_type,
            source_provider,
            channel_id: Self::normalized_optional_string(row.get_string("channel_id")),
            thread_id: Self::normalized_optional_string(row.get_string("thread_id")),
            environment,
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
            memory_processed_until_event_seq: row
                .get_i64("memory_processed_until_event_seq")
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

    fn parse_required_json<T: serde::de::DeserializeOwned>(
        raw: Option<String>,
        session_id: &str,
        column: &str,
    ) -> TalosResult<T> {
        let raw = raw.ok_or_else(|| {
            TalosError::internal(format!("session '{session_id}' is missing {column}"))
        })?;
        serde_json::from_str(&raw).map_err(|error| {
            TalosError::internal(format!("parse session '{session_id}' {column}: {error}"))
        })
    }

    fn parse_required_enum<T: serde::de::DeserializeOwned>(
        raw: Option<String>,
        session_id: &str,
        column: &str,
    ) -> TalosResult<T> {
        let raw = raw.ok_or_else(|| {
            TalosError::internal(format!("session '{session_id}' is missing {column}"))
        })?;
        serde_json::from_value(Value::String(raw)).map_err(|error| {
            TalosError::internal(format!("parse session '{session_id}' {column}: {error}"))
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

    fn build_environment_json(
        environment: Option<&TalosSessionEnvironment>,
    ) -> TalosResult<String> {
        serde_json::to_string(&environment).map_err(|error| TalosError::internal(error.to_string()))
    }

    fn build_usage_json(usage: &SessionUsage) -> TalosResult<String> {
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

    async fn fetch_session_in_transaction(
        transaction: &mut dyn talos_storage::StorageTransaction,
        session_id: &str,
    ) -> TalosResult<Option<SessionRecord>> {
        let rows = transaction
            .query(
                "SELECT session_key, agent_id, parent_session_id, parent_turn_id, tree_root_session_id, agent_path, title, user_id, source_type, source_provider, channel_id, thread_id, environment_json, created_at, last_activity, message_count, latest_turn_seq, memory_processed_until_event_seq, usage_json, conf_json, workspace_id, project_id, current_directory FROM sessions WHERE session_key = ?",
                &[session_id],
            )
            .await?;
        rows.first().map(Self::parse_session_record).transpose()
    }
}

#[async_trait]
impl SessionStore for DatabaseSessionStore {
    async fn set_generated_title(
        &self,
        identity: &talos_agent_api::AgentTurnRef,
        title: &str,
    ) -> TalosResult<bool> {
        let affected = self.storage.execute(
            "UPDATE sessions SET title = ? WHERE session_key = ? AND title = ? AND EXISTS (SELECT 1 FROM agent_active_turns WHERE session_id = ? AND turn_id = ? AND state = 'running')",
            &[title, &identity.session_id, DEFAULT_SESSION_TITLE, &identity.session_id, &identity.turn_id],
        ).await?;
        Ok(affected > 0)
    }

    async fn insert_session_record(&self, record: &SessionRecord) -> TalosResult<()> {
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
                "source_type".to_string(),
                "source_provider".to_string(),
                "channel_id".to_string(),
                "agent_id".to_string(),
                "parent_session_id".to_string(),
                "parent_turn_id".to_string(),
                "tree_root_session_id".to_string(),
                "agent_path".to_string(),
                "thread_id".to_string(),
                "environment_json".to_string(),
                "created_at".to_string(),
                "last_activity".to_string(),
                "title".to_string(),
                "message_count".to_string(),
                "latest_turn_seq".to_string(),
                "memory_processed_until_event_seq".to_string(),
                "usage_json".to_string(),
                "conf_json".to_string(),
                "workspace_id".to_string(),
                "project_id".to_string(),
                "current_directory".to_string(),
            ],
            vec![
                Value::String(record.id.clone()),
                Value::String(record.user_id.clone()),
                Value::String(record.source_type.as_str().to_string()),
                Value::String(record.source_provider.as_str().to_string()),
                record
                    .channel_id
                    .clone()
                    .map(Value::String)
                    .unwrap_or(Value::Null),
                Value::String(record.agent_id.clone()),
                parent_session_id.map(Value::String).unwrap_or(Value::Null),
                parent_turn_id.map(Value::String).unwrap_or(Value::Null),
                tree_root_session_id
                    .map(Value::String)
                    .unwrap_or(Value::Null),
                agent_path.map(Value::String).unwrap_or(Value::Null),
                record
                    .thread_id
                    .clone()
                    .map(Value::String)
                    .unwrap_or(Value::Null),
                Value::String(Self::build_environment_json(record.environment.as_ref())?),
                Value::Number(serde_json::Number::from(record.created_at)),
                Value::Number(serde_json::Number::from(record.last_activity)),
                Value::String(record.title.clone()),
                Value::Number(serde_json::Number::from(record.message_count)),
                Value::Number(serde_json::Number::from(record.latest_turn_seq)),
                Value::Number(serde_json::Number::from(
                    record.memory_processed_until_event_seq,
                )),
                Value::String(Self::build_usage_json(&record.usage)?),
                Value::String(Self::build_conf_json(&record.conf)?),
                Value::String(record.workspace_id.clone()),
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
                "SELECT session_key, agent_id, parent_session_id, parent_turn_id, tree_root_session_id, agent_path, title, user_id, source_type, source_provider, channel_id, thread_id, environment_json, created_at, last_activity, message_count, latest_turn_seq, memory_processed_until_event_seq, usage_json, conf_json, workspace_id, project_id, current_directory FROM sessions WHERE tree_root_session_id = ? AND agent_path = ?",
                &[tree_root_session_id, agent_path.as_str()],
            )
            .await?;
        rows.first().map(Self::parse_session_record).transpose()
    }

    async fn list_session_records(&self, query: &SessionQuery) -> TalosResult<Vec<SessionRecord>> {
        let mut sql = "SELECT session_key, agent_id, parent_session_id, parent_turn_id, tree_root_session_id, agent_path, title, user_id, source_type, source_provider, channel_id, thread_id, environment_json, created_at, last_activity, message_count, latest_turn_seq, memory_processed_until_event_seq, usage_json, conf_json, workspace_id, project_id, current_directory FROM sessions".to_string();
        let mut clauses = Vec::new();
        let mut params = Vec::new();
        Self::append_tree_scope(&query.tree_scope, &mut clauses, &mut params);
        if let Some(user_id) = query.user_id.as_deref() {
            clauses.push("user_id = ?");
            params.push(user_id.to_string());
        }
        if let Some(channel_id) = query.channel_id.as_deref() {
            clauses.push("channel_id = ?");
            params.push(channel_id.to_string());
        }
        if let Some(project_id) = query.project_id.as_deref() {
            clauses.push("project_id = ?");
            params.push(project_id.to_string());
        }
        if query.memory_pending_only {
            clauses.push(
                "EXISTS (SELECT 1 FROM agent_events AS event WHERE event.session_id = sessions.session_key AND json_extract(event.payload_json, '$.type') IN ('turnCompleted', 'turnFailed') AND event.event_seq > sessions.memory_processed_until_event_seq)",
            );
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
                .is_some_and(|value| record.source_provider.as_str() != value)
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
            clauses.push("channel_id = ?");
            params.push(channel_id.to_string());
        }
        if let Some(project_id) = query.project_id.as_deref() {
            clauses.push("project_id = ?");
            params.push(project_id.to_string());
        }
        if query.memory_pending_only {
            clauses.push(
                "EXISTS (SELECT 1 FROM agent_events AS event WHERE event.session_id = sessions.session_key AND json_extract(event.payload_json, '$.type') IN ('turnCompleted', 'turnFailed') AND event.event_seq > sessions.memory_processed_until_event_seq)",
            );
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
                "SELECT 1 AS active FROM sessions AS child INNER JOIN agent_active_turns AS turn ON turn.session_id = child.session_key WHERE child.tree_root_session_id = ? LIMIT 1",
                &[tree_root_session_id],
            )
            .await?;
        Ok(!rows.is_empty())
    }

    async fn has_nonterminal_turns(&self) -> TalosResult<bool> {
        let rows = self
            .storage
            .query("SELECT 1 AS active FROM agent_active_turns LIMIT 1", &[])
            .await?;
        Ok(!rows.is_empty())
    }

    async fn patch_session(
        &self,
        params: PatchSessionParams,
    ) -> TalosResult<Option<SessionRecord>> {
        let mut transaction = self.storage.begin_transaction().await?;
        let active = transaction
            .query(
                "SELECT 1 AS active FROM agent_active_turns WHERE session_id = ? LIMIT 1",
                &[params.session_id.as_str()],
            )
            .await?;
        if !active.is_empty() {
            transaction.rollback().await?;
            return Err(TalosError::public(
                talos_error::ErrorCode::ActiveTurnRunning,
                format!("session `{}` already has an active turn", params.session_id),
            ));
        }

        let conf_json = Self::build_conf_json(&params.conf)?;
        let current_directory = params
            .current_directory
            .as_ref()
            .map(|path| path.display().to_string());
        let last_activity = params.last_activity.to_string();
        let affected = if params.reset_context_usage {
            transaction
                .execute(
                    "UPDATE sessions SET conf_json = ?, current_directory = NULLIF(?, ''), last_activity = ?, usage_json = json_set(usage_json, '$.context', NULL) WHERE session_key = ?",
                    &[
                        conf_json.as_str(),
                        current_directory.as_deref().unwrap_or_default(),
                        last_activity.as_str(),
                        params.session_id.as_str(),
                    ],
                )
                .await?
        } else {
            transaction
                .execute(
                    "UPDATE sessions SET conf_json = ?, current_directory = NULLIF(?, ''), last_activity = ? WHERE session_key = ?",
                    &[
                        conf_json.as_str(),
                        current_directory.as_deref().unwrap_or_default(),
                        last_activity.as_str(),
                        params.session_id.as_str(),
                    ],
                )
                .await?
        };
        if affected == 0 {
            transaction.rollback().await?;
            return Ok(None);
        }
        let record =
            Self::fetch_session_in_transaction(transaction.as_mut(), params.session_id.as_str())
                .await?;
        transaction.commit().await?;
        Ok(record)
    }

    async fn accumulate_session_usage(
        &self,
        params: AccumulateSessionUsageParams,
    ) -> TalosResult<Option<SessionRecord>> {
        let mut transaction = self.storage.begin_transaction().await?;
        let Some(mut record) =
            Self::fetch_session_in_transaction(transaction.as_mut(), params.session_id.as_str())
                .await?
        else {
            transaction.rollback().await?;
            return Ok(None);
        };
        record.usage.tokens.accumulate(&params.tokens);
        record.last_activity = params.last_activity;
        let usage_json = Self::build_usage_json(&record.usage)?;
        let last_activity = params.last_activity.to_string();
        transaction
            .execute(
                "UPDATE sessions SET usage_json = ?, last_activity = ? WHERE session_key = ?",
                &[
                    usage_json.as_str(),
                    last_activity.as_str(),
                    params.session_id.as_str(),
                ],
            )
            .await?;
        transaction.commit().await?;
        Ok(Some(record))
    }

    async fn clear_session_history(&self, session_id: &str, cleared_at: u64) -> TalosResult<bool> {
        let mut transaction = self.storage.begin_transaction().await?;
        let active = transaction
            .query(
                "SELECT 1 AS active FROM agent_active_turns WHERE session_id = ? LIMIT 1",
                &[session_id],
            )
            .await?;
        if !active.is_empty() {
            transaction.rollback().await?;
            return Err(TalosError::public(
                talos_error::ErrorCode::ActiveTurnRunning,
                "cannot clear a Session with an active Agent Turn",
            ));
        }

        transaction
            .execute(
                "DELETE FROM agent_continuation_bindings WHERE session_id = ?",
                &[session_id],
            )
            .await?;
        transaction
            .execute(
                "DELETE FROM agent_command_idempotency WHERE session_id = ?",
                &[session_id],
            )
            .await?;
        transaction
            .execute(
                "DELETE FROM agent_events WHERE session_id = ?",
                &[session_id],
            )
            .await?;

        let usage_json = Self::build_usage_json(&SessionUsage::default())?;
        let cleared_at = cleared_at.to_string();
        let affected = transaction
            .execute(
                r#"
                UPDATE sessions
                SET title = ?, last_activity = ?, message_count = 0,
                    latest_turn_seq = 0, memory_processed_until_event_seq = 0,
                    usage_json = ?
                WHERE session_key = ?
                "#,
                &[
                    DEFAULT_SESSION_TITLE,
                    cleared_at.as_str(),
                    usage_json.as_str(),
                    session_id,
                ],
            )
            .await?;
        if affected == 0 {
            transaction.rollback().await?;
            return Ok(false);
        }
        transaction.commit().await?;
        Ok(true)
    }

    async fn update_context_usage(
        &self,
        session_id: &str,
        context_usage: &ContextUsage,
    ) -> TalosResult<bool> {
        let context_usage_json = serde_json::to_string(context_usage)
            .map_err(|error| TalosError::internal(error.to_string()))?;
        let affected = self
            .storage
            .execute(
                "UPDATE sessions SET usage_json = json_set(usage_json, '$.context', json(?)) WHERE session_key = ?",
                &[context_usage_json.as_str(), session_id],
            )
            .await?;
        Ok(affected > 0)
    }

    async fn advance_memory_processed_until_event_seq(
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
        if next_until_seq > expected_until_seq {
            let next_event_seq = i64::try_from(next_until_seq).map_err(|_| {
                TalosError::public(
                    talos_error::ErrorCode::InvalidRequest,
                    "memory cursor exceeds canonical event sequence range",
                )
            })?;
            let source = AgentExecutionStore::get_agent_event(self, next_event_seq)
                .await?
                .ok_or_else(|| {
                    TalosError::public(
                        talos_error::ErrorCode::InvalidRequest,
                        "memory cursor source event does not exist",
                    )
                })?;
            if source.identity.session_id != session_id
                || !matches!(
                    source.payload,
                    AgentEventPayload::TurnCompleted(_) | AgentEventPayload::TurnFailed(_)
                )
            {
                return Err(TalosError::public(
                    talos_error::ErrorCode::InvalidRequest,
                    "memory cursor must reference a terminal event from the same Session",
                ));
            }
        }
        let row = Row::new(
            vec!["memory_processed_until_event_seq".to_string()],
            vec![Value::Number(serde_json::Number::from(next_until_seq))],
        );
        let expected = expected_until_seq.to_string();
        let affected = self
            .storage
            .update(
                "sessions",
                &row,
                "session_key = ? AND memory_processed_until_event_seq = ?",
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
            .memory_processed_until_event_seq;
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
