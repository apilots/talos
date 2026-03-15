use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use talos_error::TalosResult;
use talos_llm_provider::{ChatMessage, LlmProvider, Role};
use talos_storage::{Row, Storage};
use talos_types::canonical::CanonicalContext;
use talos_types::{SessionPatch, SessionQuery, SessionRecord};
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionData {
    pub id: String,
    pub user_id: String,
    pub channel: String,
    pub model: String,
    pub conversation_history: Vec<Message>,
    pub metadata: std::collections::HashMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
    pub timestamp: i64,
}

/// Session Manager to manage sessions with in-memory caching functionality
pub struct SessionManager {
    session_store: Arc<dyn SessionStore>,
    /// In-memory cache for active sessions
    session_cache: RwLock<std::collections::HashMap<String, Arc<RwLock<Session>>>>,
}

impl SessionManager {
    /// Creates a new SessionManager with the provided session store
    pub fn new(session_store: Arc<dyn SessionStore>) -> Self {
        Self {
            session_store,
            session_cache: RwLock::new(std::collections::HashMap::new()),
        }
    }

    /// Gets or creates a default session for backward compatibility
    pub async fn get_or_create_default(
        &self,
        user_id: &str,
        model: &str,
    ) -> TalosResult<Arc<RwLock<Session>>> {
        let session = self
            .session_store
            .get_or_create_default(user_id, model)
            .await?;
        let session_id = session.read().await.data.id.clone();

        let mut cache = self.session_cache.write().await;
        cache.insert(session_id.clone(), session.clone());

        Ok(session)
    }

    /// Gets a session by ID from cache or persists to storage if needed
    pub async fn get_session_by_id(
        &self,
        session_id: &str,
    ) -> TalosResult<Option<Arc<RwLock<Session>>>> {
        let cache = self.session_cache.read().await;
        if let Some(session) = cache.get(session_id) {
            return Ok(Some(session.clone()));
        }

        drop(cache);
        let session_opt = self.session_store.get_session_by_id(session_id).await?;

        if let Some(session) = session_opt {
            let mut cache = self.session_cache.write().await;
            cache.insert(session_id.to_string(), session.clone());
            Ok(Some(session))
        } else {
            Ok(None)
        }
    }

    /// Creates a new session with the specified model
    pub async fn create_session(
        &self,
        user_id: &str,
        model: &str,
    ) -> TalosResult<Arc<RwLock<Session>>> {
        let session_data = SessionData {
            id: uuid::Uuid::new_v4().to_string(),
            user_id: user_id.to_string(),
            channel: "session_api".to_string(),
            model: model.to_string(),
            conversation_history: Vec::new(),
            metadata: std::collections::HashMap::new(),
        };

        let session = Arc::new(RwLock::new(Session::new(session_data)));

        let session_id = session.read().await.data.id.clone();
        let mut cache = self.session_cache.write().await;
        cache.insert(session_id, session.clone());

        Ok(session)
    }

    /// Gets a session from memory cache only (does not load from storage)
    pub async fn get_cached_session(&self, session_id: &str) -> Option<Arc<RwLock<Session>>> {
        let cache = self.session_cache.read().await;
        cache.get(session_id).cloned()
    }

    /// Removes session from cache to free memory when inactive
    pub async fn evict_session(&self, session_id: &str) {
        let mut cache = self.session_cache.write().await;
        cache.remove(session_id);
    }

    /// Clear expired sessions based on TTL or other criteria
    pub async fn cleanup_expired_sessions(&self) {
        let mut cache = self.session_cache.write().await;
        cache.retain(|_, _| true); // Placeholder - would implement actual expiration logic
    }
}

pub struct Session {
    pub data: SessionData,
    provider_cache: RwLock<Option<Arc<dyn LlmProvider>>>,
    cached_model: RwLock<String>,
}

impl Session {
    pub fn new(data: SessionData) -> Self {
        let model = data.model.clone();
        Self {
            data,
            provider_cache: RwLock::new(None),
            cached_model: RwLock::new(model),
        }
    }

    pub async fn get_provider(&self) -> TalosResult<Arc<dyn LlmProvider>> {
        let cache = self.provider_cache.read().await;
        let cached_model = self.cached_model.read().await;

        if let Some(provider) = cache.as_ref() {
            if *cached_model == self.data.model {
                return Ok(provider.clone());
            }
        }

        Err(talos_error::TalosError::InternalError(
            "Provider creation not implemented in SessionStore".to_string(),
        ))
    }

    pub async fn set_model(&mut self, model: String) {
        if self.data.model != model {
            self.data.model = model.clone();
            let mut cache = self.provider_cache.write().await;
            *cache = None;
            let mut cached = self.cached_model.write().await;
            *cached = model;
        }
    }

    pub async fn chat<F>(&self, content: &str, provider_factory: F) -> TalosResult<String>
    where
        F: FnOnce(&str) -> TalosResult<Arc<dyn LlmProvider>>,
    {
        let provider = self.get_or_create_provider(provider_factory).await?;
        let messages = self.build_messages(content);
        let request = talos_llm_provider::ChatRequest {
            messages,
            model: self.data.model.clone(),
            temperature: None,
            max_tokens: None,
            system: None,
            stop: None,
            top_p: None,
            tools: None,
            tool_choice: None,
            parallel_tool_calls: None,
        };
        let response = provider.chat(request).await?;
        Ok(response.content)
    }

    pub async fn get_cached_provider(&self) -> Option<Arc<dyn LlmProvider>> {
        let cache = self.provider_cache.read().await;
        cache.as_ref().map(|provider| provider.clone())
    }

    pub async fn get_or_create_provider<F>(
        &self,
        provider_factory: F,
    ) -> TalosResult<Arc<dyn LlmProvider>>
    where
        F: FnOnce(&str) -> TalosResult<Arc<dyn LlmProvider>>,
    {
        {
            let cache = self.provider_cache.read().await;
            let cached_model = self.cached_model.read().await;

            if let Some(provider) = cache.as_ref() {
                if *cached_model == self.data.model {
                    return Ok(provider.clone());
                }
            }
        }

        let provider = provider_factory(&self.data.model)?;

        {
            let mut cache = self.provider_cache.write().await;
            let mut cached_model = self.cached_model.write().await;
            *cache = Some(provider.clone());
            *cached_model = self.data.model.clone();
        }

        Ok(provider)
    }

    /// Refreshes the provided by recreating it if current model matches the given model
    pub async fn refresh_provider<F>(
        &self,
        current_model: &str,
        provider_factory: F,
    ) -> TalosResult<Arc<dyn LlmProvider>>
    where
        F: FnOnce(&str) -> TalosResult<Arc<dyn LlmProvider>>,
    {
        let provider = provider_factory(current_model)?;

        {
            let mut cache = self.provider_cache.write().await;
            let mut cached_model = self.cached_model.write().await;
            *cache = Some(provider.clone());
            *cached_model = current_model.to_string();
        }

        Ok(provider)
    }

    fn build_messages(&self, new_content: &str) -> Vec<ChatMessage> {
        let mut messages: Vec<ChatMessage> = self
            .data
            .conversation_history
            .iter()
            .map(|m| {
                let role = match m.role.as_str() {
                    "system" => Role::System,
                    "user" => Role::User,
                    "assistant" => Role::Assistant,
                    "tool" => Role::Tool,
                    _ => Role::User, // default to user for unknown roles
                };
                ChatMessage {
                    role,
                    content: m.content.clone(),
                    name: None,
                    tool_call_id: None,
                    tool_calls: None,
                }
            })
            .collect();

        messages.push(ChatMessage::user(new_content));

        messages
    }

    pub fn add_message(&mut self, role: &str, content: &str) {
        self.data.conversation_history.push(Message {
            role: role.to_string(),
            content: content.to_string(),
            timestamp: chrono::Utc::now().timestamp(),
        });
    }

    /// Caches the LLM provider for this session
    pub async fn cache_provider(&self, provider: Arc<dyn LlmProvider>) {
        let mut cache = self.provider_cache.write().await;
        *cache = Some(provider);
    }

    /// Clears the cached provider, forcing a new provider creation on next access
    pub async fn invalidate_provider_cache(&self) {
        let mut cache = self.provider_cache.write().await;
        *cache = None;
    }
}

#[async_trait]
pub trait SessionStore: Send + Sync {
    async fn set_session_data(&self, user_id: &str, channel: &str, data: &str) -> TalosResult<()>;
    async fn get_session_data(&self, user_id: &str, channel: &str) -> TalosResult<Option<String>>;
    async fn clear_session(&self, user_id: &str, channel: &str) -> TalosResult<()>;
    async fn has_session(&self, user_id: &str, channel: &str) -> TalosResult<bool>;
    async fn add_message_to_session(
        &self,
        user_id: &str,
        channel: &str,
        message: &Message,
    ) -> TalosResult<()>;
    async fn get_conversation_history(
        &self,
        user_id: &str,
        channel: &str,
        limit: usize,
    ) -> TalosResult<Vec<Message>>;

    async fn get_or_create_default(
        &self,
        user_id: &str,
        default_model: &str,
    ) -> TalosResult<Arc<RwLock<Session>>>;
    async fn get_session_by_id(
        &self,
        session_id: &str,
    ) -> TalosResult<Option<Arc<RwLock<Session>>>>;

    async fn create_session(
        &self,
        context: &CanonicalContext,
        agent_id: &str,
        default_model: &str,
        requested_model: Option<&str>,
        metadata: Option<Value>,
    ) -> TalosResult<SessionRecord>;

    async fn resolve_or_create_session(
        &self,
        context: &CanonicalContext,
        agent_id: &str,
        default_model: &str,
        requested_model: Option<&str>,
        metadata: Option<Value>,
    ) -> TalosResult<SessionRecord>;

    async fn get_session_record(&self, session_id: &str) -> TalosResult<Option<SessionRecord>>;

    async fn list_session_records(&self, query: &SessionQuery) -> TalosResult<Vec<SessionRecord>>;

    async fn patch_session_record(
        &self,
        session_id: &str,
        patch: &SessionPatch,
    ) -> TalosResult<Option<SessionRecord>>;

    async fn append_message_by_session_id(
        &self,
        session_id: &str,
        role: &str,
        content: &str,
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

    async fn get_session_key(&self, user_id: &str, channel: &str) -> TalosResult<String> {
        Ok(format!("{}_{}", user_id, channel))
    }

    fn stable_session_key(context: &CanonicalContext, agent_id: &str, dm_scope: &str) -> String {
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
                agent_id, context.provider, context.channel_id, context.user_id
            ),
            _ => format!(
                "agent:{}:provider:{}:channel:{}:user:{}:thread:{}",
                agent_id, context.provider, context.channel_id, context.user_id, thread
            ),
        }
    }

    fn resolve_dm_scope(metadata: Option<&Value>) -> String {
        metadata
            .and_then(|v| v.get("dm_scope"))
            .and_then(|v| v.as_str())
            .unwrap_or("per_channel_peer_topic")
            .to_string()
    }

    fn new_session_id() -> String {
        uuid::Uuid::now_v7().to_string()
    }

    async fn fetch_session_row(&self, session_key: &str) -> TalosResult<Option<Row>> {
        let sql = "SELECT session_key, user_id, channel, thread_id, created_at, last_activity, metadata FROM sessions WHERE session_key = ?";
        let rows = self.storage.query(sql, &[session_key]).await?;
        Ok(rows.into_iter().next())
    }

    fn parse_session_record(row: &Row) -> SessionRecord {
        let metadata_value = row
            .get_string("metadata")
            .and_then(|m| serde_json::from_str::<Value>(&m).ok())
            .unwrap_or_else(|| Value::Object(serde_json::Map::new()));

        let context = metadata_value
            .get("context")
            .cloned()
            .and_then(|v| serde_json::from_value::<CanonicalContext>(v).ok())
            .unwrap_or_else(|| CanonicalContext {
                source_type: "api".to_string(),
                provider: "unknown".to_string(),
                channel_id: row.get_string("channel").unwrap_or_default(),
                user_id: row.get_string("user_id").unwrap_or_default(),
                thread_id: row.get_string("thread_id"),
                message_id: None,
                trace_id: None,
            });

        let model = metadata_value
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("default")
            .to_string();
        let message_count = metadata_value
            .get("message_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

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
            metadata: metadata_value.get("custom").cloned(),
        }
    }

    async fn load_session_data(&self, session_key: &str) -> TalosResult<SessionData> {
        let sql = "SELECT user_id, channel, metadata FROM sessions WHERE session_key = ?";
        let params = &[session_key];
        let rows = self.storage.query(sql, params).await?;

        if let Some(row) = rows.first() {
            let user_id = row.get_string("user_id").unwrap_or_default();
            let channel = row.get_string("channel").unwrap_or_default();
            let metadata: std::collections::HashMap<String, Value> = row
                .get_string("metadata")
                .and_then(|m| serde_json::from_str(&m).ok())
                .unwrap_or_default();

            let history_sql = "SELECT role, content, timestamp FROM messages WHERE session_key = ? ORDER BY timestamp ASC";
            let history_rows = self.storage.query(history_sql, params).await?;
            let mut conversation_history = Vec::new();

            for row in history_rows {
                if let (Some(role), Some(content)) =
                    (row.get_string("role"), row.get_string("content"))
                {
                    if let Some(timestamp) = row.get_i64("timestamp") {
                        conversation_history.push(Message {
                            role,
                            content,
                            timestamp,
                        });
                    }
                }
            }

            Ok(SessionData {
                id: session_key.to_string(),
                user_id,
                channel,
                model: metadata
                    .get("model")
                    .and_then(|v| v.as_str())
                    .unwrap_or("default")
                    .to_string(),
                conversation_history,
                metadata,
            })
        } else {
            Ok(SessionData {
                id: session_key.to_string(),
                user_id: String::new(),
                channel: String::new(),
                model: "default".to_string(),
                conversation_history: Vec::new(),
                metadata: std::collections::HashMap::new(),
            })
        }
    }
}

#[async_trait]
impl SessionStore for DatabaseSessionStore {
    async fn set_session_data(&self, user_id: &str, channel: &str, data: &str) -> TalosResult<()> {
        let session_key = self.get_session_key(user_id, channel).await?;
        let now = chrono::Utc::now().timestamp_millis();

        // Check if session already exists
        if self.has_session(user_id, channel).await? {
            // Update existing session
            let update_data = Row::new(
                vec!["last_activity".to_string(), "metadata".to_string()],
                vec![
                    serde_json::Value::Number(now.into()),
                    serde_json::Value::String(data.to_string()),
                ],
            );

            let where_clause = "session_key = ?";
            let params = &[session_key.as_str()];
            self.storage
                .update("sessions", &update_data, where_clause, params)
                .await?;
        } else {
            // Insert new session data
            let session_row = Row::new(
                vec![
                    "session_key".to_string(),
                    "user_id".to_string(),
                    "channel".to_string(),
                    "scope".to_string(),
                    "peer_id".to_string(),
                    "created_at".to_string(),
                    "last_activity".to_string(),
                    "agent_id".to_string(),
                    "account_id".to_string(),
                    "thread_id".to_string(),
                    "metadata".to_string(),
                ],
                vec![
                    serde_json::Value::String(session_key.clone()),
                    serde_json::Value::String(user_id.to_string()),
                    serde_json::Value::String(channel.to_string()),
                    serde_json::Value::String("default".to_string()),
                    serde_json::Value::String(user_id.to_string()),
                    serde_json::Value::Number(now.into()),
                    serde_json::Value::Number(now.into()),
                    serde_json::Value::String("default".to_string()),
                    serde_json::Value::Null,
                    serde_json::Value::Null,
                    serde_json::Value::String(data.to_string()),
                ],
            );

            self.storage.insert("sessions", &session_row).await?;
        }
        Ok(())
    }

    async fn get_session_data(&self, user_id: &str, channel: &str) -> TalosResult<Option<String>> {
        let session_key = self.get_session_key(user_id, channel).await?;
        let sql = "SELECT metadata FROM sessions WHERE session_key = ?";
        let params = &[session_key.as_str()];
        let rows = self.storage.query(sql, params).await?;

        if let Some(row) = rows.first() {
            if let Some(metadata) = row.get_string("metadata") {
                Ok(Some(metadata))
            } else {
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }

    async fn clear_session(&self, user_id: &str, channel: &str) -> TalosResult<()> {
        let session_key = self.get_session_key(user_id, channel).await?;
        let where_clause = "session_key = ?";
        let params = &[session_key.as_str()];
        self.storage
            .delete("sessions", where_clause, params)
            .await?;
        Ok(())
    }

    async fn has_session(&self, user_id: &str, channel: &str) -> TalosResult<bool> {
        let session_key = self.get_session_key(user_id, channel).await?;
        let sql = "SELECT COUNT(*) as count FROM sessions WHERE session_key = ?";
        let params = &[session_key.as_str()];
        let rows = self.storage.query(sql, params).await?;

        if let Some(row) = rows.first() {
            if let Some(count) = row.get_i64("count") {
                Ok(count > 0)
            } else {
                Ok(false)
            }
        } else {
            Ok(false)
        }
    }

    async fn add_message_to_session(
        &self,
        user_id: &str,
        channel: &str,
        message: &Message,
    ) -> TalosResult<()> {
        let session_key = self.get_session_key(user_id, channel).await?;

        // First, ensure session exists using the same logic as set_session_data
        if !self.has_session(user_id, channel).await? {
            // Create a default session - just the basic structure without metadata
            let now = chrono::Utc::now().timestamp_millis();
            let session_row = Row::new(
                vec![
                    "session_key".to_string(),
                    "user_id".to_string(),
                    "channel".to_string(),
                    "scope".to_string(),
                    "peer_id".to_string(),
                    "created_at".to_string(),
                    "last_activity".to_string(),
                    "agent_id".to_string(),
                    "account_id".to_string(),
                    "thread_id".to_string(),
                    "metadata".to_string(),
                ],
                vec![
                    serde_json::Value::String(session_key.clone()),
                    serde_json::Value::String(user_id.to_string()),
                    serde_json::Value::String(channel.to_string()),
                    serde_json::Value::String("default".to_string()),
                    serde_json::Value::String(user_id.to_string()),
                    serde_json::Value::Number(now.into()),
                    serde_json::Value::Number(now.into()),
                    serde_json::Value::String("default".to_string()),
                    serde_json::Value::Null,
                    serde_json::Value::Null,
                    serde_json::Value::String("{}".to_string()),
                ],
            );

            self.storage.insert("sessions", &session_row).await?;
        }

        // Insert message into messages table
        let message_row = Row::new(
            vec![
                "session_key".to_string(),
                "role".to_string(),
                "content".to_string(),
                "timestamp".to_string(),
            ],
            vec![
                serde_json::Value::String(session_key),
                serde_json::Value::String(message.role.clone()),
                serde_json::Value::String(message.content.clone()),
                serde_json::Value::Number(message.timestamp.into()),
            ],
        );

        self.storage.insert("messages", &message_row).await?;
        Ok(())
    }

    async fn get_conversation_history(
        &self,
        user_id: &str,
        channel: &str,
        limit: usize,
    ) -> TalosResult<Vec<Message>> {
        let session_key = self.get_session_key(user_id, channel).await?;
        let sql = format!("SELECT role, content, timestamp FROM messages WHERE session_key = ? ORDER BY timestamp DESC LIMIT {}", limit);
        let params = &[session_key.as_str()];
        let rows = self.storage.query(&sql, params).await?;

        let mut messages = Vec::new();
        for row in rows {
            if let (Some(role), Some(content)) = (row.get_string("role"), row.get_string("content"))
            {
                if let Some(timestamp) = row.get_i64("timestamp") {
                    messages.push(Message {
                        role,
                        content,
                        timestamp,
                    });
                }
            }
        }

        // Reverse to return in chronological order (oldest first)
        messages.reverse();
        Ok(messages)
    }

    async fn get_or_create_default(
        &self,
        user_id: &str,
        default_model: &str,
    ) -> TalosResult<Arc<RwLock<Session>>> {
        let session_key = self.get_session_key(user_id, "default").await?;

        if !self.has_session(user_id, "default").await? {
            // Create new session
            let now = chrono::Utc::now().timestamp_millis();
            let session_row = Row::new(
                vec![
                    "session_key".to_string(),
                    "user_id".to_string(),
                    "channel".to_string(),
                    "scope".to_string(),
                    "peer_id".to_string(),
                    "created_at".to_string(),
                    "last_activity".to_string(),
                    "agent_id".to_string(),
                    "account_id".to_string(),
                    "thread_id".to_string(),
                    "metadata".to_string(),
                ],
                vec![
                    serde_json::Value::String(session_key.clone()),
                    serde_json::Value::String(user_id.to_string()),
                    serde_json::Value::String("default".to_string()),
                    serde_json::Value::String("default".to_string()),
                    serde_json::Value::String(user_id.to_string()),
                    serde_json::Value::Number(now.into()),
                    serde_json::Value::Number(now.into()),
                    serde_json::Value::String("default".to_string()),
                    serde_json::Value::Null,
                    serde_json::Value::Null,
                    serde_json::Value::String("{}".to_string()),
                ],
            );
            self.storage.insert("sessions", &session_row).await?;
        }

        // Load session data from database
        let session_data = self.load_session_data(&session_key).await?;
        let mut session = Session::new(session_data);
        session.data.model = default_model.to_string();

        Ok(Arc::new(RwLock::new(session)))
    }

    async fn get_session_by_id(
        &self,
        session_id: &str,
    ) -> TalosResult<Option<Arc<RwLock<Session>>>> {
        let session_data = self.load_session_data(session_id).await?;
        let session = Session::new(session_data);
        Ok(Some(Arc::new(RwLock::new(session))))
    }

    async fn create_session(
        &self,
        context: &CanonicalContext,
        agent_id: &str,
        default_model: &str,
        requested_model: Option<&str>,
        metadata: Option<Value>,
    ) -> TalosResult<SessionRecord> {
        let session_id = Self::new_session_id();
        let now = chrono::Utc::now().timestamp_millis();
        let model = requested_model.unwrap_or(default_model).to_string();

        let record = SessionRecord {
            id: session_id.clone(),
            context: context.clone(),
            model: model.clone(),
            created_at: now.try_into().unwrap_or_default(),
            last_activity: now.try_into().unwrap_or_default(),
            message_count: 0,
            metadata: metadata.clone(),
        };

        let mut metadata_map = serde_json::Map::new();
        metadata_map.insert(
            "context".to_string(),
            serde_json::to_value(context)
                .map_err(|e| talos_error::TalosError::SerializationError(e.to_string()))?,
        );
        metadata_map.insert("model".to_string(), Value::String(model));
        metadata_map.insert(
            "message_count".to_string(),
            Value::Number(serde_json::Number::from(0u64)),
        );
        metadata_map.insert("custom".to_string(), metadata.unwrap_or(Value::Null));

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
                "metadata".to_string(),
            ],
            vec![
                Value::String(session_id),
                Value::String(context.user_id.clone()),
                Value::String(context.channel_id.clone()),
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
                Value::String(Value::Object(metadata_map).to_string()),
            ],
        );
        self.storage.insert("sessions", &row).await?;

        Ok(record)
    }

    async fn resolve_or_create_session(
        &self,
        context: &CanonicalContext,
        agent_id: &str,
        default_model: &str,
        requested_model: Option<&str>,
        metadata: Option<Value>,
    ) -> TalosResult<SessionRecord> {
        let dm_scope = Self::resolve_dm_scope(metadata.as_ref());
        let session_key = Self::stable_session_key(context, agent_id, &dm_scope);
        let now = chrono::Utc::now().timestamp_millis();

        if let Some(existing) = self.fetch_session_row(&session_key).await? {
            let mut record = Self::parse_session_record(&existing);
            if let Some(model) = requested_model {
                record.model = model.to_string();
            }
            if let Some(custom_metadata) = metadata {
                record.metadata = Some(custom_metadata);
            }
            record.last_activity = now.try_into().unwrap_or_default();

            let mut metadata_map = serde_json::Map::new();
            metadata_map.insert(
                "context".to_string(),
                serde_json::to_value(&record.context)
                    .map_err(|e| talos_error::TalosError::SerializationError(e.to_string()))?,
            );
            metadata_map.insert("model".to_string(), Value::String(record.model.clone()));
            metadata_map.insert(
                "message_count".to_string(),
                Value::Number(serde_json::Number::from(record.message_count)),
            );
            metadata_map.insert(
                "custom".to_string(),
                record.metadata.clone().unwrap_or(Value::Null),
            );

            let update_row = Row::new(
                vec!["last_activity".to_string(), "metadata".to_string()],
                vec![
                    Value::Number(now.into()),
                    Value::String(Value::Object(metadata_map).to_string()),
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
        let record = SessionRecord {
            id: session_key.clone(),
            context: context.clone(),
            model: model.clone(),
            created_at: now.try_into().unwrap_or_default(),
            last_activity: now.try_into().unwrap_or_default(),
            message_count: 0,
            metadata: metadata.clone(),
        };

        let mut metadata_map = serde_json::Map::new();
        metadata_map.insert(
            "context".to_string(),
            serde_json::to_value(context)
                .map_err(|e| talos_error::TalosError::SerializationError(e.to_string()))?,
        );
        metadata_map.insert("model".to_string(), Value::String(model));
        metadata_map.insert(
            "message_count".to_string(),
            Value::Number(serde_json::Number::from(0u64)),
        );
        metadata_map.insert("custom".to_string(), metadata.unwrap_or(Value::Null));

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
                "metadata".to_string(),
            ],
            vec![
                Value::String(session_key),
                Value::String(context.user_id.clone()),
                Value::String(context.channel_id.clone()),
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
                Value::String(Value::Object(metadata_map).to_string()),
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
        let sql = "SELECT session_key, user_id, channel, thread_id, created_at, last_activity, metadata FROM sessions ORDER BY last_activity DESC";
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
                .is_some_and(|v| &record.context.channel_id != v)
            {
                continue;
            }
            if query
                .provider
                .as_ref()
                .is_some_and(|v| &record.context.provider != v)
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
    ) -> TalosResult<Option<SessionRecord>> {
        let Some(row) = self.fetch_session_row(session_id).await? else {
            return Ok(None);
        };
        let mut record = Self::parse_session_record(&row);
        if let Some(model) = &patch.model {
            record.model = model.clone();
        }
        if let Some(metadata) = &patch.metadata {
            record.metadata = Some(metadata.clone());
        }
        record.last_activity = chrono::Utc::now()
            .timestamp_millis()
            .try_into()
            .unwrap_or_default();

        let mut metadata_map = serde_json::Map::new();
        metadata_map.insert(
            "context".to_string(),
            serde_json::to_value(&record.context)
                .map_err(|e| talos_error::TalosError::SerializationError(e.to_string()))?,
        );
        metadata_map.insert("model".to_string(), Value::String(record.model.clone()));
        metadata_map.insert(
            "message_count".to_string(),
            Value::Number(serde_json::Number::from(record.message_count)),
        );
        metadata_map.insert(
            "custom".to_string(),
            record.metadata.clone().unwrap_or(Value::Null),
        );

        let update_row = Row::new(
            vec!["last_activity".to_string(), "metadata".to_string()],
            vec![
                Value::Number(serde_json::Number::from(record.last_activity)),
                Value::String(Value::Object(metadata_map).to_string()),
            ],
        );
        self.storage
            .update("sessions", &update_row, "session_key = ?", &[session_id])
            .await?;
        Ok(Some(record))
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

        if let Some(mut record) = self.get_session_record(session_id).await? {
            record.message_count += 1;
            record.last_activity = now.try_into().unwrap_or_default();

            let mut metadata_map = serde_json::Map::new();
            metadata_map.insert(
                "context".to_string(),
                serde_json::to_value(&record.context)
                    .map_err(|e| talos_error::TalosError::SerializationError(e.to_string()))?,
            );
            metadata_map.insert("model".to_string(), Value::String(record.model));
            metadata_map.insert(
                "message_count".to_string(),
                Value::Number(serde_json::Number::from(record.message_count)),
            );
            metadata_map.insert("custom".to_string(), record.metadata.unwrap_or(Value::Null));

            let update_row = Row::new(
                vec!["last_activity".to_string(), "metadata".to_string()],
                vec![
                    Value::Number(now.into()),
                    Value::String(Value::Object(metadata_map).to_string()),
                ],
            );
            self.storage
                .update("sessions", &update_row, "session_key = ?", &[session_id])
                .await?;
        }

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
    async fn set_session_data(
        &self,
        _user_id: &str,
        _channel: &str,
        _data: &str,
    ) -> TalosResult<()> {
        Ok(())
    }

    async fn get_session_data(
        &self,
        _user_id: &str,
        _channel: &str,
    ) -> TalosResult<Option<String>> {
        Ok(Some("test_data".to_string()))
    }

    async fn clear_session(&self, _user_id: &str, _channel: &str) -> TalosResult<()> {
        Ok(())
    }

    async fn has_session(&self, _user_id: &str, _channel: &str) -> TalosResult<bool> {
        Ok(true)
    }

    async fn add_message_to_session(
        &self,
        _user_id: &str,
        _channel: &str,
        _message: &Message,
    ) -> TalosResult<()> {
        Ok(())
    }

    async fn get_conversation_history(
        &self,
        _user_id: &str,
        _channel: &str,
        _limit: usize,
    ) -> TalosResult<Vec<Message>> {
        Ok(vec![Message {
            role: "assistant".to_string(),
            content: "Hello! This is a mock response.".to_string(),
            timestamp: chrono::Utc::now().timestamp(),
        }])
    }

    async fn get_or_create_default(
        &self,
        user_id: &str,
        default_model: &str,
    ) -> TalosResult<Arc<RwLock<Session>>> {
        let session_data = SessionData {
            id: format!("{}_default", user_id),
            user_id: user_id.to_string(),
            channel: "default".to_string(),
            model: default_model.to_string(),
            conversation_history: Vec::new(),
            metadata: std::collections::HashMap::new(),
        };
        Ok(Arc::new(RwLock::new(Session::new(session_data))))
    }

    async fn get_session_by_id(
        &self,
        session_id: &str,
    ) -> TalosResult<Option<Arc<RwLock<Session>>>> {
        let session_data = SessionData {
            id: session_id.to_string(),
            user_id: "mock".to_string(),
            channel: "default".to_string(),
            model: "default".to_string(),
            conversation_history: Vec::new(),
            metadata: std::collections::HashMap::new(),
        };
        Ok(Some(Arc::new(RwLock::new(Session::new(session_data)))))
    }

    async fn create_session(
        &self,
        context: &CanonicalContext,
        _agent_id: &str,
        default_model: &str,
        requested_model: Option<&str>,
        metadata: Option<Value>,
    ) -> TalosResult<SessionRecord> {
        Ok(SessionRecord {
            id: DatabaseSessionStore::new_session_id(),
            context: context.clone(),
            model: requested_model.unwrap_or(default_model).to_string(),
            created_at: 0,
            last_activity: 0,
            message_count: 0,
            metadata,
        })
    }

    async fn resolve_or_create_session(
        &self,
        context: &CanonicalContext,
        agent_id: &str,
        default_model: &str,
        requested_model: Option<&str>,
        metadata: Option<Value>,
    ) -> TalosResult<SessionRecord> {
        let dm_scope = DatabaseSessionStore::resolve_dm_scope(metadata.as_ref());
        Ok(SessionRecord {
            id: DatabaseSessionStore::stable_session_key(context, agent_id, &dm_scope),
            context: context.clone(),
            model: requested_model.unwrap_or(default_model).to_string(),
            created_at: 0,
            last_activity: 0,
            message_count: 0,
            metadata,
        })
    }

    async fn get_session_record(&self, session_id: &str) -> TalosResult<Option<SessionRecord>> {
        Ok(Some(SessionRecord {
            id: session_id.to_string(),
            context: CanonicalContext {
                source_type: "api".to_string(),
                provider: "mock".to_string(),
                channel_id: "default".to_string(),
                user_id: "mock-user".to_string(),
                thread_id: None,
                message_id: None,
                trace_id: None,
            },
            model: "default".to_string(),
            created_at: 0,
            last_activity: 0,
            message_count: 0,
            metadata: None,
        }))
    }

    async fn list_session_records(&self, _query: &SessionQuery) -> TalosResult<Vec<SessionRecord>> {
        Ok(Vec::new())
    }

    async fn patch_session_record(
        &self,
        session_id: &str,
        patch: &SessionPatch,
    ) -> TalosResult<Option<SessionRecord>> {
        Ok(Some(SessionRecord {
            id: session_id.to_string(),
            context: CanonicalContext {
                source_type: "api".to_string(),
                provider: "mock".to_string(),
                channel_id: "default".to_string(),
                user_id: "mock-user".to_string(),
                thread_id: None,
                message_id: None,
                trace_id: None,
            },
            model: patch.model.clone().unwrap_or_else(|| "default".to_string()),
            created_at: 0,
            last_activity: 0,
            message_count: 0,
            metadata: patch.metadata.clone(),
        }))
    }

    async fn append_message_by_session_id(
        &self,
        _session_id: &str,
        _role: &str,
        _content: &str,
    ) -> TalosResult<()> {
        Ok(())
    }
}

#[cfg(test)]
mod scope_tests {
    use super::*;

    fn sample_context() -> CanonicalContext {
        CanonicalContext {
            source_type: "im".to_string(),
            provider: "telegram".to_string(),
            channel_id: "group-1".to_string(),
            user_id: "user-1".to_string(),
            thread_id: Some("topic-1".to_string()),
            message_id: None,
            trace_id: None,
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
}
