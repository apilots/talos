use std::path::PathBuf;
use std::sync::Arc;

use talos_auth::UserRole;
use talos_error::{TalosError, TalosResult};
use talos_types::{
    OptionalProjectIdPatch, ProjectPatch, ProjectRecord, SessionPatch, SessionPatchAck,
    SessionQuery, SessionRuntime, TalosSession,
};

use crate::session::SessionService;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Actor {
    pub user_id: String,
    pub username: Option<String>,
    pub role: UserRole,
    pub access_token: Option<String>,
}

impl Actor {
    pub fn new(user_id: impl Into<String>) -> Self {
        Self {
            user_id: user_id.into(),
            username: None,
            role: UserRole::User,
            access_token: None,
        }
    }

    pub fn regular(user_id: impl Into<String>) -> Self {
        Self::new(user_id)
    }

    pub fn with_access_token(mut self, token: impl Into<String>) -> Self {
        self.access_token = Some(token.into());
        self
    }

    pub fn with_username(mut self, username: Option<String>) -> Self {
        self.username = username;
        self
    }

    pub fn with_role(mut self, role: UserRole) -> Self {
        self.role = role;
        self
    }
}

#[derive(Clone)]
pub struct ResourceAccessService {
    session_runtime: Arc<dyn SessionRuntime>,
    session_records: Arc<SessionService>,
}

impl ResourceAccessService {
    pub fn with_records(
        session_runtime: Arc<dyn SessionRuntime>,
        session_records: Arc<SessionService>,
    ) -> Self {
        Self {
            session_runtime,
            session_records,
        }
    }

    pub async fn get_or_create_session(
        &self,
        actor: &Actor,
        session_id: &str,
        mut context: talos_types::talos::TalosContext,
        model: Option<String>,
        project_id: Option<String>,
        current_directory: Option<PathBuf>,
    ) -> TalosResult<TalosSession> {
        match self.get_session(actor, session_id).await {
            Ok(session) => return Ok(session),
            Err(TalosError::ResourceNotFound(_)) => {}
            Err(error) => return Err(error),
        }

        if context.project_root.is_some() {
            return Err(TalosError::ValidationError(
                "context.project_root is not supported".to_string(),
            ));
        }
        context.user_id = actor.user_id.clone();
        if let Some(project_id_ref) = project_id.as_deref() {
            self.ensure_project_access(actor, project_id_ref).await?;
        }
        let target_agent = self.resolve_target_agent(&context).await?;
        self.session_records
            .create_session(
                Some(session_id),
                context,
                &target_agent,
                model,
                None,
                project_id,
                current_directory,
            )
            .await
    }

    pub async fn list_sessions(
        &self,
        actor: &Actor,
        mut query: SessionQuery,
    ) -> TalosResult<Vec<TalosSession>> {
        query.user_id = Some(actor.user_id.clone());
        let sessions = self.session_records.list_sessions(query).await?;
        Ok(self.filter_sessions(actor, sessions))
    }

    pub async fn get_session(&self, actor: &Actor, session_id: &str) -> TalosResult<TalosSession> {
        let session = self.get_session_record(session_id).await?;
        self.ensure_session_visibility(actor, &session)?;
        Ok(session)
    }

    pub async fn patch_session(
        &self,
        actor: &Actor,
        session_id: &str,
        patch: SessionPatch,
    ) -> TalosResult<SessionPatchAck> {
        let _session = self.get_session(actor, session_id).await?;
        if let Some(OptionalProjectIdPatch::Set { project_id }) = patch.project_id.as_ref() {
            self.ensure_project_access(actor, project_id).await?;
        }
        let ack = self
            .session_records
            .patch_session(session_id, patch)
            .await?;
        if !ack.success {
            return Err(TalosError::ResourceNotFound(format!(
                "session {}",
                session_id
            )));
        }
        self.invalidate_cached_session(session_id).await?;
        Ok(ack)
    }

    pub async fn compact_session(
        &self,
        actor: &Actor,
        session_id: &str,
    ) -> TalosResult<TalosSession> {
        let _session = self.get_session(actor, session_id).await?;
        self.compact_runtime_session(session_id).await
    }

    pub async fn clear_session(
        &self,
        actor: &Actor,
        session_id: &str,
    ) -> TalosResult<TalosSession> {
        let _session = self.get_session(actor, session_id).await?;
        self.clear_runtime_session(session_id).await
    }

    pub async fn close_session(&self, actor: &Actor, session_id: &str) -> TalosResult<()> {
        let _session = self.get_session(actor, session_id).await?;
        self.close_runtime_session(session_id).await
    }

    pub async fn delete_session(
        &self,
        actor: &Actor,
        session_id: &str,
    ) -> TalosResult<SessionPatchAck> {
        let _session = self.get_session(actor, session_id).await?;
        self.delete_runtime_session(session_id).await
    }

    pub(crate) async fn discard_session_record(
        &self,
        actor: &Actor,
        session_id: &str,
    ) -> TalosResult<SessionPatchAck> {
        let _session = self.get_session(actor, session_id).await?;
        let ack = self.session_records.delete_session(session_id).await?;
        if !ack.success {
            return Err(TalosError::ResourceNotFound(format!(
                "session {}",
                session_id
            )));
        }
        self.invalidate_cached_session(session_id).await?;
        Ok(ack)
    }

    pub async fn cancel_active_turn(&self, actor: &Actor, session_id: &str) -> TalosResult<()> {
        let _session = self.get_session(actor, session_id).await?;
        self.cancel_runtime_turn(session_id).await
    }

    pub async fn submit_dynamic_tool_result(
        &self,
        actor: &Actor,
        session_id: &str,
        call_id: &str,
        tool_name: &str,
        output: String,
        success: bool,
    ) -> TalosResult<()> {
        let _session = self.get_session(actor, session_id).await?;
        self.submit_runtime_dynamic_tool_result(session_id, call_id, tool_name, output, success)
            .await
    }

    pub async fn list_projects(&self, actor: &Actor) -> TalosResult<Vec<ProjectRecord>> {
        let projects = self.session_records.list_projects().await?;
        Ok(self.filter_projects(actor, projects))
    }

    pub async fn get_project(&self, actor: &Actor, project_id: &str) -> TalosResult<ProjectRecord> {
        let project = self.get_project_record(project_id).await?;
        self.ensure_project_visibility(actor, &project)?;
        Ok(project)
    }

    pub async fn create_project(
        &self,
        actor: &Actor,
        name: String,
        directory: PathBuf,
    ) -> TalosResult<ProjectRecord> {
        self.session_records
            .create_project(name, directory, actor.user_id.clone())
            .await
    }

    pub async fn patch_project(
        &self,
        actor: &Actor,
        project_id: &str,
        patch: ProjectPatch,
    ) -> TalosResult<ProjectRecord> {
        let _project = self.get_project(actor, project_id).await?;
        let project = self
            .session_records
            .patch_project(project_id, patch)
            .await?
            .ok_or_else(|| TalosError::ResourceNotFound(format!("project {}", project_id)))?;
        self.invalidate_project_sessions(project_id).await?;
        Ok(project)
    }

    pub async fn delete_project(&self, actor: &Actor, project_id: &str) -> TalosResult<()> {
        let _project = self.get_project(actor, project_id).await?;
        self.invalidate_project_sessions(project_id).await?;
        self.session_records.delete_project(project_id).await
    }

    fn filter_sessions(&self, actor: &Actor, sessions: Vec<TalosSession>) -> Vec<TalosSession> {
        sessions
            .into_iter()
            .filter(|session| session.user_id == actor.user_id)
            .collect()
    }

    fn filter_projects(&self, actor: &Actor, projects: Vec<ProjectRecord>) -> Vec<ProjectRecord> {
        projects
            .into_iter()
            .filter(|project| project.user_id == actor.user_id)
            .collect()
    }

    fn ensure_session_visibility(&self, actor: &Actor, session: &TalosSession) -> TalosResult<()> {
        if session.user_id == actor.user_id {
            return Ok(());
        }
        Err(TalosError::ResourceNotFound(format!(
            "session {}",
            session.context.session_id
        )))
    }

    fn ensure_project_visibility(&self, actor: &Actor, project: &ProjectRecord) -> TalosResult<()> {
        if project.user_id == actor.user_id {
            return Ok(());
        }
        Err(TalosError::ResourceNotFound(format!(
            "project {}",
            project.project_id
        )))
    }

    async fn ensure_project_access(
        &self,
        actor: &Actor,
        project_id: &str,
    ) -> TalosResult<ProjectRecord> {
        let project = self.get_project_record(project_id).await?;
        self.ensure_project_visibility(actor, &project)?;
        Ok(project)
    }

    async fn get_session_record(&self, session_id: &str) -> TalosResult<TalosSession> {
        self.session_records
            .get_session(session_id)
            .await?
            .ok_or_else(|| TalosError::ResourceNotFound(format!("session {}", session_id)))
    }

    async fn get_project_record(&self, project_id: &str) -> TalosResult<ProjectRecord> {
        self.session_records
            .get_project(project_id)
            .await?
            .ok_or_else(|| TalosError::ResourceNotFound(format!("project {}", project_id)))
    }

    async fn invalidate_project_sessions(&self, project_id: &str) -> TalosResult<()> {
        let query = SessionQuery {
            project_id: Some(project_id.to_string()),
            ..SessionQuery::default()
        };
        let sessions = self.session_records.list_sessions(query).await?;
        for session in sessions {
            self.invalidate_cached_session(&session.context.session_id)
                .await?;
        }
        Ok(())
    }

    async fn resolve_target_agent(
        &self,
        context: &talos_types::talos::TalosContext,
    ) -> TalosResult<String> {
        self.session_runtime.resolve_target_agent(context).await
    }

    async fn invalidate_cached_session(&self, session_id: &str) -> TalosResult<()> {
        self.session_runtime
            .invalidate_cached_session(session_id)
            .await
    }

    async fn compact_runtime_session(&self, session_id: &str) -> TalosResult<TalosSession> {
        self.session_runtime.compact_session(session_id).await
    }

    async fn clear_runtime_session(&self, session_id: &str) -> TalosResult<TalosSession> {
        self.session_runtime.clear_session(session_id).await
    }

    async fn close_runtime_session(&self, session_id: &str) -> TalosResult<()> {
        self.session_runtime.close_session(session_id).await
    }

    async fn delete_runtime_session(&self, session_id: &str) -> TalosResult<SessionPatchAck> {
        self.session_runtime.delete_session(session_id).await
    }

    async fn cancel_runtime_turn(&self, session_id: &str) -> TalosResult<()> {
        self.session_runtime.cancel_active_turn(session_id).await
    }

    async fn submit_runtime_dynamic_tool_result(
        &self,
        session_id: &str,
        call_id: &str,
        tool_name: &str,
        output: String,
        success: bool,
    ) -> TalosResult<()> {
        self.session_runtime
            .submit_dynamic_tool_result(session_id, call_id, tool_name, output, success)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::{Actor, ResourceAccessService};
    use async_trait::async_trait;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use talos_auth::UserRole;
    use talos_error::TalosResult;
    use talos_session_store::SessionStore;
    use talos_types::talos::{TalosContext, TalosProvider, TalosSourceType, TalosUsage};
    use talos_types::{
        ProjectPatch, ProjectRecord, SessionConf, SessionPatch, SessionPatchAck, SessionQuery,
        SessionRecord, SessionRuntime, TalosConfig, TalosSession, DEFAULT_SESSION_TITLE,
    };

    #[derive(Default)]
    struct RecordingSessionStore {
        listed_queries: Mutex<Vec<SessionQuery>>,
        records: Mutex<Vec<SessionRecord>>,
        projects: Mutex<Vec<ProjectRecord>>,
    }

    impl RecordingSessionStore {
        fn with_records(records: Vec<SessionRecord>) -> Self {
            Self {
                listed_queries: Mutex::new(Vec::new()),
                records: Mutex::new(records),
                projects: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl SessionStore for RecordingSessionStore {
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
            let record = record_for(
                &context.user_id,
                session_id.unwrap_or("generated-session"),
                conf.unwrap_or_default(),
                project_id,
                current_directory,
            );
            let record = SessionRecord {
                context: context.clone(),
                model: requested_model.unwrap_or(default_model).to_string(),
                ..record
            };
            self.records.lock().unwrap().push(record.clone());
            Ok(record)
        }

        async fn resolve_or_create_session(
            &self,
            _context: &TalosContext,
            _agent_id: &str,
            _default_model: &str,
            _requested_model: Option<&str>,
            _dm_scope: Option<String>,
            _project_id: Option<String>,
            _current_directory: Option<PathBuf>,
        ) -> TalosResult<SessionRecord> {
            unreachable!("not used in this test")
        }

        async fn get_session_record(&self, session_id: &str) -> TalosResult<Option<SessionRecord>> {
            Ok(self
                .records
                .lock()
                .unwrap()
                .iter()
                .find(|record| record.id == session_id)
                .cloned())
        }

        async fn list_session_records(
            &self,
            query: &SessionQuery,
        ) -> TalosResult<Vec<SessionRecord>> {
            self.listed_queries.lock().unwrap().push(query.clone());
            let records = self.records.lock().unwrap();
            if !records.is_empty() {
                return Ok(records.clone());
            }
            Ok(vec![
                record_for(
                    query.user_id.as_deref().unwrap_or("member-user"),
                    "session-1",
                    SessionConf::default(),
                    None,
                    None,
                ),
                record_for(
                    "other-user",
                    "session-2",
                    SessionConf::default(),
                    None,
                    None,
                ),
            ])
        }

        async fn count_session_records(&self, _query: &SessionQuery) -> TalosResult<usize> {
            Ok(2)
        }

        async fn patch_session_record(
            &self,
            session_id: &str,
            _patch: &SessionPatch,
        ) -> TalosResult<SessionPatchAck> {
            Ok(SessionPatchAck {
                success: self
                    .records
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|record| record.id == session_id),
            })
        }

        async fn reset_session_record(&self, _session_id: &str) -> TalosResult<SessionPatchAck> {
            unreachable!("not used in this test")
        }

        async fn delete_session_record(&self, _session_id: &str) -> TalosResult<SessionPatchAck> {
            unreachable!("not used in this test")
        }

        async fn update_session_title(&self, _session_id: &str, _title: &str) -> TalosResult<()> {
            unreachable!("not used in this test")
        }

        async fn record_session_progress(
            &self,
            _session_id: &str,
            _message_delta: u64,
            _usage: &TalosUsage,
            _model: Option<&str>,
        ) -> TalosResult<()> {
            unreachable!("not used in this test")
        }

        async fn list_projects(&self) -> TalosResult<Vec<ProjectRecord>> {
            Ok(self.projects.lock().unwrap().clone())
        }

        async fn get_project(&self, project_id: &str) -> TalosResult<Option<ProjectRecord>> {
            Ok(self
                .projects
                .lock()
                .unwrap()
                .iter()
                .find(|project| project.project_id == project_id)
                .cloned())
        }

        async fn create_project(
            &self,
            _name: &str,
            _directory: &PathBuf,
        ) -> TalosResult<ProjectRecord> {
            unreachable!("not used in this test")
        }

        async fn create_project_for_owner(
            &self,
            _name: &str,
            _directory: &PathBuf,
            _user_id: &str,
        ) -> TalosResult<ProjectRecord> {
            unreachable!("not used in this test")
        }

        async fn patch_project(
            &self,
            _project_id: &str,
            _patch: &ProjectPatch,
        ) -> TalosResult<Option<ProjectRecord>> {
            unreachable!("not used in this test")
        }

        async fn delete_project(&self, _project_id: &str) -> TalosResult<()> {
            unreachable!("not used in this test")
        }
    }

    struct NoopSessionRuntime;

    #[async_trait]
    impl SessionRuntime for NoopSessionRuntime {
        async fn resolve_target_agent(&self, _context: &TalosContext) -> TalosResult<String> {
            Ok("default".to_string())
        }

        async fn compact_session(&self, _session_id: &str) -> TalosResult<TalosSession> {
            unreachable!("not used in this test")
        }

        async fn clear_session(&self, _session_id: &str) -> TalosResult<TalosSession> {
            unreachable!("not used in this test")
        }

        async fn close_session(&self, _session_id: &str) -> TalosResult<()> {
            unreachable!("not used in this test")
        }

        async fn delete_session(&self, _session_id: &str) -> TalosResult<SessionPatchAck> {
            unreachable!("not used in this test")
        }

        async fn cancel_active_turn(&self, _session_id: &str) -> TalosResult<()> {
            unreachable!("not used in this test")
        }

        async fn submit_dynamic_tool_result(
            &self,
            _session_id: &str,
            _call_id: &str,
            _tool_name: &str,
            _output: String,
            _success: bool,
        ) -> TalosResult<()> {
            unreachable!("not used in this test")
        }
    }

    fn record_for(
        user_id: &str,
        session_id: &str,
        conf: SessionConf,
        project_id: Option<String>,
        current_directory: Option<PathBuf>,
    ) -> SessionRecord {
        SessionRecord {
            id: session_id.to_string(),
            title: DEFAULT_SESSION_TITLE.to_string(),
            user_id: user_id.to_string(),
            context: TalosContext {
                source_type: TalosSourceType::Http,
                provider: TalosProvider::Http,
                user_id: user_id.to_string(),
                message_id: "message-1".to_string(),
                trace_id: "trace-1".to_string(),
                channel_id: Some("web".to_string()),
                thread_id: None,
                project_root: None,
                environment: None,
            },
            model: "default".to_string(),
            created_at: 0,
            last_activity: 0,
            message_count: 0,
            usage: TalosUsage::default(),
            conf,
            project_id,
            current_directory,
        }
    }

    #[tokio::test]
    async fn list_sessions_scopes_query_to_actor() {
        let store = Arc::new(RecordingSessionStore::default());
        let records = Arc::new(crate::session::SessionService::new(
            store.clone(),
            TalosConfig::default(),
        ));
        let access = ResourceAccessService::with_records(Arc::new(NoopSessionRuntime), records);

        let sessions = access
            .list_sessions(
                &Actor::regular("member-user"),
                SessionQuery {
                    user_id: Some("external-user".to_string()),
                    channel_id: None,
                    provider: None,
                    project_id: None,
                    limit: None,
                    offset: None,
                },
            )
            .await
            .expect("list_sessions should succeed");

        assert_eq!(sessions.len(), 1);
        assert_eq!(
            store.listed_queries.lock().unwrap()[0].user_id.as_deref(),
            Some("member-user")
        );
    }

    #[tokio::test]
    async fn regular_actor_cannot_read_or_patch_other_user_session() {
        let store = Arc::new(RecordingSessionStore::with_records(vec![record_for(
            "other-user",
            "session-1",
            SessionConf::default(),
            None,
            None,
        )]));
        let records = Arc::new(crate::session::SessionService::new(
            store,
            TalosConfig::default(),
        ));
        let access = ResourceAccessService::with_records(Arc::new(NoopSessionRuntime), records);

        let read_error = access
            .get_session(&Actor::regular("member-user"), "session-1")
            .await
            .expect_err("other user session should be hidden");
        assert!(matches!(
            read_error,
            talos_error::TalosError::ResourceNotFound(_)
        ));

        let patch_error = access
            .patch_session(
                &Actor::regular("member-user"),
                "session-1",
                SessionPatch::default(),
            )
            .await
            .expect_err("other user session should not be patched");
        assert!(matches!(
            patch_error,
            talos_error::TalosError::ResourceNotFound(_)
        ));
    }

    #[tokio::test]
    async fn regular_admin_route_is_still_scoped_to_actor() {
        let store = Arc::new(RecordingSessionStore::with_records(vec![
            record_for(
                "admin-user",
                "session-admin",
                SessionConf::default(),
                None,
                None,
            ),
            record_for(
                "other-user",
                "session-other",
                SessionConf::default(),
                None,
                None,
            ),
        ]));
        let records = Arc::new(crate::session::SessionService::new(
            store,
            TalosConfig::default(),
        ));
        let access = ResourceAccessService::with_records(Arc::new(NoopSessionRuntime), records);

        let sessions = access
            .list_sessions(&Actor::regular("admin-user"), SessionQuery::default())
            .await
            .expect("list should succeed");

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].context.session_id, "session-admin");
    }

    #[tokio::test]
    async fn admin_role_metadata_does_not_expand_resource_scope() {
        let store = Arc::new(RecordingSessionStore::with_records(vec![
            record_for(
                "admin-user",
                "session-admin",
                SessionConf::default(),
                None,
                None,
            ),
            record_for(
                "other-user",
                "session-other",
                SessionConf::default(),
                None,
                None,
            ),
        ]));
        let records = Arc::new(crate::session::SessionService::new(
            store,
            TalosConfig::default(),
        ));
        let access = ResourceAccessService::with_records(Arc::new(NoopSessionRuntime), records);
        let admin_actor = Actor::regular("admin-user").with_role(UserRole::Admin);

        let sessions = access
            .list_sessions(&admin_actor, SessionQuery::default())
            .await
            .expect("list should succeed");
        let read_error = access
            .get_session(&admin_actor, "session-other")
            .await
            .expect_err("admin role metadata should not reveal other user sessions");

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].context.session_id, "session-admin");
        assert!(matches!(
            read_error,
            talos_error::TalosError::ResourceNotFound(_)
        ));
    }

    #[tokio::test]
    async fn open_mode_actor_only_sees_anonymous_sessions() {
        let store = Arc::new(RecordingSessionStore::with_records(vec![
            record_for(
                "anonymous",
                "session-anonymous",
                SessionConf::default(),
                None,
                None,
            ),
            record_for(
                "member-user",
                "session-member",
                SessionConf::default(),
                None,
                None,
            ),
        ]));
        let records = Arc::new(crate::session::SessionService::new(
            store,
            TalosConfig::default(),
        ));
        let access = ResourceAccessService::with_records(Arc::new(NoopSessionRuntime), records);

        let sessions = access
            .list_sessions(&Actor::regular("anonymous"), SessionQuery::default())
            .await
            .expect("list should succeed");

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].context.session_id, "session-anonymous");
    }

    #[tokio::test]
    async fn creating_session_uses_actor_user_id() {
        let store = Arc::new(RecordingSessionStore::default());
        let records = Arc::new(crate::session::SessionService::new(
            store.clone(),
            TalosConfig::default(),
        ));
        let access = ResourceAccessService::with_records(Arc::new(NoopSessionRuntime), records);
        let context = TalosContext {
            source_type: TalosSourceType::Http,
            provider: TalosProvider::Http,
            user_id: "spoofed-user".to_string(),
            message_id: "message-1".to_string(),
            trace_id: "trace-1".to_string(),
            channel_id: None,
            thread_id: None,
            project_root: None,
            environment: None,
        };

        let session = access
            .get_or_create_session(
                &Actor::regular("member-user"),
                "session-created",
                context,
                None,
                None,
                None,
            )
            .await
            .expect("session should be created");

        assert_eq!(session.user_id, "member-user");
        assert_eq!(session.context.user_id, "member-user");
        assert_eq!(
            store.records.lock().unwrap()[0].context.user_id,
            "member-user"
        );
    }
}
