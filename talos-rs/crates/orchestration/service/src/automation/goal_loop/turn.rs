use super::*;
use crate::automation::goal_loop::{AutomationSkillResolver, SessionAutomationService};
use talos_store::automation::goal_loop::DatabaseAutomationStore;
use talos_types::{AutomationMode, AutomationTurnOrigin};
impl TurnService {
    pub(crate) fn installed_automation(&self) -> Option<Arc<SessionAutomationService>> {
        self.automation.get().cloned()
    }

    pub(crate) fn automation_service(&self) -> TalosResult<Arc<SessionAutomationService>> {
        self.automation.get().cloned().ok_or_else(|| {
            TalosError::new(
                talos_error::ErrorCode::ServiceUnavailable,
                "Session automation has not been installed on this owner.",
            )
        })
    }
    pub(crate) fn install_automation(
        &self,
        store: DatabaseAutomationStore,
        resolver: Arc<dyn AutomationSkillResolver>,
    ) -> TalosResult<()> {
        let automation = SessionAutomationService::new(
            store,
            self.execution.execution_store.clone(),
            self.message_queue.clone(),
            resolver,
        )
        .with_managed_workers(self.nodes.clone());
        if let Some(owner) = self.session_owner() {
            automation.set_session_owner(owner);
        }
        self.automation.set(Arc::new(automation)).map_err(|_| {
            TalosError::new(
                talos_error::ErrorCode::Conflict,
                "Session automation is already installed.",
            )
        })
    }
    pub(crate) async fn continue_goal(
        &self,
        session: SessionRecord,
        explicit: bool,
    ) -> TalosResult<()> {
        let Some(automation) = self.automation.get().cloned() else {
            return Ok(());
        };
        if self.canonical_active_turn(&session.id).await?.is_some()
            || !self.message_queue.list(&session.id)?.messages.is_empty()
        {
            return Ok(());
        }
        let Some(goal) = automation
            .snapshot(&session.id)
            .await?
            .goal
            .filter(|g| g.status == talos_types::GoalStatus::Active)
        else {
            return Ok(());
        };
        if !explicit
            && !self
                .can_dispatch_automation(
                    &session.id,
                    &talos_types::AutomationObjectRef::Goal {
                        goal_id: goal.id.clone(),
                    },
                )
                .await?
        {
            return Ok(());
        }
        if SessionAutomationService::goal_budget_exhausted(&goal, chrono::Utc::now().timestamp()) {
            automation
                .pause(
                    &goal,
                    talos_types::GoalPauseReason::BudgetExhausted,
                    chrono::Utc::now().timestamp(),
                )
                .await?;
            return Ok(());
        }
        if goal.latest_turn_id.is_none()
            && chrono::Utc::now().timestamp()
                < goal
                    .created_at
                    .saturating_add(talos_types::AUTOMATION_INITIAL_DELAY_SECONDS)
        {
            return Ok(());
        }
        if self
            .defer_managed_dispatch(
                &session,
                super::managed_wake::WakeCandidate::Goal { explicit },
            )
            .await?
        {
            return Ok(());
        }
        let expected = goal.clone();
        let result: TalosResult<_> = async {
            let input = automation
                .skill_input(&session, AutomationMode::Goal, goal.objective)
                .await?;
            self.submit_turn(
                session,
                TurnSubmission {
                    queue_message_id: None,
                    input,
                    retry_of_message_id: None,
                    origin: Some(AutomationTurnOrigin::Goal {
                        goal_id: goal.id,
                        objective_revision: goal.objective_revision,
                    }),
                },
                None,
                None,
                None,
            )
            .await
        }
        .await;
        if let Err(error) = result {
            if !error.is_code(talos_error::ErrorCode::Conflict)
                && !error.is_code(talos_error::ErrorCode::ActiveTurnRunning)
            {
                let current = automation.store.goal(&expected.id).await?;
                if current.status == talos_types::GoalStatus::Active
                    && current.objective_revision == expected.objective_revision
                {
                    automation
                        .pause(
                            &current,
                            talos_types::GoalPauseReason::ExecutionFailed,
                            chrono::Utc::now().timestamp(),
                        )
                        .await?;
                }
            }
            return Err(error);
        }

        Ok(())
    }
}
