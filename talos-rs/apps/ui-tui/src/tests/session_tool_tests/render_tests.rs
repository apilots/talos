use super::*;

#[test]
fn test_tool_block_folded_shows_details_and_output_preview() {
    let app = create_embedded_app();
    let mut block = ChatBlock::new(
            ChatRole::Tool,
            r#"{"session_id":null,"status":"exited","output":"line1\nline2\nline3\nline4","exit_code":0,"truncated":false}"#
                .to_string(),
        );
    block.tool_name = Some("bash".to_string());
    block.tool_input = Some(
        serde_json::json!({
            "cmd": "git diff HEAD~1",
            "workdir": "/tmp/work"
        })
        .into(),
    );
    block.tool_status = Some(ToolCallStatus::Completed { exit_ok: true });
    block.timing_ms = Some(1200);
    block.expanded = false;

    let lines = app.tool_block_lines(&block, 80);
    let texts = lines
        .iter()
        .map(|line| line_plain_text(&line.display))
        .collect::<Vec<_>>();

    assert!(texts.iter().any(|line| line == "● Bash(git diff HEAD~1)"));
    assert!(texts.iter().any(|line| line == "  Done (exit:0 · 1.2s)"));
    assert!(texts.iter().any(|line| line == "  dir:  /tmp/work"));
    assert!(texts.iter().any(|line| line == "  stdout: line1"));
    assert!(texts.iter().any(|line| line == "  (ctrl+o to expand)"));
}

#[test]
fn test_tool_block_expanded_shows_details_without_output() {
    let app = create_embedded_app();
    let mut block = ChatBlock::new(ChatRole::Tool, String::new());
    block.tool_name = Some("read".to_string());
    block.tool_input = Some(
        serde_json::json!({
            "path": "/tmp/example.txt"
        })
        .into(),
    );
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
fn test_glob_tool_header_uses_path_summary() {
    let app = create_embedded_app();
    let mut block = ChatBlock::new(ChatRole::Tool, String::new());
    block.tool_name = Some("Glob".to_string());
    block.tool_input = Some(
        serde_json::json!({
            "path": "/tmp/project",
            "pattern": "**/*"
        })
        .into(),
    );
    block.tool_status = Some(ToolCallStatus::Queued);

    let texts = app
        .tool_block_lines(&block, 80)
        .into_iter()
        .map(|line| line_plain_text(&line.display))
        .collect::<Vec<_>>();

    assert!(texts
        .iter()
        .any(|line| line == "◌ Glob(\"**/*\", path=/tmp/project)"));
}

#[test]
fn test_grep_tool_header_uses_pattern_and_path_summary() {
    let app = create_embedded_app();
    let mut block = ChatBlock::new(ChatRole::Tool, String::new());
    block.tool_name = Some("grep".to_string());
    block.tool_input = Some(
        serde_json::json!({
            "pattern": "AgentKernel",
            "path": "crates/orchestration",
            "include": "*.rs"
        })
        .into(),
    );
    block.tool_status = Some(ToolCallStatus::Running);

    let texts = app
        .tool_block_lines(&block, 100)
        .into_iter()
        .map(|line| line_plain_text(&line.display))
        .collect::<Vec<_>>();

    assert!(texts.iter().any(|line| {
        line == "⏳ Grep(\"AgentKernel\", path=crates/orchestration, include=*.rs)"
    }));
}

#[test]
fn test_read_folded_truncates_json_preview() {
    let app = create_embedded_app();
    let mut block = ChatBlock::new(
            ChatRole::Tool,
            serde_json::json!({
                "ok": true,
                "tool": "read",
                "result": "L1: This is a very long preview line that should be truncated in folded mode because it exceeds the preview limit by a wide margin and keeps going.",
                "truncated": false
            })
            .to_string(),
        );
    block.tool_name = Some("read".to_string());
    block.tool_input = Some(
        serde_json::json!({
            "path": "/tmp/example.txt"
        })
        .into(),
    );
    block.tool_status = Some(ToolCallStatus::Completed { exit_ok: true });

    let texts = app
        .tool_block_lines(&block, 200)
        .into_iter()
        .map(|line| line_plain_text(&line.display))
        .collect::<Vec<_>>();

    assert!(texts.iter().any(|line| line == "  Done (1 lines)"));
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
fn test_read_expanded_pretty_prints_json_output() {
    let app = create_embedded_app();
    let mut block = ChatBlock::new(
        ChatRole::Tool,
        serde_json::json!({
            "ok": true,
            "tool": "read",
            "result": "L1: alpha\nL2: beta",
            "truncated": false
        })
        .to_string(),
    );
    block.tool_name = Some("read".to_string());
    block.tool_input = Some(
        serde_json::json!({
            "path": "/tmp/example.txt"
        })
        .into(),
    );
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
        .any(|line| line.trim() == "\"tool\": \"read\","));
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
    block.tool_name = Some("MemorySearch".to_string());
    block.tool_input = Some(
        serde_json::json!({
            "query": "talos 项目结构"
        })
        .into(),
    );
    block.tool_status = Some(ToolCallStatus::Completed { exit_ok: true });
    block.timing_ms = Some(800);

    let texts = app
        .tool_block_lines(&block, 100)
        .into_iter()
        .map(|line| line_plain_text(&line.display))
        .collect::<Vec<_>>();

    assert!(texts
        .iter()
        .any(|line| line == "● MemorySearch(talos 项目结构)"));
    assert!(texts.iter().any(|line| line == "  Done (2 hits · 0.8s)"));
    assert!(texts
        .iter()
        .any(|line| line == "  hit: spec/tui-improvement.md"));
    assert!(!texts
        .iter()
        .any(|line| line == "  hit: spec/tui-claude-code-style.md"));
}

#[test]
fn test_running_tool_folded_shows_latest_progress_only() {
    let app = create_embedded_app();
    let mut block = ChatBlock::new(ChatRole::Tool, String::new());
    block.tool_name = Some("bash".to_string());
    block.tool_input = Some(
        serde_json::json!({
            "cmd": "cargo test -p talos-ui-tui --lib",
            "workdir": "/tmp/work"
        })
        .into(),
    );
    block.tool_status = Some(ToolCallStatus::Running);
    block.tool_progress = vec![
        "status: spawned".to_string(),
        "stdout: running session_tool_tests".to_string(),
    ];

    let texts = app
        .tool_block_lines(&block, 120)
        .into_iter()
        .map(|line| line_plain_text(&line.display))
        .collect::<Vec<_>>();

    assert!(texts
        .iter()
        .any(|line| line == "⏳ Bash(cargo test -p talos-ui-tui --lib)"));
    assert!(texts.iter().any(|line| line == "  Running…"));
    assert!(texts.iter().any(|line| line == "  dir:  /tmp/work"));
    assert!(texts
        .iter()
        .any(|line| line == "  stdout: running session_tool_tests"));
    assert!(!texts.iter().any(|line| line == "  status: spawned"));
}

#[test]
fn test_glob_folded_shows_path_preview() {
    let app = create_embedded_app();
    let mut block = ChatBlock::new(
        ChatRole::Tool,
        serde_json::json!(["src/lib.rs", "Cargo.toml", "README.md"]).to_string(),
    );
    block.tool_name = Some("Glob".to_string());
    block.tool_input = Some(
        serde_json::json!({
            "path": "/tmp/project",
            "pattern": "**/*"
        })
        .into(),
    );
    block.tool_status = Some(ToolCallStatus::Completed { exit_ok: true });

    let texts = app
        .tool_block_lines(&block, 100)
        .into_iter()
        .map(|line| line_plain_text(&line.display))
        .collect::<Vec<_>>();

    assert!(texts
        .iter()
        .any(|line| line == "● Glob(\"**/*\", path=/tmp/project)"));
    assert!(texts.iter().any(|line| line == "  Done (3 paths)"));
    assert!(texts.iter().any(|line| line == "  path: src/lib.rs"));
}

#[test]
fn test_grep_folded_shows_match_preview() {
    let app = create_embedded_app();
    let mut block = ChatBlock::new(
        ChatRole::Tool,
        serde_json::json!({
            "content": "crates/orchestration/agent-kernel/src/planner.rs\ncrates/orchestration/agent/src/agent.rs",
            "matches_found": 2
        })
        .to_string(),
    );
    block.tool_name = Some("grep".to_string());
    block.tool_input = Some(
        serde_json::json!({
            "pattern": "AgentKernel",
            "path": "crates/orchestration",
            "include": "*.rs"
        })
        .into(),
    );
    block.tool_status = Some(ToolCallStatus::Completed { exit_ok: true });

    let texts = app
        .tool_block_lines(&block, 120)
        .into_iter()
        .map(|line| line_plain_text(&line.display))
        .collect::<Vec<_>>();

    assert!(texts
        .iter()
        .any(|line| line == "● Grep(\"AgentKernel\", path=crates/orchestration, include=*.rs)"));
    assert!(texts.iter().any(|line| line == "  Done (2 matches)"));
    assert!(texts
        .iter()
        .any(|line| line == "  match: crates/orchestration/agent-kernel/src/planner.rs"));
}

#[test]
fn test_complete_tool_call_recovers_after_block_index_shift() {
    let mut app = create_embedded_app();
    let user_idx = app.push_block(ChatRole::User, "hello".to_string());
    let thinking_idx = app.push_block(ChatRole::Thinking, String::new());
    let assistant_idx = app.push_block(ChatRole::Assistant, String::new());
    let tool_idx = app.push_tool_call_block(
        "call-shift".to_string(),
        "bash".to_string(),
        serde_json::json!({"command": "pwd"}),
        1,
        0,
        None,
    );

    app.active_user_idx = Some(user_idx);
    app.transcript.live_thinking_idx = Some(thinking_idx);
    app.transcript.live_assistant_idx = Some(assistant_idx);
    app.discard_active_pending_blocks();

    let shifted_idx = *app
        .transcript
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
    assert!(!app.transcript.active_tool_calls.contains_key("call-shift"));
}
