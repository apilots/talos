use super::*;

fn app_with_client() -> (TuiApp, Arc<MockGatewayClient>) {
    let client = Arc::new(MockGatewayClient::new());
    let gateway_client: Arc<dyn GatewayClient> = client.clone();
    (TuiApp::new_with_gateway_client(gateway_client), client)
}

async fn connected_app() -> (TuiApp, Arc<MockGatewayClient>) {
    let (mut app, client) = app_with_client();
    app.initialize_session().await;
    app.session_id = Some("sess-test".to_string());
    app.ensure_message_queue_subscription().await;
    (app, client)
}

#[tokio::test]
async fn queue_uses_server_capacity_and_preserves_fifo() {
    let mut app = create_app_server_app();
    app.initialize_session().await;

    assert!(app.send_user_message("m1".to_string()).await.unwrap());
    for index in 2..=11 {
        assert!(app.send_user_message(format!("m{index}")).await.unwrap());
    }
    assert!(!app.send_user_message("m12".to_string()).await.unwrap());

    assert_eq!(app.message_queue.messages.len(), 10);
    assert!(app.in_flight);
    let queued_text = app
        .message_queue
        .messages
        .iter()
        .map(crate::message_queue::queued_message_text)
        .collect::<Vec<_>>();
    assert_eq!(
        queued_text,
        (2..=11)
            .map(|index| format!("m{index}"))
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn queue_selection_updates_reorders_and_deletes_server_messages() {
    let (mut app, _) = connected_app().await;
    app.in_flight = true;
    app.active_message = Some("active turn".to_string());
    assert!(
        app.enqueue_message("first".to_string(), Vec::new(), vec!["file-1".to_string()])
            .await
    );
    assert!(
        app.enqueue_message("second".to_string(), Vec::new(), Vec::new())
            .await
    );

    app.handle_input(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL))
        .await
        .unwrap();
    app.handle_input(KeyEvent::new(KeyCode::Down, KeyModifiers::CONTROL))
        .await
        .unwrap();
    app.handle_input(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE))
        .await
        .unwrap();
    app.input_buffer = "updated".to_string();
    app.cursor_index = app.input_buffer.chars().count();
    app.handle_input(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .unwrap();

    assert_eq!(
        app.message_queue.messages,
        vec![
            QueuedMessage {
                id: "q2".to_string(),
                input: vec![TalosInputItem::Text {
                    text: "second".to_string(),
                }],
            },
            QueuedMessage {
                id: "q1".to_string(),
                input: vec![
                    TalosInputItem::Text {
                        text: "updated".to_string(),
                    },
                    TalosInputItem::Files {
                        file_ids: vec!["file-1".to_string()],
                    },
                ],
            },
        ]
    );

    app.handle_input(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL))
        .await
        .unwrap();
    app.handle_input(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE))
        .await
        .unwrap();
    assert_eq!(
        app.message_queue.messages,
        vec![QueuedMessage {
            id: "q1".to_string(),
            input: vec![
                TalosInputItem::Text {
                    text: "updated".to_string(),
                },
                TalosInputItem::Files {
                    file_ids: vec!["file-1".to_string()],
                },
            ],
        }]
    );
}

#[tokio::test]
async fn steer_rejects_skill_and_interrupt_keeps_the_queue() {
    let (mut app, client) = connected_app().await;
    app.in_flight = true;
    client.state.lock().unwrap().active_turn_id = Some("turn-active".to_string());
    assert!(
        app.enqueue_message("steer me".to_string(), Vec::new(), Vec::new())
            .await
    );
    assert!(
        app.enqueue_message(
            "skill later".to_string(),
            vec![SkillSuggestion {
                name: "review".to_string(),
                description: "Review changes".to_string(),
                short_description: None,
                root: PathBuf::from("/skills/review"),
                path: PathBuf::from("/skills/review/SKILL.md"),
                scope: talos_sdk::SkillScope::External,
                source_plugin: None,
                source_install: None,
                metadata: None,
            }],
            Vec::new(),
        )
        .await
    );

    app.select_message_queue();
    app.start_or_steer_selected_queued_message().await;
    app.select_message_queue();
    app.start_or_steer_selected_queued_message().await;
    app.selected_queue_message = None;
    app.cancel_active_turn().await;

    assert_eq!(
        app.message_queue.messages,
        vec![QueuedMessage {
            id: "q2".to_string(),
            input: vec![
                TalosInputItem::Text {
                    text: "skill later".to_string(),
                },
                TalosInputItem::SkillSelection {
                    name: "review".to_string(),
                    path: "/skills/review/SKILL.md".to_string(),
                },
            ],
        }]
    );
}

#[tokio::test]
async fn start_removes_only_the_selected_server_message() {
    let (mut app, client) = connected_app().await;
    assert!(
        app.enqueue_message("first".to_string(), Vec::new(), Vec::new())
            .await
    );
    assert!(
        app.enqueue_message("second".to_string(), Vec::new(), Vec::new())
            .await
    );
    client.state.lock().unwrap().reconnect_events = vec![started_event("msg-queued", 1)];

    app.select_message_queue();
    app.move_message_queue_selection(1);
    app.start_or_steer_selected_queued_message().await;

    assert_eq!(
        app.message_queue.messages,
        vec![QueuedMessage {
            id: "q1".to_string(),
            input: vec![TalosInputItem::Text {
                text: "first".to_string(),
            }],
        }]
    );
    assert!(app.in_flight);
}

#[tokio::test]
async fn changed_refreshes_and_disconnect_clears_the_projection() {
    let (mut app, client) = connected_app().await;
    client
        .add_queued_message(talos_sdk::MessageQueueAddRequest {
            session_id: "sess-test".to_string(),
            input: vec![TalosInputItem::Text {
                text: "from web".to_string(),
            }],
        })
        .await
        .unwrap();

    app.poll_message_queue_changes().await;
    assert_eq!(app.message_queue.revision, 1);
    assert_eq!(app.message_queue.messages.len(), 1);

    client
        .state
        .lock()
        .unwrap()
        .message_queue_subscribers
        .clear();
    app.poll_message_queue_changes().await;
    assert_eq!(
        app.message_queue,
        MessageQueue {
            revision: 0,
            max_messages: 0,
            messages: Vec::new(),
        }
    );
}

#[tokio::test]
async fn auto_dispatched_queue_change_connects_the_new_turn_stream() {
    let (mut app, client) = connected_app().await;
    assert!(
        app.enqueue_message("first".to_string(), Vec::new(), Vec::new())
            .await
    );
    assert!(
        app.enqueue_message("second".to_string(), Vec::new(), Vec::new())
            .await
    );
    app.in_flight = true;
    app.finish_active_stream("stream completed", false, false);
    {
        let mut state = client.state.lock().unwrap();
        state.active_turn_id = Some("turn-auto".to_string());
        state.reconnect_events = vec![started_event("msg-auto", 1)];
    }
    client
        .delete_queued_message(talos_sdk::MessageQueueDeleteRequest {
            session_id: "sess-test".to_string(),
            message_id: "q1".to_string(),
        })
        .await
        .unwrap();

    app.poll_message_queue_changes().await;

    assert_eq!(
        (
            app.in_flight,
            app.message_queue.messages,
            client
                .state
                .lock()
                .unwrap()
                .reconnect_since_event_ids
                .clone(),
        ),
        (
            true,
            vec![QueuedMessage {
                id: "q2".to_string(),
                input: vec![TalosInputItem::Text {
                    text: "second".to_string(),
                }],
            }],
            vec![None],
        )
    );
}

#[tokio::test]
async fn fast_queue_turn_reconciles_history_after_the_previous_stream_finishes() {
    let (mut app, client) = connected_app().await;
    assert!(
        app.enqueue_message("first".to_string(), Vec::new(), Vec::new())
            .await
    );
    assert!(
        app.enqueue_message("second".to_string(), Vec::new(), Vec::new())
            .await
    );
    app.in_flight = true;
    client.state.lock().unwrap().session_history_messages = vec![SessionHistoryMessage {
        message_id: "msg-auto".to_string(),
        turn_id: "turn-auto".to_string(),
        items: vec![TalosTimelineItem {
            item_id: "item-auto-answer".to_string(),
            parent_item_id: None,
            kind: TalosTimelineItemKind::AssistantText,
            phase: TalosTimelinePhase::FinalAnswer,
            status: TalosTimelineItemStatus::Completed,
            title: None,
            started_at_ms: 1,
            completed_at_ms: Some(2),
            content: TalosTimelineContent::Text {
                text: "fast queued answer".to_string(),
            },
        }],
        stop_reason: StopReason::EndTurn,
        usage: TokenUsage::default(),
        metrics: None,
    }];
    client
        .delete_queued_message(talos_sdk::MessageQueueDeleteRequest {
            session_id: "sess-test".to_string(),
            message_id: "q1".to_string(),
        })
        .await
        .unwrap();

    app.poll_message_queue_changes().await;
    assert!(app.message_queue_turn_reconcile_pending);

    app.finish_active_stream("stream completed", false, false);
    app.poll_message_queue_changes().await;

    let transcript = app
        .render_runtime
        .store()
        .pending_terminal_projection()
        .into_iter()
        .flat_map(|block| block.lines)
        .map(|line| line.plain_text())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!app.message_queue_turn_reconcile_pending);
    assert!(!app.in_flight);
    assert!(transcript.contains("fast queued answer"));
}

fn app_with_rendered_queue(input: Vec<TalosInputItem>) -> TuiApp {
    let mut app = create_embedded_app();
    app.message_queue = MessageQueue {
        revision: 2,
        max_messages: 10,
        messages: vec![QueuedMessage {
            id: "q1".to_string(),
            input,
        }],
    };
    app
}

#[test]
fn waiting_messages_default_state_has_no_action_hint() {
    let app = app_with_rendered_queue(vec![TalosInputItem::Text {
        text: "Review the current patch".to_string(),
    }]);

    assert_eq!(
        app.queue_panel_lines(4)
            .iter()
            .map(line_plain_text)
            .collect::<Vec<_>>(),
        vec![
            "Waiting messages 1/10".to_string(),
            "  1. Review the current patch".to_string(),
        ]
    );
}

#[test]
fn waiting_messages_active_selection_shows_steer_keys() {
    let mut app = app_with_rendered_queue(vec![TalosInputItem::Text {
        text: "Review the current patch".to_string(),
    }]);
    app.in_flight = true;
    app.selected_queue_message = Some(0);

    assert_eq!(
        app.queue_panel_lines(4)
            .iter()
            .map(line_plain_text)
            .collect::<Vec<_>>(),
        vec![
            "Waiting messages 1/10".to_string(),
            "❯ 1. Review the current patch".to_string(),
            "Enter Steer  e Edit  d Delete  Ctrl+Up/Down Move  Esc Close".to_string(),
        ]
    );
}

#[test]
fn waiting_messages_idle_selection_shows_start_keys() {
    let mut app = app_with_rendered_queue(vec![TalosInputItem::Text {
        text: "Continue from the queue".to_string(),
    }]);
    app.selected_queue_message = Some(0);

    assert_eq!(
        app.queue_panel_lines(4)
            .iter()
            .map(line_plain_text)
            .collect::<Vec<_>>(),
        vec![
            "Waiting messages 1/10".to_string(),
            "❯ 1. Continue from the queue".to_string(),
            "Enter Start  e Edit  d Delete  Ctrl+Up/Down Move  Esc Close".to_string(),
        ]
    );
}

#[test]
fn waiting_messages_skill_selection_omits_steer_key() {
    let mut app = app_with_rendered_queue(vec![
        TalosInputItem::Text {
            text: "Run the review skill".to_string(),
        },
        TalosInputItem::SkillSelection {
            name: "review".to_string(),
            path: "/skills/review/SKILL.md".to_string(),
        },
    ]);
    app.in_flight = true;
    app.selected_queue_message = Some(0);

    assert_eq!(
        app.queue_panel_lines(4)
            .iter()
            .map(line_plain_text)
            .collect::<Vec<_>>(),
        vec![
            "Waiting messages 1/10".to_string(),
            "❯ 1. Run the review skill".to_string(),
            "e Edit  d Delete  Ctrl+Up/Down Move  Esc Close".to_string(),
        ]
    );
}
