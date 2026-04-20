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
        context.user_id = actor.user_id.clone();
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
        query.owner_user_id = Some(actor.user_id.clone());
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
        if actor.is_admin() {
            return sessions;
        }
        sessions
            .into_iter()
            .filter(|session| session.owner_user_id == actor.user_id)
            .collect()
    }

    fn filter_projects(
        &self,
        actor: &RequestIdentity,
        projects: Vec<ProjectRecord>,
    ) -> Vec<ProjectRecord> {
        projects
            .into_iter()
            .filter(|project| project.owner_user_id == actor.user_id)
            .collect()
    }

    fn ensure_session_visibility(
        &self,
        actor: &RequestIdentity,
        session: &TalosSession,
    ) -> TalosResult<()> {
        if actor.is_admin() || session.owner_user_id == actor.user_id {
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
        if actor.is_admin() || project.owner_user_id == actor.user_id {
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
        if actor.is_admin() {
            return Ok(());
        }
        Err(TalosError::AuthError("admin access required".to_string()))
    }
}
