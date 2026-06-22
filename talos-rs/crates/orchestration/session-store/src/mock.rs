use super::*;

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
        session_id: Option<&str>,
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
            id: session_id
                .map(str::to_string)
                .unwrap_or_else(DatabaseSessionStore::new_session_id),
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
}
