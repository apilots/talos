use super::*;

#[test]
fn exact_input_source_is_not_inferred_for_later_user_messages_and_native_lines_are_not_reprinted() {
    let mut app = TuiApp::build(None);
    let mut initial = completed_text_message("input-1", AgentMessageRole::User, "Do work");
    initial.turn_id = "turn-1".into();
    app.render_runtime
        .apply_turn_event_payload("turn-1", AgentEventPayload::MessageCompleted(initial));
    let ids = app
        .render_runtime
        .pending_terminal_projection()
        .iter()
        .map(|block| block.block_id)
        .collect::<Vec<_>>();
    app.render_runtime.mark_terminal_blocks_synced(ids);
    app.render_runtime.set_automation_input(
        "turn-1",
        AutomationInputOrigin {
            message_id: "input-1".into(),
            origin: AutomationTurnOrigin::Goal {
                goal_id: "g".into(),
                objective_revision: 1,
            },
        },
    );
    assert!(app.render_runtime.pending_terminal_projection().is_empty());
    let mut steer = completed_text_message(
        "input-2",
        AgentMessageRole::User,
        "ordinary Steer about Goal",
    );
    steer.turn_id = "turn-1".into();
    app.render_runtime
        .apply_turn_event_payload("turn-1", AgentEventPayload::MessageCompleted(steer));
    assert_eq!(text(&app).matches("◎ Goal").count(), 1);
    assert!(!app
        .render_runtime
        .pending_terminal_projection()
        .iter()
        .flat_map(|b| b.lines.iter())
        .any(|line| line.plain_text().contains("◎ Goal")));
}
#[test]
fn saved_origin_renders_on_first_projection_and_history_reload_targets_only_exact_input() {
    let mut app = TuiApp::build(None);
    let origin = AutomationInputOrigin {
        message_id: "auto-input".into(),
        origin: AutomationTurnOrigin::Loop {
            loop_id: "l".into(),
            occurrence_id: "o".into(),
            config_revision: 1,
        },
    };
    app.render_runtime
        .set_automation_input("turn-1", origin.clone());
    app.render_runtime.apply_turn_event_payload(
        "turn-1",
        AgentEventPayload::MessageCompleted(completed_text_message(
            "auto-input",
            AgentMessageRole::User,
            "Check CI",
        )),
    );
    assert!(app
        .render_runtime
        .pending_terminal_projection()
        .iter()
        .flat_map(|b| b.lines.iter())
        .any(|line| line.plain_text().contains("↻ Loop")));
    let mut history = completed_history_response(
        vec![
            completed_text_message("auto-input", AgentMessageRole::User, "Check CI"),
            completed_text_message("steer", AgentMessageRole::User, "User asks about Loop"),
        ],
        None,
    );
    history.data[0].automation_input = Some(origin);
    app.render_runtime.reset_transcript();
    app.replay_history_snapshot(history);
    assert_eq!(text(&app).matches("↻ Loop").count(), 1);
}
#[tokio::test]
async fn automation_metadata_invalidation_refreshes_same_offset_and_ignores_other_sessions() {
    let (mut app, client) = connected(Some(snapshot(AutomationMode::Goal))).await;
    app.in_flight = true;
    app.last_stream_event_id = Some("ev_42".into());
    client.state.lock().unwrap().active_turn_id = Some("running-1".into());
    client.state.lock().unwrap().active_input_origin = Some(AutomationInputOrigin {
        message_id: "input".into(),
        origin: AutomationTurnOrigin::Goal {
            goal_id: "g".into(),
            objective_revision: 1,
        },
    });
    app.render_runtime.apply_turn_event_payload(
        "running-1",
        AgentEventPayload::MessageCompleted(completed_text_message(
            "input",
            AgentMessageRole::User,
            "Work",
        )),
    );
    let senders = client
        .state
        .lock()
        .unwrap()
        .message_queue_subscribers
        .clone();
    for sender in &senders {
        sender
            .send(crate::client::SessionEvent::AutomationChanged(
                AutomationChangedNotification {
                    session_id: "other".into(),
                },
            ))
            .await
            .unwrap();
    }
    app.poll_session_events().await;
    assert!(!text(&app).contains("◎ Goal"));
    for sender in &senders {
        sender
            .send(crate::client::SessionEvent::AutomationChanged(
                AutomationChangedNotification {
                    session_id: "sess-test".into(),
                },
            ))
            .await
            .unwrap();
    }
    app.poll_session_events().await;
    assert!(text(&app).contains("◎ Goal"));
    assert_eq!(app.last_stream_event_id.as_deref(), Some("ev_42"));
}

#[tokio::test]
async fn automation_notification_reconciles_unqueued_work_after_registration_and_fast_completion() {
    for mode in [AutomationMode::Goal, AutomationMode::Loop] {
        let (mut app, client) = connected(Some(snapshot(mode))).await;
        app.in_flight = true;
        {
            let mut state = client.state.lock().unwrap();
            state.active_turn_id = Some("turn-test".into());
            state.reconnect_events = vec![
                turn_started_event("sess-test", 1),
                assistant_block_started_event("answer", "sess-test", 2),
                assistant_block_completed_event("answer", "sess-test", 3, "AUTOMATIC_WORK"),
                turn_completed_event("sess-test", 4),
            ];
        }
        let senders = client
            .state
            .lock()
            .unwrap()
            .message_queue_subscribers
            .clone();
        for sender in &senders {
            sender
                .send(crate::client::SessionEvent::AutomationChanged(
                    AutomationChangedNotification {
                        session_id: "sess-test".into(),
                    },
                ))
                .await
                .unwrap();
        }
        app.poll_session_events().await;
        assert!(app.message_queue_turn_reconcile_pending);
        assert_eq!(client.state.lock().unwrap().reconnect_calls, 0);
        app.finish_active_stream("registration completed", false, false);
        app.poll_session_events().await;
        assert!(app.in_flight && app.pending_stream.is_some());
        assert_eq!(
            client.state.lock().unwrap().reconnect_since_event_ids,
            vec![None]
        );
        assert!(app.message_queue.messages.is_empty());
        app.poll_stream_events();
        assert!(text(&app).contains("AUTOMATIC_WORK"));
        assert!(!app.in_flight);

        {
            let mut state = client.state.lock().unwrap();
            state.active_turn_id = None;
            state.session_history_turns = completed_history_response(
                vec![completed_text_message(
                    "fast-answer",
                    AgentMessageRole::Agent,
                    "FAST_AUTOMATIC_WORK",
                )],
                None,
            )
            .data;
        }
        for sender in &senders {
            sender
                .send(crate::client::SessionEvent::AutomationChanged(
                    AutomationChangedNotification {
                        session_id: "sess-test".into(),
                    },
                ))
                .await
                .unwrap();
        }
        app.poll_session_events().await;
        assert!(text(&app).contains("FAST_AUTOMATIC_WORK"));
        assert!(!app.in_flight && !app.message_queue_turn_reconcile_pending);
        let state = client.state.lock().unwrap();
        assert_eq!(state.reconnect_calls, 1);
        assert!(state.sent_requests.is_empty() && state.automation_requests.is_empty());
    }
}
