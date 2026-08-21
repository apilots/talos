use std::path::PathBuf;
use std::sync::Arc;

use talos_coordinator::{
    AgentSessionCompactionIdentity, AgentSessionCompactionParams, SessionRuntime,
};
use talos_error::{TalosError, TalosResult};
use talos_store::project::ProjectRecord;
use talos_store::session::{SessionPatchAck, SessionQuery};
use talos_types::{
    CancelActiveTurnParams, ModelSelection, OptionalProjectIdPatch, ProjectPatch, SessionConf,
    SessionListRequest, SessionPatch, TalosSession, UserPermissionMode,
};

use super::types::Actor;
use crate::project::ProjectService;
use crate::session::{CreateSessionParams, SessionService};

#[derive(Clone)]
pub(crate) struct ResourceAccessService {
    session_runtime: Arc<dyn SessionRuntime>,
    sessions: Arc<SessionService>,
    projects: Arc<ProjectService>,
}

/// Request accepted by `TalosService` when a caller needs a session handle.
pub struct CreateSessionRequest {
    pub session_id: String,
    pub context: talos_types::talos::TalosContext,
    pub model_selection: Option<ModelSelection>,
    pub user_permission_mode: Option<UserPermissionMode>,
    pub workspace_id: Option<String>,
    pub project_id: Option<String>,
    pub current_directory: Option<PathBuf>,
}

impl ResourceAccessService {
    pub(crate) fn with_services(
        session_runtime: Arc<dyn SessionRuntime>,
        sessions: Arc<SessionService>,
        projects: Arc<ProjectService>,
    ) -> Self {
        Self {
            session_runtime,
            sessions,
            projects,
        }
    }

    pub(crate) async fn get_or_create_session(
        &self,
        actor: &Actor,
        params: CreateSessionRequest,
    ) -> TalosResult<TalosSession> {
        match self.get_session(actor, &params.session_id).await {
            Ok(session) => return Ok(session),
            Err(error) if error.is_code(talos_error::ErrorCode::ResourceNotFound) => {}
            Err(error) => return Err(error),
        }

        let mut context = params.context;
        if context.project_root.is_some() {
            return Err(TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                "context.project_root is not supported".to_string(),
            ));
        }
        context.user_id = actor.user_id.clone();
        if let Some(project_id_ref) = params.project_id.as_deref() {
            self.ensure_project_access(actor, project_id_ref).await?;
        }
        let target_agent = self.resolve_target_agent(&context).await?;
        self.sessions
            .create_session(CreateSessionParams {
                session_id: Some(params.session_id),
                context,
                target_agent,
                model_selection: params.model_selection,
                conf: params
                    .user_permission_mode
                    .map(|user_permission_mode| SessionConf {
                        user_permission_mode: Some(user_permission_mode),
                        ..Default::default()
                    }),
                workspace_id: params.workspace_id,
                project_id: params.project_id,
                current_directory: params.current_directory,
            })
            .await
    }

    pub(crate) async fn list_sessions(
        &self,
        actor: &Actor,
        request: SessionListRequest,
    ) -> TalosResult<Vec<TalosSession>> {
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
        let sessions = self.sessions.list_sessions(query).await?;
        Ok(self.filter_sessions(actor, sessions))
    }

    pub(crate) async fn get_session(
        &self,
        actor: &Actor,
        session_id: &str,
    ) -> TalosResult<TalosSession> {
        let session = self.get_session_record(session_id).await?;
        self.ensure_session_visibility(actor, &session)?;
        let record = self
            .sessions
            .get_session_record(session_id)
            .await?
            .ok_or_else(|| {
                TalosError::public(
                    talos_error::ErrorCode::ResourceNotFound,
                    format!("session {session_id}"),
                )
            })?;
        if matches!(
            record.tree_identity,
            talos_store::session::SessionTreeIdentity::Subagent { .. }
        ) {
            return Err(TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                format!("session '{session_id}' is controlled by its parent agent"),
            ));
        }
        Ok(session)
    }

    pub(crate) async fn patch_session(
        &self,
        actor: &Actor,
        session_id: &str,
        patch: SessionPatch,
    ) -> TalosResult<SessionPatchAck> {
        let _session = self.get_session(actor, session_id).await?;
        if let Some(OptionalProjectIdPatch::Set { project_id }) = patch.project_id.as_ref() {
            self.ensure_project_access(actor, project_id).await?;
        }
        let ack = self.sessions.patch_session(session_id, patch).await?;
        if !ack.success {
            return Err(TalosError::public(
                talos_error::ErrorCode::ResourceNotFound,
                format!("session {}", session_id),
            ));
        }
        self.invalidate_cached_session(session_id).await?;
        Ok(ack)
    }

    pub(crate) async fn compact_session(
        &self,
        actor: &Actor,
        session_id: &str,
        identity: AgentSessionCompactionIdentity,
        event_tx: tokio::sync::mpsc::Sender<talos_types::talos::AgentEvent>,
    ) -> TalosResult<TalosSession> {
        let session = self.get_session(actor, session_id).await?;
        let usage = self
            .compact_runtime_session(AgentSessionCompactionParams {
                session: session.clone(),
                identity,
                event_tx,
            })
            .await?;
        self.sessions
            .record_compaction_usage(session_id, &session.config.llm.model, &usage)
            .await?;
        self.invalidate_cached_session(session_id).await?;
        self.get_session(actor, session_id).await
    }

    pub(crate) async fn clear_session(
        &self,
        actor: &Actor,
        session_id: &str,
    ) -> TalosResult<TalosSession> {
        let session = self.get_session(actor, session_id).await?;
        self.clear_runtime_session(session).await?;
        let ack = self.sessions.reset_session(session_id).await?;
        if !ack.success {
            return Err(TalosError::public(
                talos_error::ErrorCode::ResourceNotFound,
                format!("session {}", session_id),
            ));
        }
        self.invalidate_cached_session(session_id).await?;
        self.get_session(actor, session_id).await
    }

    pub(crate) async fn close_session(&self, actor: &Actor, session_id: &str) -> TalosResult<()> {
        let session = self.get_session(actor, session_id).await?;
        self.close_runtime_session(session).await
    }

    pub(crate) async fn delete_session(
        &self,
        actor: &Actor,
        session_id: &str,
    ) -> TalosResult<SessionPatchAck> {
        let session = self.get_session(actor, session_id).await?;
        self.prepare_runtime_delete_session(session).await?;
        let ack = self.sessions.delete_session(session_id).await?;
        if !ack.success {
            return Err(TalosError::public(
                talos_error::ErrorCode::ResourceNotFound,
                format!("session {}", session_id),
            ));
        }
        self.invalidate_cached_session(session_id).await?;
        Ok(ack)
    }

    pub(crate) async fn discard_session_record(
        &self,
        actor: &Actor,
        session_id: &str,
    ) -> TalosResult<SessionPatchAck> {
        let _session = self.get_session(actor, session_id).await?;
        let ack = self.sessions.delete_session(session_id).await?;
        if !ack.success {
            return Err(TalosError::public(
                talos_error::ErrorCode::ResourceNotFound,
                format!("session {}", session_id),
            ));
        }
        self.invalidate_cached_session(session_id).await?;
        Ok(ack)
    }

    pub(crate) async fn cancel_active_turn(
        &self,
        actor: &Actor,
        session_id: &str,
        turn_id: String,
    ) -> TalosResult<()> {
        let session = self.get_session(actor, session_id).await?;
        let record = self
            .sessions
            .get_session_record(session_id)
            .await?
            .ok_or_else(|| {
                TalosError::public(
                    talos_error::ErrorCode::ResourceNotFound,
                    format!("session {session_id}"),
                )
            })?;
        self.cancel_runtime_turn(CancelActiveTurnParams {
            binding: talos_types::agent_dispatch::DispatchBinding::Exact {
                agent_id: record.agent_id,
            },
            session,
            turn_id,
        })
        .await
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
                TalosError::public(
                    talos_error::ErrorCode::ResourceNotFound,
                    format!("project {}", project_id),
                )
            })?;
        self.invalidate_project_sessions(project_id).await?;
        Ok(project)
    }

    pub(crate) async fn delete_project(&self, actor: &Actor, project_id: &str) -> TalosResult<()> {
        let _project = self.get_project(actor, project_id).await?;
        self.invalidate_project_sessions(project_id).await?;
        self.projects.delete_project(project_id).await
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
        Err(TalosError::public(
            talos_error::ErrorCode::ResourceNotFound,
            format!("session {}", session.context.session_id),
        ))
    }

    fn ensure_project_visibility(&self, actor: &Actor, project: &ProjectRecord) -> TalosResult<()> {
        if project.user_id == actor.user_id {
            return Ok(());
        }
        Err(TalosError::public(
            talos_error::ErrorCode::ResourceNotFound,
            format!("project {}", project.project_id),
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

    async fn get_session_record(&self, session_id: &str) -> TalosResult<TalosSession> {
        self.sessions.get_session(session_id).await?.ok_or_else(|| {
            TalosError::public(
                talos_error::ErrorCode::ResourceNotFound,
                format!("session {}", session_id),
            )
        })
    }

    async fn get_project_record(&self, project_id: &str) -> TalosResult<ProjectRecord> {
        self.projects.get_project(project_id).await?.ok_or_else(|| {
            TalosError::public(
                talos_error::ErrorCode::ResourceNotFound,
                format!("project {}", project_id),
            )
        })
    }

    async fn invalidate_project_sessions(&self, project_id: &str) -> TalosResult<()> {
        let query = SessionQuery {
            project_id: Some(project_id.to_string()),
            ..SessionQuery::default()
        };
        let sessions = self.sessions.list_sessions(query).await?;
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

    async fn compact_runtime_session(
        &self,
        params: AgentSessionCompactionParams,
    ) -> TalosResult<talos_types::talos::TokenUsage> {
        self.session_runtime.compact_session(params).await
    }

    async fn clear_runtime_session(&self, session: TalosSession) -> TalosResult<()> {
        self.session_runtime.clear_session(session).await
    }

    async fn close_runtime_session(&self, session: TalosSession) -> TalosResult<()> {
        self.session_runtime.close_session(session).await
    }

    async fn prepare_runtime_delete_session(&self, session: TalosSession) -> TalosResult<()> {
        self.session_runtime.prepare_delete_session(session).await
    }

    async fn cancel_runtime_turn(&self, params: CancelActiveTurnParams) -> TalosResult<()> {
        self.session_runtime.cancel_active_turn(params).await
    }

    pub(crate) async fn has_active_runtime_turn(
        &self,
        session: &TalosSession,
    ) -> TalosResult<bool> {
        self.session_runtime.has_active_turn(session).await
    }
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
