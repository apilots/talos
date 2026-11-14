//! Public authenticated adaptation; domain policy never owns a second Turn engine.
use super::*;
use crate::automation::goal_loop::AutomationSkillResolver;
use talos_store::automation::goal_loop::DatabaseAutomationStore;
use talos_types::*;
impl TalosService {
    /// Installs real Storage and execution-node System Skill resolution once.
    /// The host must complete this before exposing automation APIs or Tool control.
    pub fn install_session_automation(
        &self,
        store: DatabaseAutomationStore,
        resolver: Arc<dyn AutomationSkillResolver>,
    ) -> ServiceResult<()> {
        self.turns.install_automation(store, resolver)
    }
    pub fn subscribe_automation_changed(
        &self,
    ) -> ServiceResult<tokio::sync::broadcast::Receiver<AutomationChangedNotification>> {
        Ok(self.turns.automation_service()?.subscribe_changed())
    }
    pub async fn session_automation(
        &self,
        actor: &Actor,
        session_id: &str,
    ) -> ServiceResult<SessionAutomationSnapshot> {
        self.access.load_session(actor, session_id).await?;
        self.turns.automation_service()?.snapshot(session_id).await
    }
    pub async fn execute_automation(
        &self,
        actor: &Actor,
        request: AutomationRequest,
    ) -> ServiceResult<AutomationControlResult> {
        let session = self.access.load_session(actor, &request.session_id).await?;
        let automation = self.turns.automation_service()?;
        let start_goal = matches!(
            request.action,
            AutomationAction::Goal {
                action: GoalAction::Create { .. } | GoalAction::Resume { .. }
            }
        );
        let created_loop = matches!(
            request.action,
            AutomationAction::Loop {
                action: LoopAction::Create { .. }
            }
        );
        let explicit_target = match &request.action {
            AutomationAction::Goal {
                action: GoalAction::Resume { goal_id },
            } => Some(AutomationObjectRef::Goal {
                goal_id: goal_id.clone(),
            }),
            AutomationAction::Loop {
                action: LoopAction::Edit { loop_id, .. },
            } => Some(AutomationObjectRef::Loop {
                loop_id: loop_id.clone(),
            }),
            _ => None,
        };
        let cancelled_boundary = if start_goal || created_loop || explicit_target.is_some() {
            automation.cancelled_dispatch_boundary(&session.id).await?
        } else {
            None
        };
        let before = self
            .message_queue
            .list(&session.id)?
            .messages
            .into_iter()
            .map(|m| m.id)
            .collect::<std::collections::HashSet<_>>();
        let mut result = automation
            .execute(
                &session,
                request.action,
                crate::automation::goal_loop::AutomationCaller::User,
            )
            .await?;
        let new_wake = self
            .message_queue
            .list(&session.id)?
            .messages
            .iter()
            .any(|m| !before.contains(&m.id));
        if start_goal || created_loop || new_wake {
            if let Some(target) = result.created_object.clone().or(explicit_target) {
                automation.allow_explicit_dispatch(&session.id, target, cancelled_boundary);
            }
            let progression: ServiceResult<()> = async {
                self.turns.try_dispatch_next(&session.id).await?;
                self.turns.continue_goal(session.clone(), true).await
            }
            .await;
            if let Err(error) = progression {
                talos_logging::report_error(&error);
            }
            result.snapshot = automation.snapshot(&session.id).await?;
        }
        Ok(result)
    }
    /// Internal model port. The host authenticates invocation scope before entry;
    /// policy verifies exact durable Session/Turn, selected mode and automatic binding.
    pub async fn execute_automation_model(
        &self,
        request: AutomationModelRequest,
    ) -> ServiceResult<AutomationControlResult> {
        let session = self
            .sessions
            .load_record(&request.caller.session_id)
            .await?
            .ok_or_else(|| {
                TalosError::new(
                    talos_error::ErrorCode::ResourceNotFound,
                    "Automation Session was not found.",
                )
            })?;
        self.turns
            .automation_service()?
            .execute(
                &session,
                request.action,
                crate::automation::goal_loop::AutomationCaller::Model {
                    caller: &request.caller,
                    call_id: &request.call_id,
                },
            )
            .await
    }
    pub async fn list_automation(
        &self,
        actor: &Actor,
        request: AutomationListRequest,
    ) -> ServiceResult<AutomationListResponse> {
        let automation = self.turns.automation_service()?;
        if let Some(id) = request.session_id.as_ref() {
            self.access.load_session(actor, id).await?;
        }
        automation.store.list(&actor.user_id, &request).await
    }

    /// Original wake pump: Loop deadlines and never-admitted Goal deadlines.
    /// Ongoing Goals still use committed completion or known-idle reconciliation.
    pub async fn scan_session_loops(&self, now: i64) -> ServiceResult<()> {
        let automation = self.turns.automation_service()?;
        let loop_sessions = automation.store.active_loop_sessions().await?;
        let initial_goals = automation.store.initial_goal_sessions(now).await?;
        let sessions = loop_sessions
            .into_iter()
            .chain(initial_goals)
            .collect::<std::collections::BTreeSet<_>>();
        for id in sessions {
            let result: ServiceResult<()> = async {
                let Some(session) = self.sessions.load_record(&id).await? else {
                    return Ok(());
                };
                let mut permit = match automation.admit_scan(&session.id)? {
                    crate::automation::goal_loop::ScanAdmission::Skip => return Ok(()),
                    crate::automation::goal_loop::ScanAdmission::Run(permit) => permit,
                };
                automation.scan(&session, now).await?;
                self.turns.try_dispatch_next(&id).await?;
                self.turns.continue_goal(session, false).await?;
                if self.turns.canonical_active_turn(&id).await?.is_none() {
                    if let Some(permit) = permit.as_mut() {
                        permit.release_if_idle();
                    }
                }
                Ok(())
            }
            .await;
            // One unavailable execution node must not starve unrelated Sessions.
            // The scan owns reporting; absence does not mutate business state.
            if let Err(error) = result {
                talos_logging::report_error(&error);
            }
        }
        Ok(())
    }
    /// Reuses original recovery and the Worker idle boundary on reconnection.
    /// Never sweeps Goals at startup or reconstructs lost in-memory work.
    pub async fn reconcile_session_goal(&self, session_id: &str) -> ServiceResult<()> {
        let Some(automation) = self.turns.installed_automation() else {
            return Ok(());
        };
        if !automation
            .snapshot(session_id)
            .await?
            .goal
            .is_some_and(|goal| goal.status == GoalStatus::Active)
            || self
                .turns
                .canonical_active_turn(session_id)
                .await?
                .is_some()
        {
            return Ok(());
        }
        let session = self
            .sessions
            .load_record(session_id)
            .await?
            .ok_or_else(|| {
                TalosError::new(
                    talos_error::ErrorCode::ResourceNotFound,
                    "Automation Session was not found.",
                )
            })?;
        if self
            .sessions
            .remote_active_target(&session)
            .await?
            .is_some()
        {
            return Ok(());
        }
        match self.sessions.ensure_worker_idle(&session).await {
            Ok(_) => {}
            Err(error) if error.is_code(talos_error::ErrorCode::ActiveTurnRunning) => {
                return Ok(());
            }
            Err(error) => return Err(error),
        }
        self.turns.continue_goal(session, false).await
    }
}
