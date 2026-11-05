//! Thin host bindings into original Session authorization and Skill discovery.
use std::sync::{Arc, Weak};

use agent_protocol::WorkerEnvironmentContext;
use talos_coordinator::{AgentCoordinator, CoordinatorImpl};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_service::{automation::goal_loop::AutomationSkillResolver, TalosService};
use talos_store::session::{SessionRecord, SessionStore};
use talos_tools::{AutomationControl, AutomationControlFuture};
use talos_types::{
    AutomationMode, AutomationModelRequest, SkillScope, SkillSelector, SkillsListRequest,
};

pub(crate) struct ApplicationAutomationControl {
    service: Arc<TalosService>,
}

impl ApplicationAutomationControl {
    pub(crate) fn new(service: Arc<TalosService>) -> Self {
        Self { service }
    }
}

impl AutomationControl for ApplicationAutomationControl {
    fn execute(&self, request: AutomationModelRequest) -> AutomationControlFuture<'_> {
        Box::pin(self.service.execute_automation_model(request))
    }
}

/// Resolve trusted persisted execution scope through the original Worker port.
/// Background work has no public management access token. The Coordinator still
/// checks the saved node's owner, status and lease; it never falls back to a
/// different node or Skill root. Keep the Coordinator reference weak.
pub(crate) struct ApplicationAutomationSkillResolver {
    coordinator: Weak<CoordinatorImpl>,
    sessions: Arc<dyn SessionStore>,
}

impl ApplicationAutomationSkillResolver {
    pub(crate) fn new(coordinator: Weak<CoordinatorImpl>, sessions: Arc<dyn SessionStore>) -> Self {
        Self {
            coordinator,
            sessions,
        }
    }
}

impl AutomationSkillResolver for ApplicationAutomationSkillResolver {
    fn resolve<'a>(
        &'a self,
        session: &'a SessionRecord,
        mode: AutomationMode,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = TalosResult<SkillSelector>> + Send + 'a>>
    {
        Box::pin(async move {
            let coordinator = self.coordinator.upgrade().ok_or_else(|| {
                TalosError::new(
                    ErrorCode::ServiceUnavailable,
                    "Automation host is unavailable",
                )
            })?;
            let namespace = self
                .sessions
                .get_workspace_user_id(&session.id)
                .await?
                .unwrap_or_else(|| session.user_id.clone());
            let environment = coordinator
                .worker_environment(&session.execution_node_id, &session.user_id)
                .await?;
            let response = environment
                .list_skills(
                    WorkerEnvironmentContext {
                        user_id: namespace,
                        username: None,
                        workspace_id: Some(session.workspace_id.clone()),
                    },
                    SkillsListRequest {
                        node_id: None,
                        include_disabled: false,
                    },
                )
                .await?;
            let mut skills = response.data.into_iter().filter(|skill| {
                skill.scope == SkillScope::System
                    && skill.enabled
                    && skill.read_only
                    && skill.name == mode.skill_name()
            });
            let skill = skills.next().ok_or_else(|| {
                TalosError::new(
                    ErrorCode::ServiceUnavailable,
                    "The execution node's built-in automation Skill is unavailable",
                )
            })?;
            if skills.next().is_some() {
                return Err(TalosError::new(
                    ErrorCode::Conflict,
                    "The execution node has ambiguous System Skill definitions",
                ));
            }
            Ok(SkillSelector::selection(skill.name, skill.path))
        })
    }
}
