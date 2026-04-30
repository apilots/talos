use super::*;

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
            name: "read".to_string(),
            arguments: serde_json::json!({"path":"/tmp/demo.txt"}).into(),
        },
        TalosOutputItem::ToolResult {
            call_id: "call-history".to_string(),
            tool_name: "read".to_string(),
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
        "read".to_string(),
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
