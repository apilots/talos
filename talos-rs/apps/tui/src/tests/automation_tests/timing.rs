use super::*;
use crate::frame_fingerprint::tui_frame_fingerprint_at;

#[test]
fn goal_time_includes_pause_and_survives_turn_changes() {
    let mut app = TuiApp::build(None);
    let mut goal = goal_fixture();
    goal.created_at = 100;
    goal.updated_at = 110;
    goal.status = GoalStatus::Paused;
    app.automation_snapshot = Some(SessionAutomationSnapshot {
        session_id: "sess-test".into(),
        goal: Some(goal.clone()),
        loops: vec![],
    });
    assert_eq!(
        app.automation_task_time(&EditTarget::Goal(goal.clone()), 165),
        "1m 05s"
    );
    assert_ne!(
        tui_frame_fingerprint_at(&app, 165),
        tui_frame_fingerprint_at(&app, 166)
    );
    goal.status = GoalStatus::Active;
    goal.latest_turn_id = Some("next-turn".into());
    assert_eq!(
        app.automation_task_time(&EditTarget::Goal(goal), 166),
        "1m 06s"
    );
}

#[test]
fn completed_goal_uses_stable_completion_update_and_stops_clock_redraws() {
    let mut app = TuiApp::build(None);
    let mut goal = goal_fixture();
    goal.created_at = 100;
    goal.updated_at = 172;
    goal.status = GoalStatus::Complete;
    app.automation_snapshot = Some(SessionAutomationSnapshot {
        session_id: "sess-test".into(),
        goal: Some(goal.clone()),
        loops: vec![],
    });
    assert_eq!(
        app.automation_task_time(&EditTarget::Goal(goal.clone()), 300),
        "1m 12s"
    );
    assert_eq!(
        app.automation_task_time(&EditTarget::Goal(goal), 400),
        "1m 12s"
    );
    assert_eq!(
        tui_frame_fingerprint_at(&app, 300),
        tui_frame_fingerprint_at(&app, 400)
    );
}

#[test]
fn loop_clock_uses_exact_admitted_input_and_does_not_reset_on_steer() {
    let mut app = TuiApp::build(None);
    let task = EditTarget::Loop(loop_fixture());
    app.in_flight = true;
    app.active_stream_turn_id = Some("loop-turn".into());
    app.render_runtime.set_automation_input(
        "loop-turn",
        AutomationInputOrigin {
            message_id: "loop-input".into(),
            origin: AutomationTurnOrigin::Loop {
                loop_id: "loop-1".into(),
                occurrence_id: "occ-1".into(),
                config_revision: 3,
            },
        },
    );
    let mut input = completed_text_message("loop-input", AgentMessageRole::User, "Check CI");
    input.created_at = 100;
    app.render_runtime
        .apply_turn_event_payload("loop-turn", AgentEventPayload::MessageCompleted(input));
    assert_eq!(app.automation_task_time(&task, 105), "5s");
    let mut steer = completed_text_message("ordinary-steer", AgentMessageRole::User, "More detail");
    steer.created_at = 105;
    app.render_runtime
        .apply_turn_event_payload("loop-turn", AgentEventPayload::MessageCompleted(steer));
    assert_eq!(app.automation_task_time(&task, 106), "6s");
    app.active_stream_turn_id = Some("human-turn".into());
    assert_eq!(app.automation_task_time(&task, 107), "model-decided");
}

#[test]
fn loop_idle_next_and_stopped_do_not_claim_a_running_timer() {
    let app = TuiApp::build(None);
    let mut task = loop_fixture();
    task.next_due_at = Some(1000);
    let next = app.automation_task_time(&EditTarget::Loop(task.clone()), 500);
    assert!(next.starts_with("next "));
    assert_eq!(
        next,
        app.automation_task_time(&EditTarget::Loop(task.clone()), 501)
    );
    task.next_due_at = None;
    task.schedule = LoopSchedule::FixedInterval { minutes: 5 };
    assert_eq!(
        app.automation_task_time(&EditTarget::Loop(task.clone()), 501),
        "no next time"
    );
    task.status = LoopStatus::Stopped;
    assert_eq!(
        app.automation_task_time(&EditTarget::Loop(task), 501),
        "ended"
    );
}
