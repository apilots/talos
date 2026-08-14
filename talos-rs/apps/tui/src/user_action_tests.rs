use super::*;
use crate::test_support::{
    completed_event, timeline_event, turn_metrics_event_with_metrics, KeyCode, KeyEvent,
    KeyModifiers, MockGatewayClient,
};
use crate::TuiApp;
use ratatui::{backend::TestBackend, Terminal};
use std::sync::Arc;
use talos_sdk::{
    QuestionOption, QuestionParams, SandboxApprovalParams, SandboxApprovalReason, StopReason,
    TokenUsage, TurnExecutionMetrics,
};

fn pending(params: UserActionParams) -> PendingUserAction {
    PendingUserAction {
        action_id: "action-1".to_string(),
        call_id: "call-1".to_string(),
        item_id: Some("item-1".to_string()),
        state: UserActionState::Pending,
        params,
    }
}

#[test]
fn question_supports_server_option_and_bounded_other_text() {
    let action = pending(UserActionParams::Question(QuestionParams {
        header: "Release".to_string(),
        question: "How should this proceed?".to_string(),
        options: vec![QuestionOption {
            id: "now".to_string(),
            label: "Release now".to_string(),
            description: "Publish immediately".to_string(),
        }],
    }));
    let mut state = PendingActionState::new("turn-1".to_string(), action);
    assert_eq!(
        state.confirm(),
        Some(UserActionResponse::Question(
            QuestionAnswer::SelectedOption {
                option_id: "now".to_string(),
            }
        ))
    );

    state.move_selection(1);
    assert_eq!(state.confirm(), None);
    for value in "Wait".chars() {
        state.insert_char(value);
    }
    assert_eq!(
        state.confirm(),
        Some(UserActionResponse::Question(QuestionAnswer::FreeForm {
            text: "Wait".to_string(),
        }))
    );
}

#[test]
fn approval_only_returns_server_supplied_decisions() {
    let action = pending(UserActionParams::SandboxApproval(SandboxApprovalParams {
        tool_name: "Bash".to_string(),
        reason: SandboxApprovalReason::RiskyCommand,
        presentation: SandboxApprovalPresentation::Command {
            command: "rm output.txt".to_string(),
            cwd: "/workspace".to_string(),
            risk_summary: None,
        },
        available_decisions: vec![
            SandboxApprovalDecision::AllowOnce,
            SandboxApprovalDecision::Deny,
            SandboxApprovalDecision::Cancel,
        ],
    }));
    let mut state = PendingActionState::new("turn-1".to_string(), action);

    assert_eq!(
        state.confirm(),
        Some(UserActionResponse::SandboxApproval(
            SandboxApprovalDecision::AllowOnce
        ))
    );
    state.move_selection(1);
    assert_eq!(
        state.confirm(),
        Some(UserActionResponse::SandboxApproval(
            SandboxApprovalDecision::Deny
        ))
    );
}

#[test]
fn question_panel_renders_options_other_and_bounded_instructions() {
    let state = PendingActionState::new(
        "turn-1".to_string(),
        pending(UserActionParams::Question(QuestionParams {
            header: "Release".to_string(),
            question: "How should this proceed?".to_string(),
            options: vec![QuestionOption {
                id: "now".to_string(),
                label: "Release now".to_string(),
                description: "Publish immediately".to_string(),
            }],
        })),
    );
    let backend = TestBackend::new(80, 20);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| state.render(frame, frame.area()))
        .unwrap();
    let text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();

    assert!(text.contains("Talos needs your input"));
    assert!(text.contains("How should this proceed?"));
    assert!(text.contains("Release now"));
    assert!(text.contains("Other"));
    assert!(text.contains("Esc keeps the action pending"));
}

#[tokio::test]
async fn permissions_command_updates_the_idle_session_through_gateway() {
    let client = Arc::new(MockGatewayClient::new());
    let mut app = TuiApp::new_with_gateway_client(client);
    app.session_id = Some("sess-test".to_string());
    app.current_permission_mode = talos_sdk::UserPermissionMode::AskForApproval;

    app.execute_command("/permissions").await.unwrap();
    app.permission_picker
        .as_mut()
        .expect("permission picker")
        .move_selection(1);
    app.confirm_permission_picker();
    for _ in 0..20 {
        tokio::task::yield_now().await;
        app.poll_command_updates();
        if app.current_permission_mode == talos_sdk::UserPermissionMode::ApproveForMe {
            break;
        }
    }

    assert_eq!(
        app.current_permission_mode,
        talos_sdk::UserPermissionMode::ApproveForMe
    );
}

#[tokio::test]
async fn permissions_command_is_rejected_while_a_turn_is_active() {
    let mut app = TuiApp::build(None);
    app.session_id = Some("sess-test".to_string());
    app.in_flight = true;

    app.execute_command("/permissions").await.unwrap();

    assert!(app.permission_picker.is_none());
}

#[tokio::test]
async fn escape_never_approves_or_cancels_a_pending_action() {
    let client = Arc::new(MockGatewayClient::new());
    let mut app = TuiApp::new_with_gateway_client(client.clone());
    app.session_id = Some("sess-test".to_string());
    app.in_flight = true;
    app.pending_user_action = Some(PendingActionState::new(
        "turn-1".to_string(),
        pending(UserActionParams::SandboxApproval(SandboxApprovalParams {
            tool_name: "Bash".to_string(),
            reason: SandboxApprovalReason::RiskyCommand,
            presentation: SandboxApprovalPresentation::Command {
                command: "rm output.txt".to_string(),
                cwd: "/workspace".to_string(),
                risk_summary: None,
            },
            available_decisions: vec![SandboxApprovalDecision::AllowOnce],
        })),
    ));

    app.handle_input(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .await
        .unwrap();

    assert!(app.pending_user_action.is_some());
    assert!(client
        .state
        .lock()
        .unwrap()
        .submitted_user_actions
        .is_empty());
    assert!(client.state.lock().unwrap().cancelled_sessions.is_empty());
}

#[tokio::test]
async fn submit_uses_the_canonical_request_once_and_disables_the_action() {
    let client = Arc::new(MockGatewayClient::new());
    let mut app = TuiApp::new_with_gateway_client(client.clone());
    app.session_id = Some("sess-test".to_string());
    app.last_stream_sequence = Some(12);
    app.last_stream_event_id = Some("ev_12".to_string());
    app.pending_user_action = Some(PendingActionState::new(
        "turn-1".to_string(),
        pending(UserActionParams::Question(QuestionParams {
            header: "Release".to_string(),
            question: "Proceed?".to_string(),
            options: vec![QuestionOption {
                id: "yes".to_string(),
                label: "Yes".to_string(),
                description: "Continue".to_string(),
            }],
        })),
    ));

    app.submit_pending_user_action();
    app.submit_pending_user_action();
    for _ in 0..20 {
        tokio::task::yield_now().await;
        app.poll_command_updates();
        if !client
            .state
            .lock()
            .unwrap()
            .submitted_user_actions
            .is_empty()
        {
            break;
        }
    }

    let requests = client.state.lock().unwrap().submitted_user_actions.clone();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].session_id, "sess-test");
    assert_eq!(requests[0].turn_id, "turn-1");
    assert_eq!(requests[0].action_id, "action-1");
    assert_eq!(
        app.pending_user_action
            .as_ref()
            .map(|state| state.action.state),
        Some(UserActionState::ResponseAccepted)
    );
    assert_eq!(app.last_stream_sequence, None);
    assert_eq!(app.last_stream_event_id.as_deref(), Some("ev_12"));
}

#[test]
fn timeline_requested_and_finished_events_own_the_action_lifecycle() {
    let mut app = TuiApp::build(None);
    app.session_id = Some("sess-test".to_string());
    let action = pending(UserActionParams::Question(QuestionParams {
        header: "Release".to_string(),
        question: "Proceed?".to_string(),
        options: Vec::new(),
    }));
    let requested = timeline_event(
        "msg-1".to_string(),
        "sess-test".to_string(),
        1,
        talos_sdk::AgentEventType::ItemDelta,
        talos_sdk::AgentEventPayload::ItemDelta(talos_sdk::TalosItemDeltaEvent {
            request_id: "req-1".to_string(),
            turn_id: "turn-test".to_string(),
            item_id: "item-1".to_string(),
            delta: talos_sdk::TalosTimelineDelta::UserAction {
                event: talos_sdk::TimelineUserActionEvent::Requested { action },
            },
        }),
    );
    app.handle_stream_event(requested);
    assert!(app.pending_user_action.is_some());

    let finished = timeline_event(
        "msg-1".to_string(),
        "sess-test".to_string(),
        2,
        talos_sdk::AgentEventType::ItemDelta,
        talos_sdk::AgentEventPayload::ItemDelta(talos_sdk::TalosItemDeltaEvent {
            request_id: "req-1".to_string(),
            turn_id: "turn-test".to_string(),
            item_id: "item-1".to_string(),
            delta: talos_sdk::TalosTimelineDelta::UserAction {
                event: talos_sdk::TimelineUserActionEvent::Finished {
                    action_id: "action-1".to_string(),
                    state: UserActionState::Resolved,
                    response: None,
                },
            },
        }),
    );
    app.handle_stream_event(finished);
    assert!(app.pending_user_action.is_none());
}

#[test]
fn waiting_user_action_completion_preserves_the_reconnect_cursor() {
    let mut app = TuiApp::build(None);
    app.session_id = Some("sess-test".to_string());
    app.pending_user_action = Some(PendingActionState::new(
        "turn-test".to_string(),
        pending(UserActionParams::Question(QuestionParams {
            header: "Release".to_string(),
            question: "Proceed?".to_string(),
            options: Vec::new(),
        })),
    ));

    app.handle_stream_event(completed_event(
        "msg-1",
        "sess-test",
        12,
        talos_sdk::TokenUsage::default(),
    ));

    assert_eq!(app.last_stream_event_id.as_deref(), Some("ev_12"));
    assert_eq!(app.last_stream_sequence, None);
}

#[test]
fn live_metrics_does_not_replace_the_durable_reconnect_cursor() {
    let mut app = TuiApp::build(None);
    app.session_id = Some("sess-test".to_string());
    app.last_stream_event_id = Some("ev_11".to_string());

    let mut metrics = turn_metrics_event_with_metrics(
        "msg-1",
        "sess-test",
        12,
        TurnExecutionMetrics {
            model_rounds: 1,
            tool_calls: 1,
            tool_successes: 1,
            tool_errors: 0,
            stop_reason: StopReason::EndTurn,
            usage: TokenUsage::default(),
            duration_ms: Some(10),
        },
    );
    metrics.event_id = "live_metrics_turn-test_12_100".to_string();
    metrics.event.event_id = Some(metrics.event_id.clone());
    app.handle_stream_event(metrics);

    assert_eq!(app.last_stream_event_id.as_deref(), Some("ev_11"));
}

#[tokio::test]
async fn recovery_restores_the_server_owned_pending_action() {
    let client = Arc::new(MockGatewayClient::new());
    let mut app = TuiApp::new_with_gateway_client(client);
    app.session_id = Some("sess-test".to_string());
    let action = pending(UserActionParams::Question(QuestionParams {
        header: "Release".to_string(),
        question: "Proceed?".to_string(),
        options: Vec::new(),
    }));
    let response = talos_sdk::TimelineRecoveryResponse {
        snapshot: talos_types::TimelineRecoverySnapshot {
            replay: talos_sdk::TimelineReplay {
                events: Vec::new(),
                high_water_mark_event_id: None,
                last_replay_seq: 0,
                cursor_status: talos_sdk::TimelineCursorStatus::Current,
            },
            active_turn: talos_sdk::TimelineActiveTurnSnapshot {
                session_id: "sess-test".to_string(),
                turn_id: Some("turn-1".to_string()),
                message_id: Some("msg-1".to_string()),
                status: talos_sdk::TimelineActiveTurnStatus::WaitingUserAction,
                last_event_id: None,
                pending_user_action: Some(action),
                updated_at: 1,
            },
        },
    };

    app.replay_recovery_snapshot(response);

    assert_eq!(
        app.pending_user_action
            .as_ref()
            .map(|state| state.action.action_id.as_str()),
        Some("action-1")
    );
    app.abort_pending_stream_task();
}
