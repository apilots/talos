use std::sync::Arc;

use talos_auth::RequestIdentity;
use talos_error::{TalosError, TalosResult};
use talos_types::{
    OptionalProjectIdPatch, ProjectPatch, ProjectRecord, SessionPatch, SessionPatchAck,
    SessionQuery, SessionService, TalosSession,
};

pub(super) struct ResourceAccessService {
    session_service: Arc<dyn SessionService>,
}

impl ResourceAccessService {
    pub(super) fn new(session_service: Arc<dyn SessionService>) -> Self {
        Self { session_service }
    }

    pub(super) async fn create_session(
        &self,
        actor: &RequestIdentity,
        mut context: talos_types::talos::TalosContext,
        model: Option<String>,
        conf: Option<talos_types::SessionConf>,
        project_id: Option<String>,
        current_directory: Option<std::path::PathBuf>,
    ) -> TalosResult<TalosSession> {
        if !actor.bypasses_auth() {
            context.user_id = actor.user_id.clone();
        }
        if let Some(project_id_ref) = project_id.as_deref() {
            self.ensure_project_access(actor, project_id_ref).await?;
        }
        self.session_service
            .create_session(context, model, conf, project_id, current_directory)
            .await
    }

    pub(super) async fn list_sessions(
        &self,
        actor: &RequestIdentity,
        mut query: SessionQuery,
    ) -> TalosResult<Vec<TalosSession>> {
        if !actor.bypasses_auth() {
            query.user_id = Some(actor.user_id.clone());
        }
        let sessions = self.session_service.list_sessions(query).await?;
        Ok(self.filter_sessions(actor, sessions))
    }

    pub(super) async fn get_session(
        &self,
        actor: &RequestIdentity,
        session_id: &str,
    ) -> TalosResult<TalosSession> {
        let session = self
            .session_service
            .get_session(session_id)
            .await?
            .ok_or_else(|| TalosError::ResourceNotFound(format!("session {}", session_id)))?;
        self.ensure_session_visibility(actor, &session)?;
        Ok(session)
    }

    pub(super) async fn patch_session(
        &self,
        actor: &RequestIdentity,
        session_id: &str,
        patch: SessionPatch,
    ) -> TalosResult<SessionPatchAck> {
        let _session = self.get_session(actor, session_id).await?;
        if let Some(OptionalProjectIdPatch::Set { project_id }) = patch.project_id.as_ref() {
            self.ensure_project_access(actor, project_id).await?;
        }
        self.session_service.patch_session(session_id, patch).await
    }

    pub(super) async fn compact_session(
        &self,
        actor: &RequestIdentity,
        session_id: &str,
    ) -> TalosResult<TalosSession> {
        let _session = self.get_session(actor, session_id).await?;
        self.session_service.compact_session(session_id).await
    }

    pub(super) async fn clear_session(
        &self,
        actor: &RequestIdentity,
        session_id: &str,
    ) -> TalosResult<TalosSession> {
        let _session = self.get_session(actor, session_id).await?;
        self.session_service.clear_session(session_id).await
    }

    pub(super) async fn close_session(
        &self,
        actor: &RequestIdentity,
        session_id: &str,
    ) -> TalosResult<()> {
        let _session = self.get_session(actor, session_id).await?;
        self.session_service.close_session(session_id).await
    }

    pub(super) async fn delete_session(
        &self,
        actor: &RequestIdentity,
        session_id: &str,
    ) -> TalosResult<SessionPatchAck> {
        let _session = self.get_session(actor, session_id).await?;
        self.session_service.delete_session(session_id).await
    }

    pub(super) async fn delete_admin_session(
        &self,
        actor: &RequestIdentity,
        session_id: &str,
    ) -> TalosResult<SessionPatchAck> {
        self.require_admin(actor)?;
        self.session_service.delete_session(session_id).await
    }

    pub(super) async fn cancel_active_turn(
        &self,
        actor: &RequestIdentity,
        session_id: &str,
    ) -> TalosResult<()> {
        let _session = self.get_session(actor, session_id).await?;
        self.session_service.cancel_active_turn(session_id).await
    }

    pub(super) async fn submit_tool_approval(
        &self,
        actor: &RequestIdentity,
        session_id: &str,
        call_id: &str,
        tool_name: &str,
        approved: bool,
        reason: Option<String>,
    ) -> TalosResult<()> {
        let _session = self.get_session(actor, session_id).await?;
        self.session_service
            .submit_tool_approval(session_id, call_id, tool_name, approved, reason)
            .await
    }

    pub(super) async fn submit_dynamic_tool_result(
        &self,
        actor: &RequestIdentity,
        session_id: &str,
        call_id: &str,
        tool_name: &str,
        output: String,
        success: bool,
    ) -> TalosResult<()> {
        let _session = self.get_session(actor, session_id).await?;
        self.session_service
            .submit_dynamic_tool_result(session_id, call_id, tool_name, output, success)
            .await
    }

    pub(super) async fn get_session_turns(
        &self,
        actor: &RequestIdentity,
        session_id: &str,
    ) -> TalosResult<Vec<talos_types::talos::TalosMessageResponse>> {
        let _session = self.get_session(actor, session_id).await?;
        self.session_service.get_session_turns(session_id).await
    }

    pub(super) async fn list_projects(
        &self,
        actor: &RequestIdentity,
    ) -> TalosResult<Vec<ProjectRecord>> {
        let projects = self.session_service.list_projects().await?;
        Ok(self.filter_projects(actor, projects))
    }

    pub(super) async fn get_project(
        &self,
        actor: &RequestIdentity,
        project_id: &str,
    ) -> TalosResult<ProjectRecord> {
        let project = self
            .session_service
            .get_project(project_id)
            .await?
            .ok_or_else(|| TalosError::ResourceNotFound(format!("project {}", project_id)))?;
        self.ensure_project_visibility(actor, &project)?;
        Ok(project)
    }

    pub(super) async fn create_project(
        &self,
        actor: &RequestIdentity,
        name: String,
        directory: std::path::PathBuf,
    ) -> TalosResult<ProjectRecord> {
        self.session_service
            .create_project(name, directory, actor.user_id.clone())
            .await
    }

    pub(super) async fn patch_project(
        &self,
        actor: &RequestIdentity,
        project_id: &str,
        patch: ProjectPatch,
    ) -> TalosResult<ProjectRecord> {
        let _project = self.get_project(actor, project_id).await?;
        self.session_service
            .patch_project(project_id, patch)
            .await?
            .ok_or_else(|| TalosError::ResourceNotFound(format!("project {}", project_id)))
    }

    pub(super) async fn delete_project(
        &self,
        actor: &RequestIdentity,
        project_id: &str,
    ) -> TalosResult<()> {
        let _project = self.get_project(actor, project_id).await?;
        self.session_service.delete_project(project_id).await
    }

    pub(super) async fn list_admin_sessions(
        &self,
        actor: &RequestIdentity,
        query: SessionQuery,
    ) -> TalosResult<Vec<TalosSession>> {
        self.require_admin(actor)?;
        self.session_service.list_sessions(query).await
    }

    pub(super) async fn count_admin_sessions(
        &self,
        actor: &RequestIdentity,
        query: SessionQuery,
    ) -> TalosResult<usize> {
        self.require_admin(actor)?;
        self.session_service.count_sessions(query).await
    }

    pub(super) async fn list_admin_projects(
        &self,
        actor: &RequestIdentity,
    ) -> TalosResult<Vec<ProjectRecord>> {
        self.require_admin(actor)?;
        self.session_service.list_projects().await
    }

    fn filter_sessions(
        &self,
        actor: &RequestIdentity,
        sessions: Vec<TalosSession>,
    ) -> Vec<TalosSession> {
        if actor.bypasses_auth() {
            return sessions;
        }
        sessions
            .into_iter()
            .filter(|session| session.user_id == actor.user_id)
            .collect()
    }

    fn filter_projects(
        &self,
        actor: &RequestIdentity,
        projects: Vec<ProjectRecord>,
    ) -> Vec<ProjectRecord> {
        if actor.bypasses_auth() {
            return projects;
        }
        projects
            .into_iter()
            .filter(|project| project.user_id == actor.user_id)
            .collect()
    }

    fn ensure_session_visibility(
        &self,
        actor: &RequestIdentity,
        session: &TalosSession,
    ) -> TalosResult<()> {
        if actor.bypasses_auth() || session.user_id == actor.user_id {
            return Ok(());
        }
        Err(TalosError::ResourceNotFound(format!(
            "session {}",
            session.context.session_id
        )))
    }

    fn ensure_project_visibility(
        &self,
        actor: &RequestIdentity,
        project: &ProjectRecord,
    ) -> TalosResult<()> {
        if actor.bypasses_auth() || project.user_id == actor.user_id {
            return Ok(());
        }
        Err(TalosError::ResourceNotFound(format!(
            "project {}",
            project.project_id
        )))
    }

    async fn ensure_project_access(
        &self,
        actor: &RequestIdentity,
        project_id: &str,
    ) -> TalosResult<ProjectRecord> {
        let project = self
            .session_service
            .get_project(project_id)
            .await?
            .ok_or_else(|| TalosError::ResourceNotFound(format!("project {}", project_id)))?;
        self.ensure_project_visibility(actor, &project)?;
        Ok(project)
    }

    fn require_admin(&self, actor: &RequestIdentity) -> TalosResult<()> {
        if actor.has_full_access() {
            return Ok(());
        }
        Err(TalosError::AuthError("admin access required".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use talos_auth::UserRole;
    use talos_types::talos::{TalosContext, TalosProvider, TalosSourceType, TalosUsage};
    use talos_types::{
        ProjectPatch, ProjectRecord, SessionPatch, SessionPatchAck, SessionQuery, SessionService,
        TalosConfig, TalosSession,
    };

    use super::ResourceAccessService;
    use talos_auth::RequestIdentity;
    use talos_error::TalosResult;

    #[derive(Default)]
    struct RecordingSessionService {
        created_contexts: Mutex<Vec<TalosContext>>,
        listed_queries: Mutex<Vec<SessionQuery>>,
        counted_queries: Mutex<Vec<SessionQuery>>,
        deleted_sessions: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl SessionService for RecordingSessionService {
        async fn create_session(
            &self,
            context: TalosContext,
            _model: Option<String>,
            conf: Option<talos_types::SessionConf>,
            project_id: Option<String>,
            current_directory: Option<PathBuf>,
        ) -> TalosResult<TalosSession> {
            self.created_contexts.lock().unwrap().push(context.clone());
            Ok(TalosSession {
                title: "Untitled Session".to_string(),
                user_id: context.user_id.clone(),
                context: talos_types::SessionContext {
                    session_id: "session-1".to_string(),
                    workspace_id: None,
                    source_type: context.source_type,
                    provider: context.provider,
                    user_id: context.user_id,
                    message_id: context.message_id,
                    trace_id: context.trace_id,
                    channel_id: context.channel_id,
                    thread_id: context.thread_id,
                    project_id: project_id.clone(),
                    current_directory: current_directory.clone(),
                    project_root: context.project_root,
                    environment: context.environment,
                },
                conf: conf.unwrap_or_default(),
                project_id,
                current_directory,
                config: TalosConfig::default(),
                state: talos_types::SessionState::default(),
            })
        }

        async fn list_sessions(&self, query: SessionQuery) -> TalosResult<Vec<TalosSession>> {
            self.listed_queries.lock().unwrap().push(query.clone());
            Ok(vec![TalosSession {
                title: "Untitled Session".to_string(),
                user_id: query
                    .user_id
                    .clone()
                    .unwrap_or_else(|| "external-user".to_string()),
                context: talos_types::SessionContext {
                    session_id: "session-1".to_string(),
                    workspace_id: None,
                    source_type: TalosSourceType::Http,
                    provider: TalosProvider::Http,
                    user_id: query.user_id.unwrap_or_else(|| "external-user".to_string()),
                    message_id: "message-1".to_string(),
                    trace_id: "trace-1".to_string(),
                    channel_id: Some("web".to_string()),
                    thread_id: None,
                    project_id: None,
                    current_directory: None,
                    project_root: None,
                    environment: None,
                },
                conf: talos_types::SessionConf::default(),
                project_id: None,
                current_directory: None,
                config: TalosConfig::default(),
                state: talos_types::SessionState {
                    created_at: 0,
                    last_activity: 0,
                    message_count: 0,
                    usage: TalosUsage::default(),
                },
            }])
        }

        async fn count_sessions(&self, query: SessionQuery) -> TalosResult<usize> {
            self.counted_queries.lock().unwrap().push(query);
            Ok(42)
        }

        async fn get_session(&self, _session_id: &str) -> TalosResult<Option<TalosSession>> {
            Ok(None)
        }

        async fn patch_session(
            &self,
            _session_id: &str,
            _patch: SessionPatch,
        ) -> TalosResult<SessionPatchAck> {
            Ok(SessionPatchAck { success: true })
        }

        async fn compact_session(&self, _session_id: &str) -> TalosResult<TalosSession> {
            unreachable!("not used in tests")
        }

        async fn clear_session(&self, _session_id: &str) -> TalosResult<TalosSession> {
            unreachable!("not used in tests")
        }

        async fn close_session(&self, _session_id: &str) -> TalosResult<()> {
            Ok(())
        }

        async fn delete_session(&self, session_id: &str) -> TalosResult<SessionPatchAck> {
            self.deleted_sessions
                .lock()
                .unwrap()
                .push(session_id.to_string());
            Ok(SessionPatchAck { success: true })
        }

        async fn cancel_active_turn(&self, _session_id: &str) -> TalosResult<()> {
            Ok(())
        }

        async fn submit_tool_approval(
            &self,
            _session_id: &str,
            _call_id: &str,
            _tool_name: &str,
            _approved: bool,
            _reason: Option<String>,
        ) -> TalosResult<()> {
            Ok(())
        }

        async fn submit_dynamic_tool_result(
            &self,
            _session_id: &str,
            _call_id: &str,
            _tool_name: &str,
            _output: String,
            _success: bool,
        ) -> TalosResult<()> {
            Ok(())
        }

        async fn get_session_turns(
            &self,
            _session_id: &str,
        ) -> TalosResult<Vec<talos_types::talos::TalosMessageResponse>> {
            Ok(Vec::new())
        }

        async fn list_projects(&self) -> TalosResult<Vec<ProjectRecord>> {
            Ok(vec![
                ProjectRecord {
                    project_id: "project-1".to_string(),
                    name: "Project 1".to_string(),
                    user_id: "user-1".to_string(),
                    directory: PathBuf::from("/tmp/project-1"),
                    created_at: 0,
                    updated_at: 0,
                    session_count: 0,
                },
                ProjectRecord {
                    project_id: "project-2".to_string(),
                    name: "Project 2".to_string(),
                    user_id: "user-2".to_string(),
                    directory: PathBuf::from("/tmp/project-2"),
                    created_at: 0,
                    updated_at: 0,
                    session_count: 0,
                },
            ])
        }

        async fn get_project(&self, _project_id: &str) -> TalosResult<Option<ProjectRecord>> {
            Ok(None)
        }

        async fn create_project(
            &self,
            _name: String,
            _directory: PathBuf,
            _user_id: String,
        ) -> TalosResult<ProjectRecord> {
            unreachable!("not used in tests")
        }

        async fn patch_project(
            &self,
            _project_id: &str,
            _patch: ProjectPatch,
        ) -> TalosResult<Option<ProjectRecord>> {
            Ok(None)
        }

        async fn delete_project(&self, _project_id: &str) -> TalosResult<()> {
            Ok(())
        }
    }

    fn test_context(user_id: &str) -> TalosContext {
        TalosContext::new(
            TalosSourceType::Http,
            TalosProvider::Http,
            user_id,
            "msg-1",
            "trace-1",
        )
        .with_channel_id("web")
    }

    fn member_identity() -> RequestIdentity {
        RequestIdentity {
            user_id: "member-user".to_string(),
            username: Some("member".to_string()),
            role: UserRole::User,
            scopes: Vec::new(),
            auth_disabled: false,
        }
    }

    fn admin_identity() -> RequestIdentity {
        RequestIdentity::mock_admin("admin-user")
    }

    #[tokio::test]
    async fn create_session_rewrites_context_for_member_actor() {
        let service = Arc::new(RecordingSessionService::default());
        let access = ResourceAccessService::new(service.clone());

        let session = access
            .create_session(
                &member_identity(),
                test_context("external-user"),
                None,
                None,
                None,
                None,
            )
            .await
            .expect("create_session should succeed");

        assert_eq!(session.context.user_id, "member-user");
        assert_eq!(
            service.created_contexts.lock().unwrap()[0].user_id,
            "member-user"
        );
    }

    #[tokio::test]
    async fn create_session_rewrites_context_for_admin_actor_on_regular_routes() {
        let service = Arc::new(RecordingSessionService::default());
        let access = ResourceAccessService::new(service.clone());

        let session = access
            .create_session(
                &admin_identity(),
                test_context("external-user"),
                None,
                None,
                None,
                None,
            )
            .await
            .expect("create_session should succeed");

        assert_eq!(session.context.user_id, "admin-user");
        assert_eq!(
            service.created_contexts.lock().unwrap()[0].user_id,
            "admin-user"
        );
    }

    #[tokio::test]
    async fn list_sessions_scopes_owner_for_member_actor() {
        let service = Arc::new(RecordingSessionService::default());
        let access = ResourceAccessService::new(service.clone());

        let _ = access
            .list_sessions(
                &member_identity(),
                SessionQuery {
                    user_id: Some("external-user".to_string()),
                    channel_id: Some("web".to_string()),
                    provider: Some("http".to_string()),
                    project_id: None,
                    limit: None,
                    offset: None,
                },
            )
            .await
            .expect("list_sessions should succeed");

        assert_eq!(
            service.listed_queries.lock().unwrap()[0].user_id.as_deref(),
            Some("member-user")
        );
    }

    #[tokio::test]
    async fn list_sessions_scopes_owner_for_admin_actor_on_regular_routes() {
        let service = Arc::new(RecordingSessionService::default());
        let access = ResourceAccessService::new(service.clone());

        let _ = access
            .list_sessions(
                &admin_identity(),
                SessionQuery {
                    user_id: Some("external-user".to_string()),
                    channel_id: Some("web".to_string()),
                    provider: Some("http".to_string()),
                    project_id: None,
                    limit: None,
                    offset: None,
                },
            )
            .await
            .expect("list_sessions should succeed");

        assert_eq!(
            service.listed_queries.lock().unwrap()[0].user_id.as_deref(),
            Some("admin-user")
        );
    }

    #[tokio::test]
    async fn list_projects_scopes_admin_to_owned_resources_on_regular_routes() {
        let service = Arc::new(RecordingSessionService::default());
        let access = ResourceAccessService::new(service);

        let projects = access
            .list_projects(&admin_identity())
            .await
            .expect("list_projects should succeed");

        assert!(projects.is_empty());
    }

    #[tokio::test]
    async fn list_admin_sessions_keeps_admin_visibility() {
        let service = Arc::new(RecordingSessionService::default());
        let access = ResourceAccessService::new(service.clone());

        let _ = access
            .list_admin_sessions(
                &admin_identity(),
                SessionQuery {
                    user_id: Some("external-user".to_string()),
                    channel_id: Some("web".to_string()),
                    provider: Some("http".to_string()),
                    project_id: None,
                    limit: Some(20),
                    offset: Some(40),
                },
            )
            .await
            .expect("list_admin_sessions should succeed");

        assert_eq!(
            service.listed_queries.lock().unwrap()[0].user_id.as_deref(),
            Some("external-user")
        );
    }

    #[tokio::test]
    async fn count_admin_sessions_keeps_admin_visibility() {
        let service = Arc::new(RecordingSessionService::default());
        let access = ResourceAccessService::new(service.clone());

        let total = access
            .count_admin_sessions(
                &admin_identity(),
                SessionQuery {
                    user_id: Some("external-user".to_string()),
                    channel_id: Some("web".to_string()),
                    provider: Some("http".to_string()),
                    project_id: None,
                    limit: Some(20),
                    offset: Some(40),
                },
            )
            .await
            .expect("count_admin_sessions should succeed");

        assert_eq!(total, 42);
        assert_eq!(
            service.counted_queries.lock().unwrap()[0]
                .user_id
                .as_deref(),
            Some("external-user")
        );
    }

    #[tokio::test]
    async fn list_admin_projects_keeps_admin_visibility() {
        let service = Arc::new(RecordingSessionService::default());
        let access = ResourceAccessService::new(service);

        let projects = access
            .list_admin_projects(&admin_identity())
            .await
            .expect("list_admin_projects should succeed");

        assert_eq!(projects.len(), 2);
    }

    #[tokio::test]
    async fn delete_admin_session_bypasses_regular_visibility_scoping() {
        let service = Arc::new(RecordingSessionService::default());
        let access = ResourceAccessService::new(service.clone());

        let ack = access
            .delete_admin_session(&admin_identity(), "external-session")
            .await
            .expect("delete_admin_session should succeed");

        assert!(ack.success);
        assert_eq!(
            service.deleted_sessions.lock().unwrap().as_slice(),
            &["external-session".to_string()]
        );
    }
}
