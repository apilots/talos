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
                arguments: serde_json::json!({"command": "ls"}),
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
    assert_eq!(block.tool_status, Some(ToolCallStatus::Running));
    assert!(app.active_tool_calls.contains_key("call-1"));
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
                name: "read_file".to_string(),
                arguments: serde_json::json!({"path": "/tmp/x"}),
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
                tool: "read_file".to_string(),
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
    assert!(!app.active_tool_calls.contains_key("call-2"));
}

#[test]
fn test_tool_completed_event_updates_block_even_with_different_message_id() {
    let mut app = create_remote_app();
    app.session_id = Some("sess".to_string());
    app.active_stream_message_id = Some("msg-active".to_string());
    app.last_stream_sequence = Some(10);
    app.push_tool_call_block(
        "call-stale".to_string(),
        "memory_search".to_string(),
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
                tool: "memory_search".to_string(),
                content: vec![talos_client::TalosContentItem::Json {
                    value: serde_json::json!({"results": []}),
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
        Some(ToolCallStatus::Completed { exit_ok: true })
    );
    assert_eq!(app.chat_blocks[idx].timing_ms, Some(120));
    assert!(!app.active_tool_calls.contains_key("call-stale"));
}

#[test]
fn test_tool_completed_event_updates_block_even_when_sequence_is_older() {
    let mut app = create_remote_app();
    app.session_id = Some("sess".to_string());
    app.active_stream_message_id = Some("msg-active".to_string());
    app.last_stream_sequence = Some(20);
    app.push_tool_call_block(
        "call-older".to_string(),
        "exec_command".to_string(),
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
                tool: "exec_command".to_string(),
                content: vec![talos_client::TalosContentItem::Json {
                    value: serde_json::json!({
                        "session_id": null,
                        "status": "exited",
                        "output": "/tmp\n",
                        "exit_code": 0,
                        "truncated": false
                    }),
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
        Some(ToolCallStatus::Completed { exit_ok: true })
    );
    assert!(!app.active_tool_calls.contains_key("call-older"));
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

    let idx = *app.active_tool_calls.get("call-3").unwrap();
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
    let idx = *app.active_tool_calls.get("call-ctrl-o").unwrap();
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
                name: "read_file".to_string(),
                arguments: serde_json::json!({"file_path":"/tmp/b.txt"}),
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
                name: "grep_files".to_string(),
                arguments: serde_json::json!({"pattern":"main","path":"/tmp"}),
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

#[test]
fn test_tool_block_folded_shows_details_and_output_preview() {
    let app = create_embedded_app();
    let mut block = ChatBlock::new(
            ChatRole::Tool,
            r#"{"session_id":null,"status":"exited","output":"line1\nline2\nline3\nline4","exit_code":0,"truncated":false}"#
                .to_string(),
        );
    block.tool_name = Some("exec_command".to_string());
    block.tool_input = Some(serde_json::json!({
        "cmd": "git diff HEAD~1",
        "workdir": "/tmp/work"
    }));
    block.tool_status = Some(ToolCallStatus::Completed { exit_ok: true });
    block.timing_ms = Some(1200);
    block.expanded = false;

    let lines = app.tool_block_lines(&block, 80);
    let texts = lines
        .iter()
        .map(|line| line_plain_text(&line.display))
        .collect::<Vec<_>>();

    assert!(texts
        .iter()
        .any(|line| line == "● exec_command(git diff HEAD~1)"));
    assert_eq!(texts.get(1).map(String::as_str), Some(""));
    assert!(texts.iter().any(|line| line == "└ Done (exit:0 · 1.2s)"));
    assert!(texts.iter().any(|line| line == "  cmd:  git diff HEAD~1"));
    assert!(texts.iter().any(|line| line == "  dir:  /tmp/work"));
    assert!(texts.iter().any(|line| line == "  status: success"));
    assert!(texts.iter().any(|line| line == "  stdout: line1"));
    assert!(texts.iter().any(|line| line == "  (ctrl+o to expand)"));
}

#[test]
fn test_tool_block_expanded_shows_details_without_output() {
    let app = create_embedded_app();
    let mut block = ChatBlock::new(ChatRole::Tool, String::new());
    block.tool_name = Some("read_file".to_string());
    block.tool_input = Some(serde_json::json!({
        "path": "/tmp/example.txt"
    }));
    block.tool_status = Some(ToolCallStatus::Completed { exit_ok: true });
    block.expanded = true;

    let lines = app.tool_block_lines(&block, 80);
    let texts = lines
        .iter()
        .map(|line| line_plain_text(&line.display))
        .collect::<Vec<_>>();

    assert!(texts.iter().any(|line| line == "  path:  /tmp/example.txt"));
    assert!(!texts.iter().any(|line| line == "  ─────────────────"));
    assert!(texts.iter().any(|line| line == "  (ctrl+o to collapse)"));
}

#[test]
fn test_read_file_folded_truncates_json_preview() {
    let app = create_embedded_app();
    let mut block = ChatBlock::new(
            ChatRole::Tool,
            serde_json::json!({
                "ok": true,
                "tool": "read_file",
                "result": "L1: This is a very long preview line that should be truncated in folded mode because it exceeds the preview limit by a wide margin and keeps going.",
                "truncated": false
            })
            .to_string(),
        );
    block.tool_name = Some("read_file".to_string());
    block.tool_input = Some(serde_json::json!({
        "path": "/tmp/example.txt"
    }));
    block.tool_status = Some(ToolCallStatus::Completed { exit_ok: true });

    let texts = app
        .tool_block_lines(&block, 200)
        .into_iter()
        .map(|line| line_plain_text(&line.display))
        .collect::<Vec<_>>();

    assert!(texts.iter().any(|line| line == "  path:  /tmp/example.txt"));
    assert!(texts.iter().any(|line| line == "  truncated: false"));
    let preview = texts
        .iter()
        .find(|line| line.starts_with("  preview: "))
        .expect("folded tool block should include preview");
    assert!(preview.ends_with("..."));
    assert!(
        preview.len() <= 115,
        "preview should stay compact: {}",
        preview.len()
    );
}

#[test]
fn test_read_file_expanded_pretty_prints_json_output() {
    let app = create_embedded_app();
    let mut block = ChatBlock::new(
        ChatRole::Tool,
        serde_json::json!({
            "ok": true,
            "tool": "read_file",
            "result": "L1: alpha\nL2: beta",
            "truncated": false
        })
        .to_string(),
    );
    block.tool_name = Some("read_file".to_string());
    block.tool_input = Some(serde_json::json!({
        "path": "/tmp/example.txt"
    }));
    block.tool_status = Some(ToolCallStatus::Completed { exit_ok: true });
    block.expanded = true;

    let texts = app
        .tool_block_lines(&block, 120)
        .into_iter()
        .map(|line| line_plain_text(&line.display))
        .collect::<Vec<_>>();

    assert!(texts.iter().any(|line| line == "  ─────────────────"));
    assert!(texts
        .iter()
        .any(|line| line.trim() == "\"tool\": \"read_file\","));
    assert!(texts
        .iter()
        .any(|line| line.trim() == "\"truncated\": false"));
}

#[test]
fn test_memory_search_folded_shows_query_and_hits() {
    let app = create_embedded_app();
    let mut block = ChatBlock::new(
            ChatRole::Tool,
            r#"{"results":[{"path":"spec/tui-improvement.md","snippet":"TUI layout summary","citation":"spec/tui-improvement.md#L1","score":0.91,"start_line":1,"end_line":1},{"path":"spec/tui-claude-code-style.md","snippet":"Claude style summary","citation":"spec/tui-claude-code-style.md#L1","score":0.87,"start_line":1,"end_line":1}]}"#
                .to_string(),
        );
    block.tool_name = Some("memory_search".to_string());
    block.tool_input = Some(serde_json::json!({
        "query": "talos 项目结构"
    }));
    block.tool_status = Some(ToolCallStatus::Completed { exit_ok: true });
    block.timing_ms = Some(800);

    let texts = app
        .tool_block_lines(&block, 100)
        .into_iter()
        .map(|line| line_plain_text(&line.display))
        .collect::<Vec<_>>();

    assert!(texts
        .iter()
        .any(|line| line == "● memory_search(talos 项目结构)"));
    assert!(texts.iter().any(|line| line == "└ Done (2 hits · 0.8s)"));
    assert!(texts.iter().any(|line| line == "  query: talos 项目结构"));
    assert!(texts
        .iter()
        .any(|line| line == "  hit: spec/tui-improvement.md"));
    assert!(texts
        .iter()
        .any(|line| line == "  hit: spec/tui-claude-code-style.md"));
}

#[test]
fn test_complete_tool_call_recovers_after_block_index_shift() {
    let mut app = create_embedded_app();
    let user_idx = app.push_block(ChatRole::User, "hello".to_string());
    let thinking_idx = app.push_block(ChatRole::Thinking, String::new());
    let assistant_idx = app.push_block(ChatRole::Assistant, String::new());
    let tool_idx = app.push_tool_call_block(
        "call-shift".to_string(),
        "exec_command".to_string(),
        serde_json::json!({"command": "pwd"}),
        1,
        0,
        None,
    );

    app.active_user_idx = Some(user_idx);
    app.active_thinking_idx = Some(thinking_idx);
    app.active_assistant_idx = Some(assistant_idx);
    app.discard_active_pending_blocks();

    let shifted_idx = *app
        .active_tool_calls
        .get("call-shift")
        .expect("tool block index should be remapped");
    assert!(shifted_idx < tool_idx);
    assert_eq!(
        app.chat_blocks[shifted_idx].tool_call_id.as_deref(),
        Some("call-shift")
    );

    app.complete_tool_call_block("call-shift", "ok".to_string(), true, Some(15));

    assert_eq!(app.chat_blocks[shifted_idx].content, "ok");
    assert_eq!(
        app.chat_blocks[shifted_idx].tool_status,
        Some(ToolCallStatus::Completed { exit_ok: true })
    );
    assert_eq!(app.chat_blocks[shifted_idx].timing_ms, Some(15));
    assert!(!app.active_tool_calls.contains_key("call-shift"));
}

#[test]
fn test_replay_full_mixed_turn_matches_streaming_order() {
    let mut history_app = create_embedded_app();
    history_app.chat_blocks.clear();
    let items = vec![
        TalosOutputItem::UserInput {
            text: "open the file".to_string(),
        },
        TalosOutputItem::Reasoning {
            text: "need to inspect the workspace".to_string(),
        },
        TalosOutputItem::ToolCall {
            call_id: "call-history".to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path":"/tmp/demo.txt"}),
        },
        TalosOutputItem::ToolResult {
            call_id: "call-history".to_string(),
            tool_name: "read_file".to_string(),
            content: vec![TalosContentItem::Text {
                text: "demo contents".to_string(),
            }],
            is_error: false,
        },
        TalosOutputItem::OutputText {
            text: "done".to_string(),
        },
    ];

    for (index, item) in items.iter().enumerate() {
        history_app.replay_history_item(item, 0, index as u64);
    }

    let mut stream_app = create_embedded_app();
    stream_app.chat_blocks.clear();
    stream_app.push_block(ChatRole::User, "open the file".to_string());
    stream_app.add_message_to_history("open the file".to_string());
    stream_app.push_block(
        ChatRole::Thinking,
        "need to inspect the workspace".to_string(),
    );
    stream_app.push_tool_call_block(
        "call-history".to_string(),
        "read_file".to_string(),
        serde_json::json!({"path":"/tmp/demo.txt"}),
        0,
        2,
        None,
    );
    stream_app.complete_tool_call_block("call-history", "demo contents".to_string(), true, None);
    stream_app.push_block(ChatRole::Assistant, "done".to_string());

    assert_eq!(history_app.chat_blocks.len(), stream_app.chat_blocks.len());

    let history_roles = history_app
        .chat_blocks
        .iter()
        .map(|block| block.role.clone())
        .collect::<Vec<_>>();
    let stream_roles = stream_app
        .chat_blocks
        .iter()
        .map(|block| block.role.clone())
        .collect::<Vec<_>>();
    assert_eq!(history_roles, stream_roles);

    let history_tool = history_app
        .chat_blocks
        .iter()
        .find(|block| block.role == ChatRole::Tool)
        .expect("history replay should keep a tool block");
    let stream_tool = stream_app
        .chat_blocks
        .iter()
        .find(|block| block.role == ChatRole::Tool)
        .expect("stream replay should keep a tool block");

    assert_eq!(history_tool.tool_name, stream_tool.tool_name);
    assert_eq!(history_tool.tool_input, stream_tool.tool_input);
    assert_eq!(history_tool.tool_status, stream_tool.tool_status);
    assert_eq!(history_tool.content, stream_tool.content);
    assert_eq!(history_app.chat_blocks[0].content, "open the file");
    assert_eq!(
        history_app.chat_blocks[1].content,
        "need to inspect the workspace"
    );
    assert_eq!(history_app.chat_blocks[3].content, "done");
}

#[test]
fn test_system_block_lines_keep_multiline_structure() {
    let app = create_embedded_app();
    let block = ChatBlock::new(ChatRole::System, "System ready\nLoading tools".to_string());

    let lines = app.system_block_lines(&block, 80);
    let texts = lines
        .iter()
        .map(|line| line_plain_text(&line.display))
        .collect::<Vec<_>>();

    assert_eq!(
        texts,
        vec!["⚙  System", "", "System ready", "Loading tools"]
    );
}

#[test]
fn test_system_block_lines_render_welcome_card_without_prefix() {
    let app = create_embedded_app();
    let block = ChatBlock::new(ChatRole::System, "╭────╮\n│ hi │\n╰────╯".to_string());

    let lines = app.system_block_lines(&block, 80);
    let texts = lines
        .iter()
        .map(|line| line_plain_text(&line.display))
        .collect::<Vec<_>>();

    assert_eq!(texts, vec!["╭────╮", "│ hi │", "╰────╯"]);
}

// ── Phase 4: markdown rendering ──────────────────────────────────────

#[test]
fn test_markdown_bold_renders_bold_span() {
    let lines = render_markdown_lines("Hello **world** text");
    assert_eq!(lines.len(), 1);
    let has_bold = lines[0]
        .spans
        .iter()
        .any(|s| s.content.contains("world") && s.style.add_modifier.contains(Modifier::BOLD));
    assert!(has_bold, "expected 'world' to be bold");
}

#[test]
fn test_markdown_italic_renders_italic_span() {
    let lines = render_markdown_lines("Hello *italic* text");
    assert_eq!(lines.len(), 1);
    let has_italic = lines[0]
        .spans
        .iter()
        .any(|s| s.content.contains("italic") && s.style.add_modifier.contains(Modifier::ITALIC));
    assert!(has_italic, "expected 'italic' to be italic");
}

#[test]
fn test_markdown_inline_code_uses_green_fg() {
    let lines = render_markdown_lines("Run `cargo build` now");
    assert_eq!(lines.len(), 1);
    let has_code = lines[0]
        .spans
        .iter()
        .any(|s| s.content.contains("cargo build") && s.style.fg == Some(Color::LightGreen));
    assert!(has_code, "expected inline code to have LightGreen fg");
}

#[test]
fn test_markdown_header_h1_has_prefix() {
    let lines = render_markdown_lines("# Title");
    assert_eq!(lines.len(), 1);
    let has_prefix = lines[0]
        .spans
        .first()
        .map(|s| s.content.contains('▌'))
        .unwrap_or(false);
    assert!(has_prefix, "H1 should have ▌ prefix span");
}

#[test]
fn test_markdown_list_item_has_bullet() {
    let lines = render_markdown_lines("- item one");
    assert_eq!(lines.len(), 1);
    let has_bullet = lines[0].spans.iter().any(|s| s.content.contains('•'));
    assert!(has_bullet, "list item should have • bullet");
}
