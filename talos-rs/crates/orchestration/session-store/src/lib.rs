use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use talos_error::TalosResult;
use talos_storage::{Row, Storage};
use talos_types::talos::TalosUsage;
use talos_types::talos::{TalosContext, TalosProvider, TalosSourceType};
use talos_types::{
    SessionConfigOverridePolicy, SessionPatch, SessionPatchAck, SessionQuery, SessionRecord,
};

#[async_trait]
pub trait SessionStore: Send + Sync {
    async fn create_session(
        &self,
        context: &TalosContext,
        agent_id: &str,
        default_model: &str,
        requested_model: Option<&str>,
        dm_scope: Option<String>,
    ) -> TalosResult<SessionRecord>;

    async fn resolve_or_create_session(
        &self,
        context: &TalosContext,
        agent_id: &str,
        default_model: &str,
        requested_model: Option<&str>,
        dm_scope: Option<String>,
    ) -> TalosResult<SessionRecord>;

    async fn get_session_record(&self, session_id: &str) -> TalosResult<Option<SessionRecord>>;

    async fn list_session_records(&self, query: &SessionQuery) -> TalosResult<Vec<SessionRecord>>;

    async fn patch_session_record(
        &self,
        session_id: &str,
        patch: &SessionPatch,
    ) -> TalosResult<SessionPatchAck>;

    async fn append_message_by_session_id(
        &self,
        session_id: &str,
        role: &str,
        content: &str,
    ) -> TalosResult<()>;

    async fn record_session_progress(
        &self,
        session_id: &str,
        message_delta: u64,
        usage: &TalosUsage,
        model: Option<&str>,
    ) -> TalosResult<()>;
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
        let sql = "SELECT session_key, user_id, channel, thread_id, created_at, last_activity, context_json, model, message_count, usage_json, conf_json FROM sessions WHERE session_key = ?";
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
            conf: Self::parse_conf_json(row.get_string("conf_json"))
                .into_iter()
                .collect::<std::collections::HashMap<_, _>>(),
        }
    }

    fn parse_conf_json(conf_json: Option<String>) -> serde_json::Map<String, Value> {
        conf_json
            .and_then(|v| serde_json::from_str::<Value>(&v).ok())
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default()
    }

    fn build_conf_json(conf: serde_json::Map<String, Value>) -> TalosResult<String> {
        serde_json::to_string(&Value::Object(conf))
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
}

#[async_trait]
impl SessionStore for DatabaseSessionStore {
    async fn create_session(
        &self,
        context: &TalosContext,
        agent_id: &str,
        default_model: &str,
        requested_model: Option<&str>,
        _dm_scope: Option<String>,
    ) -> TalosResult<SessionRecord> {
        let session_id = Self::new_session_id();
        let now = chrono::Utc::now().timestamp_millis();
        let model = requested_model.unwrap_or(default_model).to_string();
        let conf: serde_json::Map<String, Value> = if requested_model.is_some() {
            SessionConfigOverridePolicy::single_model_conf(&model)
                .into_iter()
                .collect()
        } else {
            serde_json::Map::new()
        };

        let record = SessionRecord {
            id: session_id.clone(),
            context: context.clone(),
            model: model.clone(),
            created_at: now.try_into().unwrap_or_default(),
            last_activity: now.try_into().unwrap_or_default(),
            message_count: 0,
            usage: TalosUsage::default(),
            conf: conf
                .clone()
                .into_iter()
                .collect::<std::collections::HashMap<_, _>>(),
        };

        let context_json = Self::build_context_json(context)?;
        let usage_json = Self::build_usage_json(&record.usage)?;
        let conf_json = Self::build_conf_json(conf)?;

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
                "context_json".to_string(),
                "model".to_string(),
                "message_count".to_string(),
                "usage_json".to_string(),
                "conf_json".to_string(),
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
                Value::String(context_json),
                Value::String(model),
                Value::Number(0.into()),
                Value::String(usage_json),
                Value::String(conf_json),
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
    ) -> TalosResult<SessionRecord> {
        let dm_scope = Self::resolve_dm_scope(dm_scope.as_deref());
        let session_key = Self::stable_session_key(context, agent_id, &dm_scope);
        let now = chrono::Utc::now().timestamp_millis();

        if let Some(existing) = self.fetch_session_row(&session_key).await? {
            let mut record = Self::parse_session_record(&existing);
            let mut conf = Self::parse_conf_json(existing.get_string("conf_json"));
            if let Some(model) = requested_model {
                record.model = model.to_string();
                conf = SessionConfigOverridePolicy::single_model_conf(model)
                    .into_iter()
                    .collect();
            }
            record.conf = conf
                .clone()
                .into_iter()
                .collect::<std::collections::HashMap<_, _>>();
            record.last_activity = now.try_into().unwrap_or_default();

            let context_json = Self::build_context_json(&record.context)?;
            let conf_json = Self::build_conf_json(conf)?;

            let update_row = Row::new(
                vec![
                    "last_activity".to_string(),
                    "context_json".to_string(),
                    "model".to_string(),
                    "message_count".to_string(),
                    "usage_json".to_string(),
                    "conf_json".to_string(),
                ],
                vec![
                    Value::Number(now.into()),
                    Value::String(context_json),
                    Value::String(record.model.clone()),
                    Value::Number(serde_json::Number::from(record.message_count)),
                    Value::String(Self::build_usage_json(&record.usage)?),
                    Value::String(conf_json),
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
        let conf: serde_json::Map<String, Value> = if requested_model.is_some() {
            SessionConfigOverridePolicy::single_model_conf(&model)
                .into_iter()
                .collect()
        } else {
            serde_json::Map::new()
        };
        let record = SessionRecord {
            id: session_key.clone(),
            context: context.clone(),
            model: model.clone(),
            created_at: now.try_into().unwrap_or_default(),
            last_activity: now.try_into().unwrap_or_default(),
            message_count: 0,
            usage: TalosUsage::default(),
            conf: conf
                .clone()
                .into_iter()
                .collect::<std::collections::HashMap<_, _>>(),
        };

        let context_json = Self::build_context_json(context)?;
        let usage_json = Self::build_usage_json(&record.usage)?;
        let conf_json = Self::build_conf_json(conf)?;

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
                "context_json".to_string(),
                "model".to_string(),
                "message_count".to_string(),
                "usage_json".to_string(),
                "conf_json".to_string(),
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
                Value::String(context_json),
                Value::String(model),
                Value::Number(0.into()),
                Value::String(usage_json),
                Value::String(conf_json),
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
        let sql = "SELECT session_key, user_id, channel, thread_id, created_at, last_activity, context_json, model, message_count, usage_json, conf_json FROM sessions ORDER BY last_activity DESC";
        let rows = self.storage.query(sql, &[]).await?;
        let mut records = Vec::new();
        for row in rows {
            let record = Self::parse_session_record(&row);
            if query
                .user_id
                .as_ref()
                .is_some_and(|v| &record.context.user_id != v)
            {
                continue;
            }
            if query
                .channel_id
                .as_ref()
                .is_some_and(|v| record.context.channel_id.as_ref() != Some(v))
            {
                continue;
            }
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

    async fn patch_session_record(
        &self,
        session_id: &str,
        patch: &SessionPatch,
    ) -> TalosResult<SessionPatchAck> {
        SessionConfigOverridePolicy::validate_conf(&patch.conf)?;
        let Some(row) = self.fetch_session_row(session_id).await? else {
            return Ok(SessionPatchAck { success: false });
        };
        let mut record = Self::parse_session_record(&row);
        let mut conf = Self::parse_conf_json(row.get_string("conf_json"));

        if let Some(model) = SessionConfigOverridePolicy::extract_agent_model(&patch.conf)? {
            record.model = model.clone();
            conf = SessionConfigOverridePolicy::single_model_conf(&model)
                .into_iter()
                .collect();
        }
        record.last_activity = chrono::Utc::now()
            .timestamp_millis()
            .try_into()
            .unwrap_or_default();
        let context_json = Self::build_context_json(&record.context)?;
        let conf_json = Self::build_conf_json(conf)?;

        let update_row = Row::new(
            vec![
                "last_activity".to_string(),
                "context_json".to_string(),
                "model".to_string(),
                "message_count".to_string(),
                "usage_json".to_string(),
                "conf_json".to_string(),
            ],
            vec![
                Value::Number(serde_json::Number::from(record.last_activity)),
                Value::String(context_json),
                Value::String(record.model.clone()),
                Value::Number(serde_json::Number::from(record.message_count)),
                Value::String(Self::build_usage_json(&record.usage)?),
                Value::String(conf_json),
            ],
        );
        self.storage
            .update("sessions", &update_row, "session_key = ?", &[session_id])
            .await?;
        Ok(SessionPatchAck { success: true })
    }

    async fn append_message_by_session_id(
        &self,
        session_id: &str,
        role: &str,
        content: &str,
    ) -> TalosResult<()> {
        let now = chrono::Utc::now().timestamp_millis();
        let msg_row = Row::new(
            vec![
                "session_key".to_string(),
                "role".to_string(),
                "content".to_string(),
                "timestamp".to_string(),
            ],
            vec![
                Value::String(session_id.to_string()),
                Value::String(role.to_string()),
                Value::String(content.to_string()),
                Value::Number(now.into()),
            ],
        );
        self.storage.insert("messages", &msg_row).await?;

        Ok(())
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
        record.usage.input_tokens = record.usage.input_tokens.saturating_add(usage.input_tokens);
        record.usage.output_tokens = record
            .usage
            .output_tokens
            .saturating_add(usage.output_tokens);
        record.usage.total_tokens = record.usage.total_tokens.saturating_add(usage.total_tokens);
        if let Some(model) = model {
            record.model = model.to_string();
        }

        let row = self.fetch_session_row(session_id).await?;
        let conf_json = Self::build_conf_json(Self::parse_conf_json(
            row.and_then(|stored| stored.get_string("conf_json")),
        ))?;
        let context_json = Self::build_context_json(&record.context)?;

        let update_row = Row::new(
            vec![
                "last_activity".to_string(),
                "context_json".to_string(),
                "model".to_string(),
                "message_count".to_string(),
                "usage_json".to_string(),
                "conf_json".to_string(),
            ],
            vec![
                Value::Number(now.into()),
                Value::String(context_json),
                Value::String(record.model.clone()),
                Value::Number(serde_json::Number::from(record.message_count)),
                Value::String(Self::build_usage_json(&record.usage)?),
                Value::String(conf_json),
            ],
        );
        self.storage
            .update("sessions", &update_row, "session_key = ?", &[session_id])
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
        _dm_scope: Option<String>,
    ) -> TalosResult<SessionRecord> {
        Ok(SessionRecord {
            id: DatabaseSessionStore::new_session_id(),
            context: context.clone(),
            model: requested_model.unwrap_or(default_model).to_string(),
            created_at: 0,
            last_activity: 0,
            message_count: 0,
            usage: TalosUsage::default(),
            conf: requested_model
                .map(|model| {
                    let mut conf = std::collections::HashMap::new();
                    conf.insert("agent.model".to_string(), Value::String(model.to_string()));
                    conf
                })
                .unwrap_or_default(),
        })
    }

    async fn resolve_or_create_session(
        &self,
        context: &TalosContext,
        agent_id: &str,
        default_model: &str,
        requested_model: Option<&str>,
        dm_scope: Option<String>,
    ) -> TalosResult<SessionRecord> {
        let dm_scope = DatabaseSessionStore::resolve_dm_scope(dm_scope.as_deref());
        Ok(SessionRecord {
            id: DatabaseSessionStore::stable_session_key(context, agent_id, &dm_scope),
            context: context.clone(),
            model: requested_model.unwrap_or(default_model).to_string(),
            created_at: 0,
            last_activity: 0,
            message_count: 0,
            usage: TalosUsage::default(),
            conf: requested_model
                .map(|model| {
                    let mut conf = std::collections::HashMap::new();
                    conf.insert("agent.model".to_string(), Value::String(model.to_string()));
                    conf
                })
                .unwrap_or_default(),
        })
    }

    async fn get_session_record(&self, session_id: &str) -> TalosResult<Option<SessionRecord>> {
        Ok(Some(SessionRecord {
            id: session_id.to_string(),
            context: TalosContext {
                source_type: TalosSourceType::System,
                provider: TalosProvider::System,
                user_id: "mock-user".to_string(),
                message_id: "message-mock".to_string(),
                trace_id: "trace-mock".to_string(),
                channel_id: Some("default".to_string()),
                thread_id: None,
                environment: None,
            },
            model: "default".to_string(),
            created_at: 0,
            last_activity: 0,
            message_count: 0,
            usage: TalosUsage::default(),
            conf: std::collections::HashMap::new(),
        }))
    }

    async fn list_session_records(&self, _query: &SessionQuery) -> TalosResult<Vec<SessionRecord>> {
        Ok(Vec::new())
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

    async fn append_message_by_session_id(
        &self,
        _session_id: &str,
        _role: &str,
        _content: &str,
    ) -> TalosResult<()> {
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
}

#[cfg(test)]
mod scope_tests {
    use super::*;
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
}
