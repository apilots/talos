use super::*;
use crate::automation::EditTarget;
use talos_sdk::*;

fn goal_fixture() -> SessionGoal {
    SessionGoal {
        id: "goal-1".into(),
        session_id: "sess-test".into(),
        objective: "Finish checks".into(),
        objective_revision: 4,
        status: GoalStatus::Active,
        pause_reason: None,
        reason: None,
        token_budget: Some(100),
        tokens_used: 12,
        latest_turn_id: Some("running-1".into()),
        completion_evidence: None,
        created_at: 1,
        updated_at: 1,
    }
}
fn loop_fixture() -> SessionLoop {
    SessionLoop {
        id: "loop-1".into(),
        session_id: "sess-test".into(),
        prompt: "Check CI".into(),
        schedule: LoopSchedule::ModelDecided {},
        config_revision: 3,
        status: LoopStatus::Active,
        stop_reason: None,
        reason: None,
        next_due_at: None,
        latest_turn_id: None,
        created_at: 1,
        updated_at: 1,
    }
}
async fn connected(
    snapshot: Option<SessionAutomationSnapshot>,
) -> (TuiApp, Arc<MockGatewayClient>) {
    let client = Arc::new(MockGatewayClient::new());
    client.state.lock().unwrap().automation = snapshot;
    let mut app = TuiApp::new_with_gateway_client(client.clone());
    app.initialize_session().await;
    app.session_id = Some("sess-test".into());
    app.ensure_session_subscription().await;
    (app, client)
}
fn snapshot(mode: AutomationMode) -> SessionAutomationSnapshot {
    SessionAutomationSnapshot {
        session_id: "sess-test".into(),
        goal: if mode == AutomationMode::Goal {
            Some(goal_fixture())
        } else {
            None
        },
        loops: if mode == AutomationMode::Loop {
            vec![loop_fixture()]
        } else {
            vec![]
        },
    }
}
fn text(app: &TuiApp) -> String {
    app.render_runtime
        .interactive_projection_lines()
        .iter()
        .map(|line| line.plain_text())
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test]
async fn slash_selection_is_a_protected_draft_consumed_once_and_busy_submission_queues() {
    let (mut app, client) = connected(None).await;
    app.in_flight = true;
    app.active_message = Some("original User task".into());
    app.handle_input(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE))
        .await
        .unwrap();
    app.command_autocomplete_selected = app
        .command_suggestions
        .iter()
        .position(|s| s.name == "loop")
        .unwrap();
    app.handle_input(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .unwrap();
    assert!(app.input_buffer.is_empty());
    assert_eq!(app.selected_skills[0].scope, SkillScope::System);
    assert_eq!(
        app.selected_skills[0].path,
        PathBuf::from("/test/system/loop/SKILL.md")
    );
    for value in "Check CI".chars() {
        app.handle_input(KeyEvent::new(KeyCode::Char(value), KeyModifiers::NONE))
            .await
            .unwrap();
    }
    app.handle_input(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .unwrap();
    assert!(app.selected_skills.is_empty());
    assert!(client.state.lock().unwrap().sent_requests.is_empty());
    assert!(app.message_queue.messages[0]
        .input
        .content
        .iter()
        .any(|c| matches!(c,AgentUserContent::SkillSelection {name,..} if name=="loop")));
    app.input_buffer = "ordinary follow-up".into();
    app.cursor_index = app.input_buffer.len();
    app.submit_input().await.unwrap();
    assert!(!app.message_queue.messages[1]
        .input
        .content
        .iter()
        .any(|c| matches!(c, AgentUserContent::SkillSelection { .. })));
}
#[tokio::test]
async fn goal_and_loop_running_keep_ordinary_send_in_queue_and_explicit_steer_in_same_turn() {
    for mode in [AutomationMode::Goal, AutomationMode::Loop] {
        let (mut app, client) = connected(Some(snapshot(mode))).await;
        client.state.lock().unwrap().active_turn_id = Some("running-1".into());
        app.in_flight = true;
        client.state.lock().unwrap().active_input_origin = Some(AutomationInputOrigin {
            message_id: "running-input".into(),
            origin: match mode {
                AutomationMode::Goal => AutomationTurnOrigin::Goal {
                    goal_id: "goal-1".into(),
                    objective_revision: 4,
                },
                AutomationMode::Loop => AutomationTurnOrigin::Loop {
                    loop_id: "loop-1".into(),
                    occurrence_id: "occurrence".into(),
                    config_revision: 3,
                },
            },
        });
        app.refresh_automation().await;
        assert!(!app.context_compaction_in_flight());
        app.active_stream_turn_id = Some("running-1".into());
        app.input_buffer = "Please inspect this too".into();
        app.cursor_index = app.input_buffer.len();
        app.submit_input().await.unwrap();
        assert_eq!(app.message_queue.messages.len(), 1);
        assert!(client
            .state
            .lock()
            .unwrap()
            .submitted_turn_inputs
            .is_empty());
        app.handle_input(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL))
            .await
            .unwrap();
        app.handle_input(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .await
            .unwrap();
        let state = client.state.lock().unwrap();
        assert_eq!(state.submitted_turn_inputs.len(), 1);
        assert_eq!(state.submitted_turn_inputs[0].identity.turn_id, "running-1");
        assert!(state.sent_requests.is_empty());
        assert_eq!(app.active_stream_turn_id.as_deref(), Some("running-1"));
    }
}
#[tokio::test]
async fn goal_edit_preserves_budget_and_loop_edit_changes_only_later_configuration() {
    let (mut app, client) = connected(Some(snapshot(AutomationMode::Goal))).await;
    app.in_flight = true;
    app.begin_automation_edit(EditTarget::Goal(goal_fixture()));
    app.input_buffer = "Finish checks with a new acceptance criterion".into();
    app.save_automation_edit().await.unwrap();
    let updated = client
        .state
        .lock()
        .unwrap()
        .automation
        .as_ref()
        .unwrap()
        .goal
        .clone()
        .unwrap();
    assert_eq!(updated.token_budget, Some(100));
    assert_eq!(updated.tokens_used, 12);
    assert_eq!(updated.id, "goal-1");
    assert!(app.in_flight);
    assert!(client.state.lock().unwrap().sent_requests.is_empty());
    client.state.lock().unwrap().automation = Some(snapshot(AutomationMode::Loop));
    app.refresh_automation().await;
    app.begin_automation_edit(EditTarget::Loop(loop_fixture()));
    app.handle_input(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
        .await
        .unwrap();
    app.handle_input(KeyEvent::new(KeyCode::Down, KeyModifiers::CONTROL))
        .await
        .unwrap();
    app.save_automation_edit().await.unwrap();
    assert!(
        matches!(&client.state.lock().unwrap().automation_requests.last().unwrap().action,AutomationAction::Loop {action:LoopAction::Edit {loop_id,expected_revision:3,schedule:LoopSchedule::FixedInterval {minutes:1},..}} if loop_id=="loop-1")
    );
    assert!(app.in_flight);
    assert!(client
        .state
        .lock()
        .unwrap()
        .submitted_turn_inputs
        .is_empty());
}
#[tokio::test]
async fn loop_occurrence_is_one_task_row_and_has_no_manual_queue_controls() {
    let (mut app, client) = connected(Some(snapshot(AutomationMode::Loop))).await;
    let message = QueuedMessage {
        id: "auto-occurrence".into(),
        source: QueuedMessageSource::Loop {
            loop_id: "loop-1".into(),
            occurrence_id: "occ-1".into(),
        },
        input: AgentUserInput {
            content: vec![AgentUserContent::Text {
                text: "Check CI".into(),
            }],
        },
    };
    app.message_queue.messages.push(message.clone());
    client.state.lock().unwrap().message_queue = app.message_queue.clone();
    app.tasks_expanded = true;
    let lines = app
        .task_panel_lines(5)
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(lines.matches("↻ Loop").count(), 1);
    assert!(app.task_panel_height() <= 6);
    app.selected_queue_message = Some(0);
    app.begin_selected_queue_edit();
    assert!(app.editing_queue_message_id.is_none());
    app.delete_selected_queued_message().await;
    app.start_or_steer_selected_queued_message().await;
    assert_eq!(
        client.state.lock().unwrap().message_queue.messages,
        vec![message]
    );
    assert!(client
        .state
        .lock()
        .unwrap()
        .submitted_turn_inputs
        .is_empty());
}
#[tokio::test]
async fn failed_activation_and_revision_conflict_keep_the_editable_draft() {
    let (mut app, client) = connected(Some(snapshot(AutomationMode::Goal))).await;
    app.in_flight = true;
    app.active_message = Some("ordinary running task".into());
    client.state.lock().unwrap().message_queue.max_messages = 0;
    app.stage_automation_mode(AutomationMode::Goal)
        .await
        .unwrap();
    app.input_buffer = "an explicit new Goal".into();
    app.cursor_index = app.input_buffer.len();
    app.submit_input().await.unwrap();
    assert_eq!(app.input_buffer, "an explicit new Goal");
    assert_eq!(app.selected_skills[0].name, "goal");
    app.begin_automation_edit(EditTarget::Goal(goal_fixture()));
    app.input_buffer = "changed objective".into();
    client
        .state
        .lock()
        .unwrap()
        .automation
        .as_mut()
        .unwrap()
        .goal
        .as_mut()
        .unwrap()
        .objective_revision = 5;
    assert!(app.save_automation_edit().await.is_err());
    assert_eq!(app.input_buffer, "changed objective");
    assert!(app.automation_edit.is_some());
}
#[tokio::test]
async fn native_job_management_uses_returned_computer_and_instance_without_roles() {
    let (mut app, client) = connected(None).await;
    client
        .state
        .lock()
        .unwrap()
        .builtin_jobs
        .push(BuiltinJobInfo {
            id: BuiltinJobId::ProfileUsageRetention,
            computer_id: "computer-1".into(),
            execution_instance_id: "instance-1".into(),
            name: "Usage retention".into(),
            schedule: "0 0 * * *".into(),
            timezone: "UTC".into(),
            paused: false,
            running: false,
            next_due_at: None,
            last_finished_at: None,
            last_outcome: None,
            available_actions: vec![BuiltinJobAction::Pause, BuiltinJobAction::RunOnce],
        });
    assert!(app
        .manage_builtin_job("other pause profile-usage-retention")
        .await
        .is_err());
    app.manage_builtin_job("computer-1 pause profile-usage-retention")
        .await
        .unwrap();
    app.manage_builtin_job("computer-1 run-once profile-usage-retention")
        .await
        .unwrap();
    let state = client.state.lock().unwrap();
    let job = &state.builtin_jobs[0];
    assert!(job.paused && job.running);
    assert_eq!(job.execution_instance_id, "instance-1");
}

#[tokio::test]
async fn bare_goal_after_dismissing_suggestions_reads_without_activating_a_skill() {
    let (mut app, client) = connected(Some(snapshot(AutomationMode::Goal))).await;
    for value in "/goal".chars() {
        app.handle_input(KeyEvent::new(KeyCode::Char(value), KeyModifiers::NONE))
            .await
            .unwrap();
    }
    app.handle_input(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .await
        .unwrap();
    app.handle_input(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .unwrap();
    assert!(app.selected_skills.is_empty());
    assert!(app.tasks_expanded);
    assert!(client.state.lock().unwrap().automation_requests.is_empty());
    assert!(client.state.lock().unwrap().sent_requests.is_empty());
    assert!(text(&app).contains("goal-1"));
}
#[tokio::test]
async fn bounded_shared_task_rows_render_above_the_original_composer_on_a_small_terminal() {
    use ratatui::{backend::TestBackend, Terminal};
    let (mut app, _) = connected(Some(snapshot(AutomationMode::Loop))).await;
    let single = loop_fixture();
    app.automation_snapshot.as_mut().unwrap().loops = (0..12)
        .map(|i| {
            let mut task = single.clone();
            task.id = format!("loop-{i}");
            task.prompt =
                format!("Task {i} checks a deliberately long target without wrapping the list");
            task
        })
        .collect();
    app.tasks_expanded = true;
    app.selected_automation = Some(11);
    let mut terminal = Terminal::new(TestBackend::new(42, 16)).unwrap();
    terminal.draw(|frame| app.render_ui(frame)).unwrap();
    let rendered = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(rendered.contains("Task 11"));
    assert!(rendered.contains('❯'));
    assert!(app.task_panel_height() <= 6);
    assert_eq!(app.task_panel_lines(5).len(), 5);
}

#[tokio::test]
async fn conflicting_full_command_keeps_the_existing_mode_and_draft() {
    let (mut app, client) = connected(None).await;
    app.stage_automation_mode(AutomationMode::Goal)
        .await
        .unwrap();
    app.input_buffer = "/loop 5m check CI".into();
    app.cursor_index = app.input_buffer.len();
    app.submit_input().await.unwrap();
    assert_eq!(app.input_buffer, "/loop 5m check CI");
    assert_eq!(app.selected_skills[0].name, "goal");
    assert!(client.state.lock().unwrap().sent_requests.is_empty());
    assert!(client
        .state
        .lock()
        .unwrap()
        .message_queue
        .messages
        .is_empty());
}

mod provenance;

#[tokio::test]
async fn clearing_only_the_draft_mode_keeps_prompt_and_running_goal() {
    let (mut app, client) = connected(Some(snapshot(AutomationMode::Goal))).await;
    app.input_buffer = "Keep this draft".into();
    app.stage_automation_mode(AutomationMode::Goal)
        .await
        .unwrap();
    app.handle_input(KeyEvent::new(KeyCode::Backspace, KeyModifiers::ALT))
        .await
        .unwrap();
    assert!(app.selected_skills.is_empty());
    assert_eq!(app.input_buffer, "Keep this draft");
    assert!(client.state.lock().unwrap().automation_requests.is_empty());
    assert_eq!(
        app.automation_snapshot.unwrap().goal.unwrap().status,
        GoalStatus::Active
    );
}
#[tokio::test]
async fn ended_list_exposes_stopped_loop_reason_without_inventing_an_execution() {
    let mut state = snapshot(AutomationMode::Loop);
    state.loops[0].status = LoopStatus::Stopped;
    state.loops[0].stop_reason = Some(LoopStopReason::QueueFull);
    let (mut app, client) = connected(Some(state)).await;
    app.execute_command("/automation ended").await.unwrap();
    assert!(text(&app).contains("QueueFull"));
    assert!(client.state.lock().unwrap().sent_requests.is_empty());
    assert!(!text(&app).contains("↻ Loop"));
}

mod foreground;

mod timing;

#[tokio::test]
async fn full_slash_and_selected_mode_queue_identical_unparsed_prompts() {
    let prompt = "5m 每 10 分钟检查 CI";
    for mode in [AutomationMode::Goal, AutomationMode::Loop] {
        let (mut slash, slash_client) = connected(None).await;
        slash.in_flight = true;
        slash.active_message = Some("ordinary running turn".into());
        slash.input_buffer = format!("/{} {prompt}", mode.skill_name());
        slash.cursor_index = slash.input_buffer.len();
        slash.submit_input().await.unwrap();
        let (mut selected, selected_client) = connected(None).await;
        selected.in_flight = true;
        selected.active_message = Some("ordinary running turn".into());
        selected.stage_automation_mode(mode).await.unwrap();
        selected.input_buffer = prompt.into();
        selected.cursor_index = selected.input_buffer.len();
        selected.submit_input().await.unwrap();
        assert_eq!(
            slash.message_queue.messages[0].input,
            selected.message_queue.messages[0].input
        );
        assert_eq!(
            slash.message_queue.messages[0].input,
            AgentUserInput {
                content: vec![
                    AgentUserContent::Text {
                        text: prompt.into()
                    },
                    AgentUserContent::SkillSelection {
                        name: mode.skill_name().into(),
                        path: format!("/test/system/{}/SKILL.md", mode.skill_name()),
                    },
                ],
            }
        );
        for client in [slash_client, selected_client] {
            let state = client.state.lock().unwrap();
            assert!(state.sent_requests.is_empty());
            assert!(state.automation_requests.is_empty());
        }
        assert!(slash.automation_edit.is_none() && selected.automation_edit.is_none());
    }
}

#[tokio::test]
async fn full_slash_idle_sends_existing_skill_selection_in_original_turn_request() {
    for mode in [AutomationMode::Goal, AutomationMode::Loop] {
        let (mut app, client) = connected(None).await;
        app.input_buffer = format!("/{} Reply READY", mode.skill_name());
        app.cursor_index = app.input_buffer.len();
        app.submit_input().await.unwrap();
        let _stream = app
            .pending_stream_connect
            .take()
            .expect("original stream connection")
            .recv()
            .await
            .unwrap()
            .unwrap();
        let state = client.state.lock().unwrap();
        assert_eq!(state.sent_requests.len(), 1);
        assert_eq!(
            state.sent_requests[0].input,
            AgentUserInput {
                content: vec![
                    AgentUserContent::Text {
                        text: "Reply READY".into()
                    },
                    AgentUserContent::SkillSelection {
                        name: mode.skill_name().into(),
                        path: format!("/test/system/{}/SKILL.md", mode.skill_name()),
                    },
                ],
            }
        );
        assert!(state.automation_requests.is_empty());
        assert!(app.selected_skills.is_empty());
    }
}

#[tokio::test]
async fn completed_goal_leaves_composer_but_remains_readable_and_paused_goal_stays() {
    let mut state = snapshot(AutomationMode::Goal);
    state.goal.as_mut().unwrap().status = GoalStatus::Complete;
    let (mut app, client) = connected(Some(state)).await;
    app.selected_automation = Some(0);
    app.refresh_automation().await;
    assert!(app.automation_targets().is_empty());
    assert_eq!(app.task_panel_height(), 0);
    assert!(app.selected_automation.is_none());
    app.handle_input(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE))
        .await
        .unwrap();
    assert_eq!(app.input_buffer, "x");
    app.execute_command("/goal").await.unwrap();
    assert!(text(&app).contains("Goal goal-1 · Complete"));
    assert_eq!(app.task_panel_height(), 0);
    client
        .state
        .lock()
        .unwrap()
        .automation
        .as_mut()
        .unwrap()
        .goal
        .as_mut()
        .unwrap()
        .status = GoalStatus::Paused;
    app.refresh_automation().await;
    assert!(
        matches!(app.automation_targets().as_slice(), [EditTarget::Goal(goal)] if goal.status == GoalStatus::Paused)
    );
    assert!(app.task_panel_height() > 0);
}
