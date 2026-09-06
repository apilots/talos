use std::path::PathBuf;
use std::sync::Arc;

use talos_coordinator::{AgentCoordinator, AgentSelectionParams};
use talos_error::{TalosError, TalosResult};
use talos_store::project::ProjectRecord;
use talos_store::session::{SessionQuery, SessionRecord, SessionTreeIdentity};
use talos_types::{
    ModelSelection, ProjectPatch, SessionConf, SessionInfo, SessionListRequest, SessionPatch,
    UserPermissionMode,
};

use super::types::Actor;
use crate::project::ProjectService;
use crate::session::{CreateSessionParams, SessionService};

#[derive(Clone)]
pub(crate) struct ResourceAccessService {
    coordinator: Arc<dyn AgentCoordinator>,
    sessions: Arc<SessionService>,
    projects: Arc<ProjectService>,
}

/// Request accepted by `TalosService` when a caller needs a session handle.
pub(crate) struct CreateSessionRequest {
    pub session_id: String,
    pub source: talos_types::talos::SessionSource,
    pub model_selection: Option<ModelSelection>,
    pub user_permission_mode: Option<UserPermissionMode>,
    pub project_id: Option<String>,
    pub current_directory: Option<PathBuf>,
}

impl ResourceAccessService {
    pub(crate) fn with_services(
        coordinator: Arc<dyn AgentCoordinator>,
        sessions: Arc<SessionService>,
        projects: Arc<ProjectService>,
    ) -> Self {
        Self {
            coordinator,
            sessions,
            projects,
        }
    }

    pub(crate) async fn create_session(
        &self,
        actor: &Actor,
        params: CreateSessionRequest,
    ) -> TalosResult<SessionRecord> {
        if let Some(project_id_ref) = params.project_id.as_deref() {
            self.ensure_project_access(actor, project_id_ref).await?;
        }
        let target_agent = self
            .resolve_target_agent(&params.source, &actor.user_id)
            .await?;
        self.sessions
            .create_session(CreateSessionParams {
                session_id: Some(params.session_id),
                source: params.source,
                user_id: actor.user_id.clone(),
                target_agent,
                model_selection: params.model_selection,
                conf: params
                    .user_permission_mode
                    .map(|user_permission_mode| SessionConf {
                        user_permission_mode: Some(user_permission_mode),
                        ..Default::default()
                    }),
                workspace_id: self.sessions.default_workspace_id(),
                project_id: params.project_id,
                current_directory: params.current_directory,
            })
            .await
    }

    pub(crate) async fn list_sessions(
        &self,
        actor: &Actor,
        request: SessionListRequest,
    ) -> TalosResult<Vec<SessionInfo>> {
        let query = SessionQuery {
            tree_scope: talos_store::session::SessionTreeScope::Primary,
            user_id: Some(actor.user_id.clone()),
            channel_id: None,
            provider: None,
            project_id: request.project_id,
            limit: request.limit,
            offset: request.offset,
            ..SessionQuery::default()
        };
        self.sessions.list_sessions(query).await
    }

    pub(crate) async fn load_session(
        &self,
        actor: &Actor,
        session_id: &str,
    ) -> TalosResult<SessionRecord> {
        let record = self
            .sessions
            .load_record(session_id)
            .await?
            .ok_or_else(|| {
                TalosError::new(
                    talos_error::ErrorCode::ResourceNotFound,
                    format!("Session {session_id} was not found."),
                )
            })?;
        if matches!(record.tree_identity, SessionTreeIdentity::Subagent { .. }) {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                format!("session '{session_id}' is controlled by its parent agent"),
            ));
        }
        self.ensure_session_visibility(actor, &record)?;
        Ok(record)
    }

    pub(crate) async fn patch_session(
        &self,
        session: SessionRecord,
        patch: SessionPatch,
    ) -> TalosResult<SessionRecord> {
        let session_id = session.id.clone();
        self.sessions
            .patch_session(session, patch)
            .await?
            .ok_or_else(|| {
                TalosError::new(
                    talos_error::ErrorCode::ResourceNotFound,
                    format!("Session {session_id} was not found."),
                )
            })
    }

    pub(crate) async fn clear_session(&self, session: SessionRecord) -> TalosResult<SessionInfo> {
        let context = self.sessions.build_agent_session_context(&session)?;
        self.coordinator.clear_session(context).await?;
        let record = self.sessions.clear_session_history(session).await?;
        self.sessions.session_info(&record)
    }

    pub(crate) async fn close_session(&self, session: SessionRecord) -> TalosResult<()> {
        let context = self.sessions.build_agent_session_context(&session)?;
        self.coordinator.close_session(context).await
    }

    pub(crate) async fn delete_session(&self, session: SessionRecord) -> TalosResult<bool> {
        let context = self.sessions.build_agent_session_context(&session)?;
        self.coordinator.clear_session(context.clone()).await?;
        self.coordinator.close_session(context).await?;
        self.sessions.delete_session(session).await
    }

    pub(crate) async fn list_projects(&self, actor: &Actor) -> TalosResult<Vec<ProjectRecord>> {
        let projects = self.projects.list_projects().await?;
        Ok(self.filter_projects(actor, projects))
    }

    pub(crate) async fn get_project(
        &self,
        actor: &Actor,
        project_id: &str,
    ) -> TalosResult<ProjectRecord> {
        let project = self.get_project_record(project_id).await?;
        self.ensure_project_visibility(actor, &project)?;
        Ok(project)
    }

    pub(crate) async fn create_project(
        &self,
        actor: &Actor,
        name: String,
        directory: PathBuf,
    ) -> TalosResult<ProjectRecord> {
        self.projects
            .create_project(name, directory, actor.user_id.clone())
            .await
    }

    pub(crate) async fn patch_project(
        &self,
        actor: &Actor,
        project_id: &str,
        patch: ProjectPatch,
    ) -> TalosResult<ProjectRecord> {
        let _project = self.get_project(actor, project_id).await?;
        let project = self
            .projects
            .patch_project(project_id, patch)
            .await?
            .ok_or_else(|| {
                TalosError::new(
                    talos_error::ErrorCode::ResourceNotFound,
                    format!("Project {} was not found.", project_id),
                )
            })?;
        Ok(project)
    }

    pub(crate) async fn delete_project(&self, actor: &Actor, project_id: &str) -> TalosResult<()> {
        let _project = self.get_project(actor, project_id).await?;
        self.projects.delete_project(project_id).await
    }

    fn filter_projects(&self, actor: &Actor, projects: Vec<ProjectRecord>) -> Vec<ProjectRecord> {
        projects
            .into_iter()
            .filter(|project| project.user_id == actor.user_id)
            .collect()
    }

    fn ensure_session_visibility(&self, actor: &Actor, session: &SessionRecord) -> TalosResult<()> {
        if session.user_id == actor.user_id {
            return Ok(());
        }
        Err(TalosError::new(
            talos_error::ErrorCode::ResourceNotFound,
            format!("Session {} was not found.", session.id),
        ))
    }

    fn ensure_project_visibility(&self, actor: &Actor, project: &ProjectRecord) -> TalosResult<()> {
        if project.user_id == actor.user_id {
            return Ok(());
        }
        Err(TalosError::new(
            talos_error::ErrorCode::ResourceNotFound,
            format!("Project {} was not found.", project.project_id),
        ))
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

    async fn get_project_record(&self, project_id: &str) -> TalosResult<ProjectRecord> {
        self.projects.get_project(project_id).await?.ok_or_else(|| {
            TalosError::new(
                talos_error::ErrorCode::ResourceNotFound,
                format!("Project {} was not found.", project_id),
            )
        })
    }

    async fn resolve_target_agent(
        &self,
        source: &talos_types::talos::SessionSource,
        user_id: &str,
    ) -> TalosResult<String> {
        self.coordinator
            .resolve_agent(AgentSelectionParams::for_session_source(
                source.clone(),
                user_id.to_string(),
            ))
            .await
    }
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
