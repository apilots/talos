use super::super::tests::{test_execution_context, CountingTool};
use super::super::*;
use super::*;
use crate::ToolStreamKey;
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
async fn registration_blocks_parallel_and_streamed_work_with_one_trusted_read() {
    let control = control(None, None);
    let executions = Arc::new(AtomicUsize::new(0));
    let registry = Arc::new(ToolRegistry::new());
    registry.set_automation_control(control.clone());
    registry.register(Arc::new(CountingTool {
        executions: executions.clone(),
    }));
    let mut context = test_execution_context();
    context.registration = Some(guard(AutomationMode::Goal));
    let calls = (0..4)
        .map(|i| ToolExecutionInput {
            call_id: format!("call-{i}"),
            tool_name: "Read".into(),
            arguments: serde_json::json!({}),
        })
        .collect::<Vec<_>>();
    let outputs = ParallelToolExecutor
        .execute(&context, registry.clone(), &calls)
        .await
        .unwrap();
    assert!(outputs.iter().all(|output| output.is_error));
    let mut stream =
        StreamingToolScheduler::new(ToolExecutionMode::Parallel, 2, context, registry, None);
    stream
        .push_call(ToolStreamKey { round: 1, index: 0 }, calls[0].clone())
        .await
        .unwrap();
    assert!(stream.finish().await.unwrap()[0].output.is_error);
    assert_eq!(executions.load(Ordering::SeqCst), 0);
    assert_eq!(control.calls.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn registration_binding_does_not_grant_work_after_create_or_answer() {
    // Reconstructed after an AskUserQuestion answer: still the original selected Skill.
    let control = control(Some(goal_origin()), None);
    let context = ToolContext::new("s1", "u1")
        .with_turn_id("turn-1")
        .with_agent_id("agent-1")
        .with_automation_control(control);
    let guard = guard(AutomationMode::Goal);
    guard.check(&context, "call", "automation").await.unwrap();
    guard
        .check(&context, "call", "AskUserQuestion")
        .await
        .unwrap();
    assert!(guard
        .check(&context, "call", "Bash")
        .await
        .unwrap_err()
        .is_code(ErrorCode::PermissionDenied));
    assert!(guard.check(&context, "call", "Write").await.is_err());
}
#[tokio::test]
async fn admitted_automatic_work_is_allowed_but_missing_authorization_fails_closed() {
    for (mode, origin, latest) in [
        (AutomationMode::Goal, goal_origin(), Some("turn-1")),
        (
            AutomationMode::Loop,
            AutomationTurnOrigin::Loop {
                loop_id: "l".into(),
                occurrence_id: "o".into(),
                config_revision: 1,
            },
            None,
        ),
    ] {
        let context = ToolContext::new("s1", "u1")
            .with_turn_id("turn-1")
            .with_agent_id("agent-1")
            .with_automation_control(control(Some(origin), latest));
        guard(mode).check(&context, "call", "Bash").await.unwrap();
    }
    let context = ToolContext::new("s1", "u1")
        .with_turn_id("turn-1")
        .with_agent_id("agent-1");
    assert!(guard(AutomationMode::Goal)
        .check(&context, "call", "Read")
        .await
        .unwrap_err()
        .is_code(ErrorCode::ServiceUnavailable));
    assert!(RegistrationGuard::from_initial("ordinary work", &[])
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn cleared_bound_goal_preserves_the_original_work_or_registration_authority() {
    for latest in [Some("turn-1"), None] {
        let mut control = control(Some(goal_origin()), latest);
        Arc::get_mut(&mut control)
            .unwrap()
            .result
            .snapshot
            .goal
            .as_mut()
            .unwrap()
            .status = GoalStatus::Cleared;
        let context = ToolContext::new("s1", "u1")
            .with_turn_id("turn-1")
            .with_agent_id("agent-1")
            .with_automation_control(control);
        let result = guard(AutomationMode::Goal)
            .check(&context, "resumed-read", "Write")
            .await;
        if latest.is_some() {
            result.unwrap();
        } else {
            assert!(result.unwrap_err().is_code(ErrorCode::PermissionDenied));
        }
    }
}
