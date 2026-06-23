use std::path::PathBuf;
use std::sync::Arc;

use talos_error::{TalosError, TalosResult};
use talos_store::session::{SessionPatchAck, SessionQuery, SessionRecord, SessionStore};
use talos_types::talos::{TalosContext, TalosUsage};
use talos_types::{
    OptionalCurrentDirectoryPatch, OptionalProjectIdPatch, SessionConf,
    SessionConfigOverridePolicy, SessionPatch, DEFAULT_SESSION_TITLE,
};

use crate::session::scope;

const API_SESSION_SCOPE: &str = "api";

#[derive(Clone)]
pub struct SessionRecordService {
    session_store: Arc<dyn SessionStore>,
}

impl SessionRecordService {
    pub fn new(session_store: Arc<dyn SessionStore>) -> Self {
        Self { session_store }
    }

    pub fn new_session_id() -> String {
        uuid::Uuid::now_v7().to_string()
    }

    pub fn stable_session_key(context: &TalosContext, agent_id: &str, dm_scope: &str) -> String {
        scope::stable_session_key(context, agent_id, dm_scope)
    }

    pub async fn create_session_record(
        &self,
        session_id: Option<&str>,
        context: &TalosContext,
        agent_id: &str,
        default_model: &str,
        requested_model: Option<&str>,
        conf: Option<SessionConf>,
        _dm_scope: Option<String>,
        project_id: Option<String>,
        current_directory: Option<PathBuf>,
    ) -> TalosResult<SessionRecord> {
        let session_id = session_id
            .map(str::to_string)
            .unwrap_or_else(Self::new_session_id);
        let now = chrono::Utc::now().timestamp_millis();
        let record = Self::new_session_record(
            session_id,
            context,
            default_model,
            requested_model,
            conf,
            project_id,
            current_directory,
            now,
        )?;

        self.session_store
            .insert_session_record(&record, agent_id, API_SESSION_SCOPE)
            .await?;
        Ok(record)
    }

    pub async fn create_session(
        &self,
        session_id: Option<&str>,
        context: &TalosContext,
        agent_id: &str,
        default_model: &str,
        requested_model: Option<&str>,
        conf: Option<SessionConf>,
        dm_scope: Option<String>,
        project_id: Option<String>,
        current_directory: Option<PathBuf>,
    ) -> TalosResult<SessionRecord> {
        self.create_session_record(
            session_id,
            context,
            agent_id,
            default_model,
            requested_model,
            conf,
            dm_scope,
            project_id,
            current_directory,
        )
        .await
    }

    pub async fn resolve_or_create_session_record(
        &self,
        context: &TalosContext,
        agent_id: &str,
        default_model: &str,
        requested_model: Option<&str>,
        dm_scope: Option<String>,
        project_id: Option<String>,
        current_directory: Option<PathBuf>,
    ) -> TalosResult<SessionRecord> {
        let dm_scope = scope::resolve_dm_scope(dm_scope.as_deref());
        let session_key = scope::stable_session_key(context, agent_id, &dm_scope);
        let now = chrono::Utc::now().timestamp_millis();

        if let Some(mut record) = self.session_store.get_session_record(&session_key).await? {
            if let Some(model) = requested_model {
                record.model = model.to_string();
                record.conf = SessionConfigOverridePolicy::merge_conf(
                    &record.conf,
                    &SessionConfigOverridePolicy::single_model_conf(model),
                );
            }
            record.last_activity = now.try_into().unwrap_or_default();
            self.session_store.update_session_record(&record).await?;
            return Ok(record);
        }

        let record = Self::new_session_record(
            session_key,
            context,
            default_model,
            requested_model,
            None,
            project_id,
            current_directory,
            now,
        )?;
        self.session_store
            .insert_session_record(&record, agent_id, &dm_scope)
            .await?;
        Ok(record)
    }

    pub async fn resolve_or_create_session(
        &self,
        context: &TalosContext,
        agent_id: &str,
        default_model: &str,
        requested_model: Option<&str>,
        dm_scope: Option<String>,
        project_id: Option<String>,
        current_directory: Option<PathBuf>,
    ) -> TalosResult<SessionRecord> {
        self.resolve_or_create_session_record(
            context,
            agent_id,
            default_model,
            requested_model,
            dm_scope,
            project_id,
            current_directory,
        )
        .await
    }

    pub async fn get_session_record(&self, session_id: &str) -> TalosResult<Option<SessionRecord>> {
        self.session_store.get_session_record(session_id).await
    }

    pub async fn list_session_records(
        &self,
        query: &SessionQuery,
    ) -> TalosResult<Vec<SessionRecord>> {
        self.session_store.list_session_records(query).await
    }

    pub async fn count_session_records(&self, query: &SessionQuery) -> TalosResult<usize> {
        self.session_store.count_session_records(query).await
    }

    pub async fn patch_session_record(
        &self,
        session_id: &str,
        patch: &SessionPatch,
    ) -> TalosResult<SessionPatchAck> {
        if let Some(conf) = patch.conf.as_ref() {
            SessionConfigOverridePolicy::validate_conf(conf)?;
        }
        let Some(mut record) = self.session_store.get_session_record(session_id).await? else {
            return Ok(SessionPatchAck { success: false });
        };

        let patch_conf = patch.conf.as_ref().cloned().unwrap_or_default();
        record.conf = SessionConfigOverridePolicy::merge_conf(&record.conf, &patch_conf);
        if let Some(model) = SessionConfigOverridePolicy::extract_agent_model(&record.conf)? {
            record.model = model;
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
        let success = self.session_store.update_session_record(&record).await?;
        Ok(SessionPatchAck { success })
    }

    pub async fn reset_session_record(&self, session_id: &str) -> TalosResult<SessionPatchAck> {
        let Some(mut record) = self.session_store.get_session_record(session_id).await? else {
            return Ok(SessionPatchAck { success: false });
        };
        record.title = DEFAULT_SESSION_TITLE.to_string();
        record.last_activity = chrono::Utc::now()
            .timestamp_millis()
            .try_into()
            .unwrap_or_default();
        record.message_count = 0;
        record.usage = TalosUsage::default();
        let success = self.session_store.update_session_record(&record).await?;
        Ok(SessionPatchAck { success })
    }

    pub async fn delete_session_record(&self, session_id: &str) -> TalosResult<SessionPatchAck> {
        let success = self.session_store.delete_session_record(session_id).await?;
        Ok(SessionPatchAck { success })
    }

    pub async fn update_session_title(&self, session_id: &str, title: &str) -> TalosResult<()> {
        let normalized = title.trim();
        if normalized.is_empty() {
            return Err(TalosError::ValidationError(
                "session title must not be empty".to_string(),
            ));
        }
        let Some(mut record) = self.session_store.get_session_record(session_id).await? else {
            return Err(TalosError::ResourceNotFound(format!(
                "session {}",
                session_id
            )));
        };
        record.title = normalized.to_string();
        if !self.session_store.update_session_record(&record).await? {
            return Err(TalosError::ResourceNotFound(format!(
                "session {}",
                session_id
            )));
        }
        Ok(())
    }

    pub async fn record_session_progress(
        &self,
        session_id: &str,
        message_delta: u64,
        usage: &TalosUsage,
        model: Option<&str>,
    ) -> TalosResult<()> {
        let Some(mut record) = self.session_store.get_session_record(session_id).await? else {
            return Err(TalosError::ResourceNotFound(format!(
                "session {}",
                session_id
            )));
        };
        record.last_activity = chrono::Utc::now()
            .timestamp_millis()
            .try_into()
            .unwrap_or_default();
        record.message_count = record.message_count.saturating_add(message_delta);
        record.usage.accumulate(usage);
        if let Some(model) = model {
            record.model = model.to_string();
        }
        self.session_store.update_session_record(&record).await?;
        Ok(())
    }

    fn new_session_record(
        session_id: String,
        context: &TalosContext,
        default_model: &str,
        requested_model: Option<&str>,
        conf: Option<SessionConf>,
        project_id: Option<String>,
        current_directory: Option<PathBuf>,
        now: i64,
    ) -> TalosResult<SessionRecord> {
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

        Ok(SessionRecord {
            id: session_id,
            title: DEFAULT_SESSION_TITLE.to_string(),
            user_id: context.user_id.clone(),
            context: context.clone(),
            model,
            created_at: now.try_into().unwrap_or_default(),
            last_activity: now.try_into().unwrap_or_default(),
            message_count: 0,
            usage: TalosUsage::default(),
            conf: effective_conf,
            project_id,
            current_directory,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use talos_storage::SqliteStorage;
    use talos_store::session::{DatabaseSessionStore, SessionStore};
    use talos_types::talos::{TalosContext, TalosProvider, TalosSourceType, TalosUsage};

    use super::*;

    fn context() -> TalosContext {
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

    async fn service() -> (SessionRecordService, Arc<DatabaseSessionStore>) {
        let storage = Arc::new(SqliteStorage::new(":memory:").await.unwrap());
        let store = Arc::new(DatabaseSessionStore::new(storage).await.unwrap());
        let session_store: Arc<dyn SessionStore> = store.clone();
        (SessionRecordService::new(session_store), store)
    }

    #[tokio::test]
    async fn resolve_or_create_session_uses_default_scope() {
        let (service, _store) = service().await;
        let session = service
            .resolve_or_create_session_record(
                &context(),
                "agent-a",
                "gpt-5.3",
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        assert_eq!(
            session.id,
            scope::stable_session_key(
                &context(),
                "agent-a",
                scope::SESSION_SCOPE_PER_CHANNEL_PEER_TOPIC,
            )
        );
    }

    #[tokio::test]
    async fn create_session_generates_record() {
        let (service, _store) = service().await;
        let session = service
            .create_session_record(
                Some("session-1"),
                &context(),
                "agent-a",
                "gpt-5.3",
                None,
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        assert_eq!(session.id, "session-1");
        assert_eq!(session.model, "gpt-5.3");
    }

    #[tokio::test]
    async fn record_session_progress_accumulates_usage() {
        let (service, store) = service().await;
        service
            .create_session_record(
                Some("session-1"),
                &context(),
                "agent-a",
                "gpt-5.3",
                None,
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();
        service
            .record_session_progress(
                "session-1",
                2,
                &TalosUsage {
                    input_tokens: 1,
                    output_tokens: 2,
                    total_tokens: 3,
                    ..TalosUsage::default()
                },
                Some("gpt-5.4"),
            )
            .await
            .unwrap();

        let record = store
            .get_session_record("session-1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(record.message_count, 2);
        assert_eq!(record.usage.total_tokens, 3);
        assert_eq!(record.model, "gpt-5.4");
    }
}
