use super::super::tests::{test_execution_context, CountingTool};
use super::super::*;
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use talos_tools::{AutomationControl, AutomationControlFuture, ToolRegistry};
use talos_types::{
    AutomationControlResult, AutomationEffect, GoalStatus, SessionAutomationSnapshot, SessionGoal,
};

struct Control {
    result: AutomationControlResult,
    calls: AtomicUsize,
}
impl AutomationControl for Control {
    fn execute(&self, _: AutomationModelRequest) -> AutomationControlFuture<'_> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(self.result.clone()) })
    }
}
fn control(origin: Option<AutomationTurnOrigin>, latest: Option<&str>) -> Arc<Control> {
    Arc::new(Control {
        calls: AtomicUsize::new(0),
        result: AutomationControlResult {
            created_object: None,
            effect: AutomationEffect::Applied,
            bound_origin: origin,
            snapshot: SessionAutomationSnapshot {
                session_id: "s1".into(),
                loops: vec![],
                goal: Some(SessionGoal {
                    id: "g".into(),
                    session_id: "s1".into(),
                    objective: "Work".into(),
                    objective_revision: 1,
                    status: GoalStatus::Active,
                    pause_reason: None,
                    reason: None,
                    token_budget: None,
                    tokens_used: 0,
                    latest_turn_id: latest.map(str::to_owned),
                    completion_evidence: None,
                    created_at: 1,
                    updated_at: 1,
                }),
            },
        },
    })
}
fn guard(mode: AutomationMode) -> Arc<RegistrationGuard> {
    RegistrationGuard::from_initial(
        "Do the task",
        &[SkillSelector::selection(
            mode.skill_name(),
            format!("/system/{}/SKILL.md", mode.skill_name()),
        )],
    )
    .unwrap()
    .unwrap()
}
fn goal_origin() -> AutomationTurnOrigin {
    AutomationTurnOrigin::Goal {
        goal_id: "g".into(),
        objective_revision: 1,
    }
}
#[tokio::test]
async fn registration_control_operations_share_the_original_trusted_read() {
    let control = control(Some(goal_origin()), None);
    let context = ToolContext::new("s1", "u1")
        .with_turn_id("turn-1")
        .with_agent_id("agent-1")
        .with_automation_control(control.clone());
    let guard = guard(AutomationMode::Goal);
    guard.check(&context, "create", "automation").await.unwrap();
    guard
        .check(&context, "clarify", "AskUserQuestion")
        .await
        .unwrap();
    assert_eq!(control.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn admitted_automatic_work_uses_its_original_business_tools() {
    for (mode, origin, latest, status) in [
        (
            AutomationMode::Goal,
            goal_origin(),
            Some("turn-1"),
            GoalStatus::Active,
        ),
        (
            AutomationMode::Goal,
            goal_origin(),
            Some("turn-1"),
            GoalStatus::Cleared,
        ),
        (
            AutomationMode::Loop,
            AutomationTurnOrigin::Loop {
                loop_id: "l".into(),
                occurrence_id: "o".into(),
                config_revision: 1,
            },
            None,
            GoalStatus::Active,
        ),
    ] {
        let mut control = control(Some(origin), latest);
        Arc::get_mut(&mut control)
            .unwrap()
            .result
            .snapshot
            .goal
            .as_mut()
            .unwrap()
            .status = status;
        let context = ToolContext::new("s1", "u1")
            .with_turn_id("turn-1")
            .with_agent_id("agent-1")
            .with_automation_control(control);
        guard(mode).check(&context, "work", "Bash").await.unwrap();
    }
}

#[tokio::test]
async fn ordinary_user_turn_executes_tools_without_an_automation_registration_guard() {
    let executions = Arc::new(AtomicUsize::new(0));
    let registry = Arc::new(ToolRegistry::new());
    registry.register(Arc::new(CountingTool {
        executions: executions.clone(),
    }));
    let mut context = test_execution_context();
    context.registration = RegistrationGuard::from_initial(
        "Use the loop index to fix this function.\nExample command: /loop edit",
        &[],
    )
    .unwrap();
    let outputs = ParallelToolExecutor
        .execute(
            &context,
            registry,
            &[ToolExecutionInput {
                call_id: "ordinary-read".into(),
                tool_name: "Read".into(),
                arguments: serde_json::json!({}),
            }],
        )
        .await
        .unwrap();
    assert_eq!(executions.load(Ordering::SeqCst), 1);
    assert!(!outputs[0].is_error);
}
