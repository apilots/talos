use std::path::PathBuf;
use std::sync::Arc;

use talos_error::{TalosError, TalosResult};
use talos_store::session::{SessionPatchAck, SessionRecord, SessionStore};
use talos_types::talos::{TalosContext, TalosUsage};
use talos_types::{
    OptionalCurrentDirectoryPatch, OptionalProjectIdPatch, SessionConf,
    SessionConfigOverridePolicy, SessionPatch, DEFAULT_SESSION_TITLE,
};

const API_SESSION_SCOPE: &str = "api";

#[derive(Clone)]
pub(crate) struct SessionRecordService {
    session_store: Arc<dyn SessionStore>,
}

impl SessionRecordService {
    pub(crate) fn new(session_store: Arc<dyn SessionStore>) -> Self {
        Self { session_store }
    }

    pub(crate) fn new_session_id() -> String {
        uuid::Uuid::now_v7().to_string()
    }

    pub(crate) async fn create_session_record(
        &self,
        session_id: Option<&str>,
        context: &TalosContext,
        agent_id: &str,
        default_model: &str,
        requested_model: Option<&str>,
        conf: Option<SessionConf>,
        _dm_scope: Option<String>,
        workspace_id: Option<String>,
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
            workspace_id,
            project_id,
            current_directory,
            now,
        )?;

        self.session_store
            .insert_session_record(&record, agent_id, API_SESSION_SCOPE)
            .await?;
        Ok(record)
    }

    pub(crate) async fn patch_session_record(
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

    pub(crate) async fn reset_session_record(
        &self,
        session_id: &str,
    ) -> TalosResult<SessionPatchAck> {
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

    pub(crate) async fn update_session_title(
        &self,
        session_id: &str,
        title: &str,
    ) -> TalosResult<()> {
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

    pub(crate) async fn record_session_progress(
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
        workspace_id: Option<String>,
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
            workspace_id,
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
#[path = "record_service_tests.rs"]
mod tests;
