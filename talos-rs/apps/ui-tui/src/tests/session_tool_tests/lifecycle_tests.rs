use super::*;

#[tokio::test]
async fn test_closing_state_blocks_new_messages() {
    let mut app = create_remote_app_with_close_delay(5_000);
    app.initialize_remote().await;
    app.close_remote_session_command().unwrap();

    let accepted = app.send_user_message("blocked".to_string()).await.unwrap();

    assert!(!accepted);
    assert!(app.message_queue.is_empty());
    assert!(app
        .status_hint
        .as_ref()
        .map(|(msg, _)| msg.contains("closing"))
        .unwrap_or(false));
}

// ── Phase 3: tool call inline cards ──────────────────────────────────

#[test]
fn test_tool_call_block_pushed_on_started_event() {
    let mut app = create_remote_app();
    app.session_id = Some("sess".to_string());

    let event = TalosEvent {
        event_type: TalosEventType::MessageToolCallStarted,
        message_id: "msg1".to_string(),
        session_id: "sess".to_string(),
        sequence: 1,
        payload: TalosEventPayload::MessageToolCallStarted(
            talos_client::TalosMessageToolCallStartedEvent {
                request_id: "req1".to_string(),
                call_id: "call-1".to_string(),
                name: "bash".to_string(),
                arguments: serde_json::json!({"command": "ls"}).into(),
                item_index: 0,
                iteration: 1,
                step_id: "step-1".to_string(),
                timing_ms: None,
            },
        ),
    };

    app.handle_stream_event(event);

    let tool_block = app.chat_blocks.iter().find(|b| b.role == ChatRole::Tool);
    assert!(tool_block.is_some(), "expected tool block to be added");
    let block = tool_block.unwrap();
    assert_eq!(block.tool_name.as_deref(), Some("bash"));
    assert_eq!(block.tool_status, Some(ToolCallStatus::Queued));
    assert!(app.transcript.active_tool_calls.contains_key("call-1"));
}

#[test]
fn test_tool_call_block_completed_on_event() {
    let mut app = create_remote_app();
    app.session_id = Some("sess".to_string());

    // First push a started event
    let started = TalosEvent {
        event_type: TalosEventType::MessageToolCallStarted,
        message_id: "msg1".to_string(),
        session_id: "sess".to_string(),
        sequence: 1,
        payload: TalosEventPayload::MessageToolCallStarted(
            talos_client::TalosMessageToolCallStartedEvent {
                request_id: "req1".to_string(),
                call_id: "call-2".to_string(),
                name: "read".to_string(),
                arguments: serde_json::json!({"path": "/tmp/x"}).into(),
                item_index: 0,
                iteration: 1,
                step_id: "step-1".to_string(),
                timing_ms: None,
            },
        ),
    };
    app.handle_stream_event(started);

    let completed = TalosEvent {
        event_type: TalosEventType::MessageToolCallCompleted,
        message_id: "msg1".to_string(),
        session_id: "sess".to_string(),
        sequence: 2,
        payload: TalosEventPayload::MessageToolCallCompleted(
            talos_client::TalosMessageToolCallCompletedEvent {
                request_id: "req1".to_string(),
                call_id: "call-2".to_string(),
                tool: "read".to_string(),
                content: vec![talos_client::TalosContentItem::Text {
                    text: "file contents".to_string(),
                }],
                is_error: false,
                item_index: 0,
                iteration: 1,
                step_id: "step-1".to_string(),
                timing_ms: Some(42),
            },
        ),
    };
    app.handle_stream_event(completed);

    let tool_block = app.chat_blocks.iter().find(|b| b.role == ChatRole::Tool);
    assert!(tool_block.is_some());
    let block = tool_block.unwrap();
    assert_eq!(
        block.tool_status,
        Some(ToolCallStatus::Completed { exit_ok: true })
    );
    assert_eq!(block.content, "file contents");
    assert_eq!(block.timing_ms, Some(42));
    // Should be removed from active after completion
    assert!(!app.transcript.active_tool_calls.contains_key("call-2"));
}

#[test]
fn test_tool_completed_event_with_different_message_id_is_ignored() {
    let mut app = create_remote_app();
    app.session_id = Some("sess".to_string());
    app.active_stream_message_id = Some("msg-active".to_string());
    app.last_stream_sequence = Some(10);
    app.push_tool_call_block(
        "call-stale".to_string(),
        "MemorySearch".to_string(),
        serde_json::json!({"query": "talos 进展"}),
        1,
        0,
        None,
    );

    let completed = TalosEvent {
        event_type: TalosEventType::MessageToolCallCompleted,
        message_id: "msg-other".to_string(),
        session_id: "sess".to_string(),
        sequence: 11,
        payload: TalosEventPayload::MessageToolCallCompleted(
            talos_client::TalosMessageToolCallCompletedEvent {
                request_id: "req1".to_string(),
                call_id: "call-stale".to_string(),
                tool: "MemorySearch".to_string(),
                content: vec![talos_client::TalosContentItem::Json {
                    value: serde_json::json!({"results": []}).into(),
                }],
                is_error: false,
                item_index: 0,
                iteration: 1,
                step_id: "step-1".to_string(),
                timing_ms: Some(120),
            },
        ),
    };

    app.handle_stream_event(completed);

    let idx = app
        .chat_blocks
        .iter()
        .position(|b| b.tool_call_id.as_deref() == Some("call-stale"))
        .unwrap();
    assert_eq!(
        app.chat_blocks[idx].tool_status,
        Some(ToolCallStatus::Queued)
    );
    assert_eq!(app.chat_blocks[idx].timing_ms, None);
    assert!(app.transcript.active_tool_calls.contains_key("call-stale"));
    assert_eq!(app.last_stream_sequence, Some(10));
}

#[test]
fn test_tool_completed_event_with_older_sequence_is_ignored() {
    let mut app = create_remote_app();
    app.session_id = Some("sess".to_string());
    app.active_stream_message_id = Some("msg-active".to_string());
    app.last_stream_sequence = Some(20);
    app.push_tool_call_block(
        "call-older".to_string(),
        "bash".to_string(),
        serde_json::json!({"cmd": "pwd"}),
        1,
        0,
        None,
    );

    let completed = TalosEvent {
        event_type: TalosEventType::MessageToolCallCompleted,
        message_id: "msg-active".to_string(),
        session_id: "sess".to_string(),
        sequence: 5,
        payload: TalosEventPayload::MessageToolCallCompleted(
            talos_client::TalosMessageToolCallCompletedEvent {
                request_id: "req1".to_string(),
                call_id: "call-older".to_string(),
                tool: "bash".to_string(),
                content: vec![talos_client::TalosContentItem::Json {
                    value: serde_json::json!({
                        "session_id": null,
                        "status": "exited",
                        "output": "/tmp\n",
                        "exit_code": 0,
                        "truncated": false
                    })
                    .into(),
                }],
                is_error: false,
                item_index: 0,
                iteration: 1,
                step_id: "step-1".to_string(),
                timing_ms: Some(15),
            },
        ),
    };

    app.handle_stream_event(completed);

    let idx = app
        .chat_blocks
        .iter()
        .position(|b| b.tool_call_id.as_deref() == Some("call-older"))
        .unwrap();
    assert_eq!(
        app.chat_blocks[idx].tool_status,
        Some(ToolCallStatus::Queued)
    );
    assert!(app.transcript.active_tool_calls.contains_key("call-older"));
    assert_eq!(app.last_stream_sequence, Some(20));
}

#[test]
fn test_tool_panel_events_are_upserted_per_call_id() {
    let mut app = create_remote_app();
    app.session_id = Some("sess".to_string());

    app.handle_stream_event(TalosEvent {
        event_type: TalosEventType::MessageToolCallStarted,
        message_id: "msg-tools".to_string(),
        session_id: "sess".to_string(),
        sequence: 1,
        payload: TalosEventPayload::MessageToolCallStarted(
            talos_client::TalosMessageToolCallStartedEvent {
                request_id: "req1".to_string(),
                call_id: "call-tools".to_string(),
                name: "bash".to_string(),
                arguments: serde_json::json!({"cmd": "pwd"}).into(),
                item_index: 0,
                iteration: 1,
                step_id: "step-1".to_string(),
                timing_ms: None,
            },
        ),
    });
    app.handle_stream_event(TalosEvent {
        event_type: TalosEventType::MessageToolCallQueued,
        message_id: "msg-tools".to_string(),
        session_id: "sess".to_string(),
        sequence: 2,
        payload: TalosEventPayload::MessageToolCallQueued(
            talos_types::talos::TalosMessageToolCallQueuedEvent {
                request_id: "req1".to_string(),
                stream_key: talos_types::talos::TalosToolStreamKey { round: 0, index: 0 },
                call_id: "call-tools".to_string(),
                tool_name: "bash".to_string(),
                item_index: 0,
                iteration: 1,
                step_id: "step-1".to_string(),
            },
        ),
    });
    app.handle_stream_event(TalosEvent {
        event_type: TalosEventType::MessageToolCallExecuting,
        message_id: "msg-tools".to_string(),
        session_id: "sess".to_string(),
        sequence: 3,
        payload: TalosEventPayload::MessageToolCallExecuting(
            talos_types::talos::TalosMessageToolCallExecutingEvent {
                request_id: "req1".to_string(),
                stream_key: talos_types::talos::TalosToolStreamKey { round: 0, index: 0 },
                call_id: "call-tools".to_string(),
                tool_name: "bash".to_string(),
                item_index: 0,
                iteration: 1,
                step_id: "step-1".to_string(),
            },
        ),
    });
    app.handle_stream_event(TalosEvent {
        event_type: TalosEventType::MessageToolCallCompleted,
        message_id: "msg-tools".to_string(),
        session_id: "sess".to_string(),
        sequence: 4,
        payload: TalosEventPayload::MessageToolCallCompleted(
            talos_client::TalosMessageToolCallCompletedEvent {
                request_id: "req1".to_string(),
                call_id: "call-tools".to_string(),
                tool: "bash".to_string(),
                content: vec![TalosContentItem::Text {
                    text: "/tmp".to_string(),
                }],
                is_error: false,
                item_index: 0,
                iteration: 1,
                step_id: "step-1".to_string(),
                timing_ms: Some(8),
            },
        ),
    });

    assert_eq!(app.tool_events.len(), 1);
    assert_eq!(app.tool_events[0].call_id, "call-tools");
    assert_eq!(app.tool_events[0].summary, "done: Bash");

    let tool_block = app
        .chat_blocks
        .iter()
        .find(|block| block.tool_call_id.as_deref() == Some("call-tools"))
        .expect("tool block should exist");
    assert_eq!(
        tool_block.tool_input,
        Some(serde_json::json!({"cmd": "pwd"}).into())
    );
}

#[test]
fn test_tool_progress_event_appends_to_existing_tool_block() {
    let mut app = create_remote_app();
    app.session_id = Some("sess".to_string());

    app.handle_stream_event(TalosEvent {
        event_type: TalosEventType::MessageToolCallStarted,
        message_id: "msg-progress".to_string(),
        session_id: "sess".to_string(),
        sequence: 1,
        payload: TalosEventPayload::MessageToolCallStarted(
            talos_client::TalosMessageToolCallStartedEvent {
                request_id: "req1".to_string(),
                call_id: "call-progress".to_string(),
                name: "bash".to_string(),
                arguments: serde_json::json!({"cmd": "pwd"}).into(),
                item_index: 0,
                iteration: 1,
                step_id: "step-1".to_string(),
                timing_ms: None,
            },
        ),
    });
    app.handle_stream_event(TalosEvent {
        event_type: TalosEventType::MessageToolCallProgressDelta,
        message_id: "msg-progress".to_string(),
        session_id: "sess".to_string(),
        sequence: 2,
        payload: TalosEventPayload::MessageToolCallProgressDelta(
            talos_types::talos::TalosMessageToolCallProgressDeltaEvent {
                request_id: "req1".to_string(),
                stream_key: talos_types::talos::TalosToolStreamKey { round: 0, index: 0 },
                call_id: "call-progress".to_string(),
                tool_name: "bash".to_string(),
                message: "running pwd".to_string(),
                progress_kind: talos_types::talos::TalosToolProgressKind::Status,
                item_index: 0,
                iteration: 1,
                step_id: "step-1".to_string(),
            },
        ),
    });

    let tool_block = app
        .chat_blocks
        .iter()
        .find(|block| block.tool_call_id.as_deref() == Some("call-progress"))
        .expect("tool block should exist");
    assert_eq!(tool_block.tool_name.as_deref(), Some("bash"));
    assert!(tool_block
        .tool_progress
        .iter()
        .any(|line| line == "status: running pwd"));
    assert_eq!(
        app.chat_blocks
            .iter()
            .filter(|block| block.tool_call_id.as_deref() == Some("call-progress"))
            .count(),
        1
    );
}

#[test]
fn test_tool_progress_events_append_to_same_tool_block() {
    let mut app = create_remote_app();
    app.session_id = Some("sess".to_string());

    app.handle_stream_event(TalosEvent {
        event_type: TalosEventType::MessageToolCallStarted,
        message_id: "msg-progress-append".to_string(),
        session_id: "sess".to_string(),
        sequence: 0,
        payload: TalosEventPayload::MessageToolCallStarted(
            talos_client::TalosMessageToolCallStartedEvent {
                request_id: "req1".to_string(),
                call_id: "call-progress-append".to_string(),
                name: "bash".to_string(),
                arguments: serde_json::json!({"cmd": "pwd"}).into(),
                item_index: 0,
                iteration: 1,
                step_id: "step-1".to_string(),
                timing_ms: None,
            },
        ),
    });

    for (sequence, message) in [(1, "line one"), (2, "line two")] {
        app.handle_stream_event(TalosEvent {
            event_type: TalosEventType::MessageToolCallProgressDelta,
            message_id: "msg-progress-append".to_string(),
            session_id: "sess".to_string(),
            sequence,
            payload: TalosEventPayload::MessageToolCallProgressDelta(
                talos_types::talos::TalosMessageToolCallProgressDeltaEvent {
                    request_id: "req1".to_string(),
                    stream_key: talos_types::talos::TalosToolStreamKey { round: 0, index: 0 },
                    call_id: "call-progress-append".to_string(),
                    tool_name: "bash".to_string(),
                    message: message.to_string(),
                    progress_kind: talos_types::talos::TalosToolProgressKind::Stdout,
                    item_index: 0,
                    iteration: 1,
                    step_id: "step-1".to_string(),
                },
            ),
        });
    }

    let tool_block = app
        .chat_blocks
        .iter()
        .find(|block| block.tool_call_id.as_deref() == Some("call-progress-append"))
        .expect("tool block should exist");

    assert!(tool_block
        .tool_progress
        .iter()
        .any(|line| line == "stdout: line one"));
    assert!(tool_block
        .tool_progress
        .iter()
        .any(|line| line == "stdout: line two"));
    assert_eq!(
        app.chat_blocks
            .iter()
            .filter(|block| block.tool_call_id.as_deref() == Some("call-progress-append"))
            .count(),
        1
    );
}

#[test]
fn test_pending_approval_updates_existing_tool_block() {
    let mut app = create_remote_app();
    app.session_id = Some("sess".to_string());

    app.handle_stream_event(TalosEvent {
        event_type: TalosEventType::MessageToolCallStarted,
        message_id: "msg-approval".to_string(),
        session_id: "sess".to_string(),
        sequence: 1,
        payload: TalosEventPayload::MessageToolCallStarted(
            talos_client::TalosMessageToolCallStartedEvent {
                request_id: "req1".to_string(),
                call_id: "call-approval".to_string(),
                name: "bash".to_string(),
                arguments: serde_json::json!({"cmd": "git push"}).into(),
                item_index: 0,
                iteration: 1,
                step_id: "step-1".to_string(),
                timing_ms: None,
            },
        ),
    });

    app.handle_stream_event(TalosEvent {
        event_type: TalosEventType::MessageToolCallPendingApproval,
        message_id: "msg-approval".to_string(),
        session_id: "sess".to_string(),
        sequence: 2,
        payload: TalosEventPayload::MessageToolCallPendingApproval(
            talos_types::talos::TalosMessageToolCallPendingApprovalEvent {
                request_id: "req1".to_string(),
                call_id: "call-approval".to_string(),
                tool_name: "bash".to_string(),
                prompt: "Need approval".to_string(),
                arguments: serde_json::json!({"cmd": "git push"}).into(),
                step_id: "step-1".to_string(),
            },
        ),
    });

    let tool_block = app
        .chat_blocks
        .iter()
        .find(|block| block.tool_call_id.as_deref() == Some("call-approval"))
        .expect("tool block should exist");
    assert_eq!(
        tool_block.tool_status,
        Some(ToolCallStatus::PendingApproval)
    );
    assert!(tool_block
        .tool_progress
        .iter()
        .any(|line| line == "status: waiting for approval"));
}

#[test]
fn test_assistant_output_remains_interleaved_with_tool_blocks() {
    let mut app = create_remote_app();
    app.session_id = Some("sess".to_string());

    app.handle_stream_event(TalosEvent {
        event_type: TalosEventType::MessageOutputDelta,
        message_id: "msg-interleave".to_string(),
        session_id: "sess".to_string(),
        sequence: 1,
        payload: TalosEventPayload::MessageOutputDelta(
            talos_client::TalosMessageOutputDeltaEvent {
                request_id: "req1".to_string(),
                delta: "I will inspect the repo first.".to_string(),
                item_index: 0,
                iteration: 1,
                step_id: "step-1".to_string(),
            },
        ),
    });
    app.handle_stream_event(TalosEvent {
        event_type: TalosEventType::MessageToolCallStarted,
        message_id: "msg-interleave".to_string(),
        session_id: "sess".to_string(),
        sequence: 2,
        payload: TalosEventPayload::MessageToolCallStarted(
            talos_client::TalosMessageToolCallStartedEvent {
                request_id: "req1".to_string(),
                call_id: "call-interleave".to_string(),
                name: "Glob".to_string(),
                arguments: serde_json::json!({"path": "/tmp/demo", "pattern": "**/*"}).into(),
                item_index: 0,
                iteration: 1,
                step_id: "step-1".to_string(),
                timing_ms: None,
            },
        ),
    });
    app.handle_stream_event(TalosEvent {
        event_type: TalosEventType::MessageOutputDelta,
        message_id: "msg-interleave".to_string(),
        session_id: "sess".to_string(),
        sequence: 3,
        payload: TalosEventPayload::MessageOutputDelta(
            talos_client::TalosMessageOutputDeltaEvent {
                request_id: "req1".to_string(),
                delta: "Next I will summarize what I found.".to_string(),
                item_index: 1,
                iteration: 1,
                step_id: "step-1".to_string(),
            },
        ),
    });

    let visible_blocks = app
        .chat_blocks
        .iter()
        .filter(|block| matches!(block.role, ChatRole::Assistant | ChatRole::Tool))
        .map(|block| {
            (
                block.role.clone(),
                block.content.clone(),
                block.tool_name.clone(),
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(visible_blocks.len(), 3);
    assert!(matches!(visible_blocks[0].0, ChatRole::Assistant));
    assert_eq!(visible_blocks[0].1, "I will inspect the repo first.");
    assert!(matches!(visible_blocks[1].0, ChatRole::Tool));
    assert_eq!(visible_blocks[1].2.as_deref(), Some("Glob"));
    assert!(matches!(visible_blocks[2].0, ChatRole::Assistant));
    assert_eq!(visible_blocks[2].1, "Next I will summarize what I found.");
}

#[test]
fn test_tool_block_tab_toggle_expand() {
    let mut app = create_remote_app();
    app.session_id = Some("sess".to_string());
    app.active_stream_message_id = Some("msg1".to_string());

    app.push_tool_call_block(
        "call-3".to_string(),
        "grep".to_string(),
        serde_json::json!({"pattern": "foo"}),
        1,
        0,
        None,
    );

    let idx = *app.transcript.active_tool_calls.get("call-3").unwrap();
    assert!(!app.chat_blocks[idx].expanded);

    app.toggle_last_tool_block();
    assert!(app.chat_blocks[idx].expanded);

    app.toggle_last_tool_block();
    assert!(!app.chat_blocks[idx].expanded);
}

#[tokio::test]
async fn test_ctrl_o_toggles_last_tool_block() {
    let mut app = create_remote_app();
    app.push_tool_call_block(
        "call-ctrl-o".to_string(),
        "grep".to_string(),
        serde_json::json!({"pattern": "foo"}),
        1,
        0,
        None,
    );
    let idx = *app.transcript.active_tool_calls.get("call-ctrl-o").unwrap();
    assert!(!app.chat_blocks[idx].expanded);

    let _ = app
        .handle_input(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL))
        .await
        .unwrap();
    assert!(app.chat_blocks[idx].expanded);
}

#[test]
fn test_tool_blocks_are_ordered_by_iteration_and_item_index() {
    let mut app = create_remote_app();
    app.session_id = Some("sess-test".to_string());

    app.handle_stream_event(TalosEvent {
        event_type: TalosEventType::MessageToolCallStarted,
        message_id: "msg-order".to_string(),
        session_id: "sess-test".to_string(),
        sequence: 2,
        payload: TalosEventPayload::MessageToolCallStarted(
            talos_client::TalosMessageToolCallStartedEvent {
                request_id: "req-test".to_string(),
                call_id: "call-second".to_string(),
                name: "read".to_string(),
                arguments: serde_json::json!({"file_path":"/tmp/b.txt"}).into(),
                item_index: 1,
                iteration: 1,
                step_id: "step-1".to_string(),
                timing_ms: None,
            },
        ),
    });
    app.handle_stream_event(TalosEvent {
        event_type: TalosEventType::MessageToolCallStarted,
        message_id: "msg-order".to_string(),
        session_id: "sess-test".to_string(),
        sequence: 3,
        payload: TalosEventPayload::MessageToolCallStarted(
            talos_client::TalosMessageToolCallStartedEvent {
                request_id: "req-test".to_string(),
                call_id: "call-first".to_string(),
                name: "grep".to_string(),
                arguments: serde_json::json!({"pattern":"main","path":"/tmp"}).into(),
                item_index: 0,
                iteration: 1,
                step_id: "step-1".to_string(),
                timing_ms: None,
            },
        ),
    });

    let tool_call_ids = app
        .chat_blocks
        .iter()
        .filter(|block| block.role == ChatRole::Tool)
        .filter_map(|block| block.tool_call_id.clone())
        .collect::<Vec<_>>();

    assert_eq!(tool_call_ids, vec!["call-first", "call-second"]);
}
