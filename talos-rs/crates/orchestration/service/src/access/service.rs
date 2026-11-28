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
    restrict_embedded: bool,
    nodes: Option<crate::ProductNodeAccess>,
    product_owner: talos_types::NodeProductOwner,
}

/// Media-only access preserves NodeMedia grants without exposing product operations.
/// The inner environment never leaves this boundary and cannot create Session state.
pub(crate) struct DesktopWorkerEnvironment(Arc<dyn agent_protocol::WorkerEnvironment>);
impl DesktopWorkerEnvironment {
    pub(crate) async fn snapshot(
        &self,
        context: agent_protocol::WorkerEnvironmentContext,
    ) -> TalosResult<talos_types::BrowserSnapshot> {
        self.0
            .browser(
                context,
                talos_types::BrowserRequest {
                    node_id: None,
                    operation: talos_types::BrowserOperation::Read,
                },
            )
            .await
    }
    pub(crate) async fn open(
        &self,
        context: agent_protocol::WorkerEnvironmentContext,
        request: talos_types::WorkerDesktopOpen,
    ) -> TalosResult<()> {
        self.0.desktop_open(context, request).await
    }
}

/// Request accepted by `TalosService` when a caller needs a session handle.
pub(crate) struct CreateSessionRequest {
    pub node_id: Option<String>,
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
        restrict_embedded: bool,
        nodes: Option<crate::ProductNodeAccess>,
    ) -> Self {
        Self {
            coordinator,
            sessions,
            projects,
            restrict_embedded,
            nodes,
            product_owner: talos_types::NodeProductOwner::Server,
        }
    }

    pub(crate) fn set_product_owner(&mut self, owner: talos_types::NodeProductOwner) {
        self.product_owner = owner;
    }

    async fn ensure_product_node_access(&self, actor: &Actor, node_id: &str) -> TalosResult<()> {
        if node_id == "embedded" {
            return Ok(());
        }
        if self.product_owner == talos_types::NodeProductOwner::Computer {
            return Err(TalosError::new(
                talos_error::ErrorCode::PermissionDenied,
                "Personal products operate only their embedded Worker.",
            ));
        }
        let Some(nodes) = &self.nodes else {
            return Err(TalosError::new(
                talos_error::ErrorCode::PermissionDenied,
                "This product has no registered Worker authority.",
            ));
        };
        if nodes.product_owner(actor, node_id).await? != Some(talos_types::NodeProductOwner::Server)
        {
            return Err(TalosError::new(
                talos_error::ErrorCode::PermissionDenied,
                "Personal Computer operations require its product API through Relay.",
            ));
        }
        nodes
            .ensure_execution_product_owner(
                node_id,
                &actor.user_id,
                talos_types::NodeProductOwner::Server,
            )
            .await?;
        Ok(())
    }

    pub(crate) async fn ensure_worker_access(
        &self,
        actor: &Actor,
        node_id: &str,
    ) -> TalosResult<()> {
        if self.restrict_embedded
            && node_id == "embedded"
            && (actor.auth_disabled || actor.role != talos_auth::UserRole::Admin)
        {
            return Err(embedded_access_denied());
        }
        self.ensure_managed_node_access(actor, node_id).await
    }

    async fn ensure_managed_node_access(&self, actor: &Actor, node_id: &str) -> TalosResult<()> {
        self.ensure_product_node_access(actor, node_id).await?;
        if let Some(nodes) = &self.nodes {
            nodes.ensure_managed_node_access(actor, node_id).await?;
        }
        Ok(())
    }

    pub(crate) async fn create_session(
        &self,
        actor: &Actor,
        params: CreateSessionRequest,
    ) -> TalosResult<SessionRecord> {
        let project = match params.project_id.as_deref() {
            Some(id) => Some(self.ensure_project_access(actor, id).await?),
            None => None,
        };
        if let (Some(node_id), Some(project)) = (&params.node_id, &project) {
            if node_id != &project.execution_node_id {
                return Err(TalosError::new(
                    talos_error::ErrorCode::InvalidRequest,
                    "Project and requested Worker differ",
                ));
            }
        }
        let preferred = project
            .as_ref()
            .map(|project| project.execution_node_id.as_str())
            .or(params.node_id.as_deref());
        let target_agent = self
            .resolve_target_agent(&params.source, &actor.user_id)
            .await?;
        let execution_node_id = self
            .coordinator
            .select_execution_node(&target_agent, &actor.user_id, preferred)
            .await?;
        let workspace = self
            .worker_environment(actor, &execution_node_id)
            .await?
            .workspace_info(crate::access::worker_environment_context(actor))
            .await?;
        self.sessions
            .create_session(CreateSessionParams {
                session_id: Some(params.session_id),
                source: params.source,
                user_id: actor.user_id.clone(),
                target_agent,
                execution_node_id,
                model_selection: params.model_selection,
                conf: params
                    .user_permission_mode
                    .map(|user_permission_mode| SessionConf {
                        user_permission_mode: Some(user_permission_mode),
                        ..Default::default()
                    }),
                workspace_id: workspace.workspace_id,
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
        if request
            .node_id
            .as_ref()
            .is_some_and(|id| id.trim().is_empty())
        {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "Node ID cannot be empty",
            ));
        }
        let query = SessionQuery {
            worker_execution_only: actor.request_origin
                == talos_auth::ProductRequestOrigin::RemoteProduct,
            tree_scope: talos_store::session::SessionTreeScope::Primary,
            user_id: Some(actor.user_id.clone()),
            channel_id: None,
            provider: None,
            execution_node_id: request.node_id,
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
        self.sessions
            .ensure_product_history_access(actor, session_id)
            .await?;
        if record.session_kind == talos_types::SessionKind::Talo {
            talos_auth::ensure_managed_worker_access(actor)?;
        }
        Ok(record)
    }

    pub(crate) async fn patch_session(
        &self,
        session: SessionRecord,
        patch: SessionPatch,
    ) -> TalosResult<SessionRecord> {
        self.sessions
            .ensure_execution_supported(&session.id)
            .await?;
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
        let session = self.sessions.prepare_execution(&session).await?;
        let mut expected_history_cursor = self.sessions.ensure_worker_idle(&session).await?;
        expected_history_cursor.history_generation = self
            .sessions
            .product_history_generation(&session.id)
            .await?;
        let context = self.sessions.build_agent_session_context(&session).await?;
        self.coordinator
            .clear_session(context, expected_history_cursor)
            .await?;
        let record = self.sessions.clear_session_history(session).await?;
        self.sessions.session_info(&record).await
    }

    pub(crate) async fn close_session(&self, session: SessionRecord) -> TalosResult<()> {
        let session = self.sessions.prepare_execution(&session).await?;
        let expected_history_cursor = self.sessions.ensure_worker_idle(&session).await?;
        let context = self.sessions.build_agent_session_context(&session).await?;
        self.coordinator
            .close_session(context, expected_history_cursor)
            .await?;
        Ok(())
    }

    pub(crate) async fn prepare_session_deletion(
        &self,
        session: SessionRecord,
    ) -> TalosResult<SessionRecord> {
        let session = self.sessions.prepare_execution(&session).await?;
        let expected_history_cursor = self.sessions.ensure_worker_idle(&session).await?;
        let context = self.sessions.build_agent_session_context(&session).await?;
        self.coordinator
            .retire_session(context, expected_history_cursor)
            .await?;
        Ok(session)
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

    /// Authorizes the original NodeMedia path independently of product ownership.
    /// This port supports snapshot validation and one ephemeral stream grant only.
    pub(crate) async fn desktop_environment(
        &self,
        actor: &Actor,
        node_id: &str,
    ) -> TalosResult<DesktopWorkerEnvironment> {
        if node_id == "embedded" {
            self.ensure_worker_access(actor, node_id).await?;
        } else {
            if self.product_owner != talos_types::NodeProductOwner::Server || actor.auth_disabled {
                return Err(TalosError::new(
                    talos_error::ErrorCode::PermissionDenied,
                    "Remote desktop admission requires the central Server.",
                ));
            }
            let nodes = self.nodes.as_ref().ok_or_else(|| {
                TalosError::new(
                    talos_error::ErrorCode::PermissionDenied,
                    "NodeMedia authority is unavailable.",
                )
            })?;
            nodes
                .get_node(
                    actor,
                    crate::GetNodeRequest {
                        node_id: node_id.into(),
                    },
                )
                .await?;
        }
        Ok(DesktopWorkerEnvironment(
            self.coordinator
                .worker_environment(node_id, &actor.user_id)
                .await?,
        ))
    }

    pub(crate) async fn worker_environment(
        &self,
        actor: &Actor,
        node_id: &str,
    ) -> TalosResult<Arc<dyn agent_protocol::WorkerEnvironment>> {
        self.ensure_worker_access(actor, node_id).await?;
        self.coordinator
            .worker_environment(node_id, &actor.user_id)
            .await
    }

    pub(crate) async fn create_project(
        &self,
        actor: &Actor,
        request: talos_types::ProjectCreateRequest,
    ) -> TalosResult<ProjectRecord> {
        let node_id = request.node_id.unwrap_or_else(|| "embedded".to_owned());
        let directory = self
            .worker_environment(actor, &node_id)
            .await?
            .validate_project_directory(
                crate::access::worker_environment_context(actor),
                request.directory,
            )
            .await?;
        self.projects
            .create_project(request.name, directory, actor.user_id.clone(), node_id)
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
        self.ensure_managed_node_access(actor, &project.execution_node_id)
            .await?;
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

pub(crate) fn embedded_access_denied() -> TalosError {
    TalosError::new(
        talos_error::ErrorCode::PermissionDenied,
        "The Server's embedded Worker requires an administrator.",
    )
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
