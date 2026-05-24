use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use talos_error::{TalosError, TalosResult};
use talos_session_store::SessionStore;
use talos_types::{
    OptionalProjectIdPatch, ProjectPatch, ProjectRecord, SessionConf, SessionPatch,
    SessionPatchAck, SessionQuery, SessionRecord, SessionRuntime,
    SessionService as RuntimeSessionService, TalosConfig, TalosContext, TalosSession,
};

use crate::session::SessionService;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorAccess {
    Regular,
    Full,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Actor {
    pub user_id: String,
    pub access: ActorAccess,
}

impl Actor {
    pub fn regular(user_id: impl Into<String>) -> Self {
        Self {
            user_id: user_id.into(),
            access: ActorAccess::Regular,
        }
    }

    pub fn full(user_id: impl Into<String>) -> Self {
        Self {
            user_id: user_id.into(),
            access: ActorAccess::Full,
        }
    }

    pub fn has_full_access(&self) -> bool {
        self.access == ActorAccess::Full
    }
}

#[derive(Clone)]
pub struct ResourceAccessService {
    session_runtime: Arc<dyn SessionRuntime>,
    session_records: Arc<SessionService>,
}

impl ResourceAccessService {
    pub fn new(runtime_sessions: Arc<dyn RuntimeSessionService>) -> Self {
        let runtime = Arc::new(RuntimeSessionAdapter::new(runtime_sessions.clone()));
        let records = Arc::new(SessionService::new(
            Arc::new(RuntimeSessionStoreAdapter::new(runtime_sessions)),
            TalosConfig::default(),
        ));
        Self {
            session_runtime: runtime,
            session_records: records,
        }
    }

    pub fn with_records(
        session_runtime: Arc<dyn SessionRuntime>,
        session_records: Arc<SessionService>,
    ) -> Self {
        Self {
            session_runtime,
            session_records,
        }
    }

    pub async fn create_session(
        &self,
        actor: &Actor,
        mut context: talos_types::talos::TalosContext,
        model: Option<String>,
        conf: Option<SessionConf>,
        project_id: Option<String>,
        current_directory: Option<PathBuf>,
    ) -> TalosResult<TalosSession> {
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
                context,
                &target_agent,
                model,
                conf,
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

    pub async fn delete_admin_session(
        &self,
        actor: &Actor,
        session_id: &str,
    ) -> TalosResult<SessionPatchAck> {
        Self::require_admin(actor)?;
        self.delete_runtime_session(session_id).await
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

    pub async fn list_admin_sessions(
        &self,
        actor: &Actor,
        query: SessionQuery,
    ) -> TalosResult<Vec<TalosSession>> {
        Self::require_admin(actor)?;
        self.session_records.list_sessions(query).await
    }

    pub async fn count_admin_sessions(
        &self,
        actor: &Actor,
        query: SessionQuery,
    ) -> TalosResult<usize> {
        Self::require_admin(actor)?;
        self.session_records.count_sessions(query).await
    }

    pub async fn list_admin_projects(&self, actor: &Actor) -> TalosResult<Vec<ProjectRecord>> {
        Self::require_admin(actor)?;
        self.session_records.list_projects().await
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

    pub fn require_admin(actor: &Actor) -> TalosResult<()> {
        if actor.has_full_access() {
            return Ok(());
        }
        Err(TalosError::AuthError("admin access required".to_string()))
    }
}

struct RuntimeSessionAdapter(Arc<dyn RuntimeSessionService>);

impl RuntimeSessionAdapter {
    fn new(service: Arc<dyn RuntimeSessionService>) -> Self {
        Self(service)
    }
}

#[async_trait]
impl SessionRuntime for RuntimeSessionAdapter {
    async fn resolve_target_agent(&self, context: &TalosContext) -> TalosResult<String> {
        self.0.resolve_target_agent(context).await
    }

    async fn compact_session(&self, session_id: &str) -> TalosResult<TalosSession> {
        self.0.compact_session(session_id).await
    }

    async fn clear_session(&self, session_id: &str) -> TalosResult<TalosSession> {
        self.0.clear_session(session_id).await
    }

    async fn close_session(&self, session_id: &str) -> TalosResult<()> {
        self.0.close_session(session_id).await
    }

    async fn delete_session(&self, session_id: &str) -> TalosResult<SessionPatchAck> {
        self.0.delete_session(session_id).await
    }

    async fn cancel_active_turn(&self, session_id: &str) -> TalosResult<()> {
        self.0.cancel_active_turn(session_id).await
    }

    async fn submit_dynamic_tool_result(
        &self,
        session_id: &str,
        call_id: &str,
        tool_name: &str,
        output: String,
        success: bool,
    ) -> TalosResult<()> {
        self.0
            .submit_dynamic_tool_result(session_id, call_id, tool_name, output, success)
            .await
    }
}

struct RuntimeSessionStoreAdapter(Arc<dyn RuntimeSessionService>);

impl RuntimeSessionStoreAdapter {
    fn new(service: Arc<dyn RuntimeSessionService>) -> Self {
        Self(service)
    }

    fn to_record(&self, session: TalosSession) -> TalosResult<SessionRecord> {
        Ok(SessionRecord::from_session(
            &session,
            session.config.llm.model.as_str(),
        ))
    }
}

#[async_trait]
impl SessionStore for RuntimeSessionStoreAdapter {
    async fn create_session(
        &self,
        context: &TalosContext,
        _agent_id: &str,
        _default_model: &str,
        requested_model: Option<&str>,
        conf: Option<talos_types::SessionConf>,
        _dm_scope: Option<String>,
        project_id: Option<String>,
        current_directory: Option<PathBuf>,
    ) -> TalosResult<SessionRecord> {
        let session = self
            .0
            .create_session(
                context.clone(),
                requested_model.map(str::to_string),
                conf,
                project_id,
                current_directory,
            )
            .await?;
        self.to_record(session)
    }

    async fn resolve_or_create_session(
        &self,
        context: &TalosContext,
        agent_id: &str,
        _default_model: &str,
        requested_model: Option<&str>,
        _dm_scope: Option<String>,
        project_id: Option<String>,
        current_directory: Option<PathBuf>,
    ) -> TalosResult<SessionRecord> {
        self.create_session(
            context,
            agent_id,
            _default_model,
            requested_model,
            None,
            None,
            project_id,
            current_directory,
        )
        .await
    }

    async fn get_session_record(&self, session_id: &str) -> TalosResult<Option<SessionRecord>> {
        match self.0.get_session(session_id).await? {
            Some(session) => Ok(Some(self.to_record(session)?)),
            None => Ok(None),
        }
    }

    async fn list_session_records(&self, query: &SessionQuery) -> TalosResult<Vec<SessionRecord>> {
        let mut records = Vec::new();
        for session in self.0.list_sessions(query.clone()).await? {
            records.push(self.to_record(session)?);
        }
        Ok(records)
    }

    async fn count_session_records(&self, query: &SessionQuery) -> TalosResult<usize> {
        self.0.count_sessions(query.clone()).await
    }

    async fn patch_session_record(
        &self,
        session_id: &str,
        patch: &talos_types::SessionPatch,
    ) -> TalosResult<SessionPatchAck> {
        self.0.patch_session(session_id, patch.clone()).await
    }

    async fn reset_session_record(&self, session_id: &str) -> TalosResult<SessionPatchAck> {
        self.0.delete_session(session_id).await
    }

    async fn delete_session_record(&self, session_id: &str) -> TalosResult<SessionPatchAck> {
        self.0.delete_session(session_id).await
    }

    async fn update_session_title(&self, _session_id: &str, _title: &str) -> TalosResult<()> {
        Ok(())
    }

    async fn record_session_progress(
        &self,
        _session_id: &str,
        _message_delta: u64,
        _usage: &talos_types::talos::TalosUsage,
        _model: Option<&str>,
    ) -> TalosResult<()> {
        Ok(())
    }

    async fn list_projects(&self) -> TalosResult<Vec<ProjectRecord>> {
        self.0.list_projects().await
    }

    async fn get_project(&self, project_id: &str) -> TalosResult<Option<ProjectRecord>> {
        self.0.get_project(project_id).await
    }

    async fn create_project(&self, name: &str, directory: &PathBuf) -> TalosResult<ProjectRecord> {
        self.0
            .create_project(name.to_string(), directory.clone(), String::new())
            .await
    }

    async fn create_project_for_owner(
        &self,
        name: &str,
        directory: &PathBuf,
        user_id: &str,
    ) -> TalosResult<ProjectRecord> {
        self.0
            .create_project(name.to_string(), directory.clone(), user_id.to_string())
            .await
    }

    async fn patch_project(
        &self,
        project_id: &str,
        patch: &ProjectPatch,
    ) -> TalosResult<Option<ProjectRecord>> {
        self.0.patch_project(project_id, patch.clone()).await
    }

    async fn delete_project(&self, project_id: &str) -> TalosResult<()> {
        self.0.delete_project(project_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::{Actor, ResourceAccessService};
    use async_trait::async_trait;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use talos_error::TalosResult;
    use talos_types::talos::{TalosContext, TalosProvider, TalosSourceType, TalosUsage};
    use talos_types::{
        ProjectPatch, ProjectRecord, SessionPatch, SessionPatchAck, SessionQuery, SessionService,
        TalosConfig, TalosSession,
    };

    #[derive(Default)]
    struct RecordingSessionService {
        listed_queries: Mutex<Vec<SessionQuery>>,
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
            Ok(session_for(
                &context.user_id,
                "session-1",
                conf.unwrap_or_default(),
                project_id,
                current_directory,
            ))
        }

        async fn list_sessions(&self, query: SessionQuery) -> TalosResult<Vec<TalosSession>> {
            self.listed_queries.lock().unwrap().push(query.clone());
            Ok(vec![
                session_for(
                    query.user_id.as_deref().unwrap_or("member-user"),
                    "session-1",
                    talos_types::SessionConf::default(),
                    None,
                    None,
                ),
                session_for(
                    "other-user",
                    "session-2",
                    talos_types::SessionConf::default(),
                    None,
                    None,
                ),
            ])
        }

        async fn count_sessions(&self, _query: SessionQuery) -> TalosResult<usize> {
            Ok(2)
        }

        async fn get_session(&self, session_id: &str) -> TalosResult<Option<TalosSession>> {
            Ok(Some(session_for(
                "member-user",
                session_id,
                talos_types::SessionConf::default(),
                None,
                None,
            )))
        }

        async fn patch_session(
            &self,
            _session_id: &str,
            _patch: SessionPatch,
        ) -> TalosResult<SessionPatchAck> {
            Ok(SessionPatchAck { success: true })
        }

        async fn compact_session(&self, _session_id: &str) -> TalosResult<TalosSession> {
            unreachable!("not used in these tests")
        }

        async fn clear_session(&self, _session_id: &str) -> TalosResult<TalosSession> {
            unreachable!("not used in these tests")
        }

        async fn close_session(&self, _session_id: &str) -> TalosResult<()> {
            Ok(())
        }

        async fn delete_session(&self, _session_id: &str) -> TalosResult<SessionPatchAck> {
            Ok(SessionPatchAck { success: true })
        }

        async fn cancel_active_turn(&self, _session_id: &str) -> TalosResult<()> {
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

        async fn list_projects(&self) -> TalosResult<Vec<ProjectRecord>> {
            Ok(vec![
                project_for("project-1", "member-user"),
                project_for("project-2", "other-user"),
            ])
        }

        async fn get_project(&self, project_id: &str) -> TalosResult<Option<ProjectRecord>> {
            Ok(Some(project_for(project_id, "member-user")))
        }

        async fn create_project(
            &self,
            name: String,
            directory: PathBuf,
            user_id: String,
        ) -> TalosResult<ProjectRecord> {
            Ok(ProjectRecord {
                project_id: "project-created".to_string(),
                name,
                user_id,
                directory,
                created_at: 0,
                updated_at: 0,
                session_count: 0,
            })
        }

        async fn patch_project(
            &self,
            project_id: &str,
            _patch: ProjectPatch,
        ) -> TalosResult<Option<ProjectRecord>> {
            Ok(Some(project_for(project_id, "member-user")))
        }

        async fn delete_project(&self, _project_id: &str) -> TalosResult<()> {
            Ok(())
        }
    }

    fn session_for(
        user_id: &str,
        session_id: &str,
        conf: talos_types::SessionConf,
        project_id: Option<String>,
        current_directory: Option<PathBuf>,
    ) -> TalosSession {
        TalosSession {
            title: "Untitled Session".to_string(),
            user_id: user_id.to_string(),
            context: talos_types::SessionContext {
                session_id: session_id.to_string(),
                workspace_id: None,
                source_type: TalosSourceType::Http,
                provider: TalosProvider::Http,
                user_id: user_id.to_string(),
                message_id: "message-1".to_string(),
                trace_id: "trace-1".to_string(),
                channel_id: Some("web".to_string()),
                thread_id: None,
                project_id: project_id.clone(),
                current_directory: current_directory.clone(),
                project_root: None,
                environment: None,
            },
            conf,
            project_id,
            current_directory,
            config: TalosConfig::default(),
            state: talos_types::SessionState {
                created_at: 0,
                last_activity: 0,
                message_count: 0,
                usage: TalosUsage::default(),
            },
        }
    }

    fn project_for(project_id: &str, user_id: &str) -> ProjectRecord {
        ProjectRecord {
            project_id: project_id.to_string(),
            name: project_id.to_string(),
            user_id: user_id.to_string(),
            directory: PathBuf::from("/tmp/project"),
            created_at: 0,
            updated_at: 0,
            session_count: 0,
        }
    }

    #[tokio::test]
    async fn list_sessions_scopes_query_to_actor() {
        let service = Arc::new(RecordingSessionService::default());
        let access = ResourceAccessService::new(service.clone());

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
            service.listed_queries.lock().unwrap()[0].user_id.as_deref(),
            Some("member-user")
        );
    }

    #[tokio::test]
    async fn regular_actor_cannot_use_admin_route() {
        let service = Arc::new(RecordingSessionService::default());
        let access = ResourceAccessService::new(service);

        let error = access
            .list_admin_projects(&Actor::regular("member-user"))
            .await
            .expect_err("regular actor should not access admin route");

        assert!(matches!(error, talos_error::TalosError::AuthError(_)));
    }

    #[tokio::test]
    async fn full_actor_can_use_admin_route() {
        let service = Arc::new(RecordingSessionService::default());
        let access = ResourceAccessService::new(service);

        let projects = access
            .list_admin_projects(&Actor::full("admin-user"))
            .await
            .expect("full actor should access admin route");

        assert_eq!(projects.len(), 2);
    }
}
