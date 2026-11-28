use super::*;

#[tokio::test]
async fn history_query_enforces_utf8_bytes_before_returning_message_bodies() {
    let directory = tempfile::tempdir().unwrap();
    let store = WorkerExecutionStore::open(&directory.path().join("history.sqlite"))
        .await
        .unwrap();
    admit(&store).await;
    let mut message = match command() {
        AgentExecutionCommand::Input { params } => params.input,
        _ => unreachable!(),
    };
    message.blocks = vec![talos_agent_api::AgentMessageBlock {
        block_id: "block".into(),
        status: talos_agent_api::AgentMessageBlockStatus::Completed,
        content: talos_agent_api::AgentMessageBlockContent::UserContent(
            talos_agent_api::AgentUserContent::Text {
                text: "\u{754c}".repeat(128),
            },
        ),
        started_at: 1,
        completed_at: Some(1),
    }];
    let mut saved = checkpoint(3);
    saved.messages = vec![message.clone(), message.clone()];
    store
        .append(
            &session(),
            vec![
                event(2, AgentEventPayload::MessageCompleted(message.clone())),
                event(3, AgentEventPayload::MessageCompleted(message.clone())),
            ],
            &saved,
            None,
        )
        .await
        .unwrap();
    let bytes = encode(&event(
        2,
        AgentEventPayload::MessageCompleted(message.clone()),
    ))
    .unwrap()
    .len();
    let page = store
        .history_messages(&AgentTurnRef::new("session", "turn"), Some(0), 2, bytes * 2)
        .await
        .unwrap();
    assert_eq!(
        (page.messages, page.has_more, page.bytes),
        (vec![message], true, bytes)
    );
    // A persisted malformed oversized record must be rejected before decoding.
    sqlx::query("UPDATE agent_events SET event_json=json_set(event_json,'$.payload.data.blocks',?) WHERE sequence=2")
        .bind("x".repeat(agent_protocol::MAX_EXECUTION_EVENT_BYTES)).execute(&store.pool).await.unwrap();
    let error = store
        .history_messages(&AgentTurnRef::new("session", "turn"), Some(0), 2, bytes * 4)
        .await
        .err()
        .unwrap();
    assert_eq!(error.error_code(), ErrorCode::ContextWindowExceeded);
}

#[tokio::test]
async fn history_keeps_canonical_creation_order_when_messages_complete_out_of_order() {
    let directory = tempfile::tempdir().unwrap();
    let store = WorkerExecutionStore::open(&directory.path().join("order.sqlite"))
        .await
        .unwrap();
    admit(&store).await;
    let mut first = match command() {
        AgentExecutionCommand::Input { params } => params.input,
        _ => unreachable!(),
    };
    first.message_id = "first".into();
    let mut second = first.clone();
    second.message_id = "second".into();
    let mut saved = checkpoint(5);
    saved.messages = vec![first.clone(), second.clone()];
    store
        .append(
            &session(),
            vec![
                event(
                    2,
                    AgentEventPayload::MessageStarted(talos_agent_api::AgentMessageStarted {
                        message: first.clone(),
                    }),
                ),
                event(
                    3,
                    AgentEventPayload::MessageStarted(talos_agent_api::AgentMessageStarted {
                        message: second.clone(),
                    }),
                ),
                event(4, AgentEventPayload::MessageCompleted(second.clone())),
                event(5, AgentEventPayload::MessageCompleted(first.clone())),
            ],
            &saved,
            None,
        )
        .await
        .unwrap();
    let first_page = store
        .history_messages(&AgentTurnRef::new("session", "turn"), Some(0), 1, 16 * 1024)
        .await
        .unwrap();
    assert_eq!(
        (first_page.messages, first_page.covered_until_event_seq),
        (vec![first.clone()], 2)
    );
    let next_page = store
        .history_messages(&AgentTurnRef::new("session", "turn"), Some(2), 1, 16 * 1024)
        .await
        .unwrap();
    assert_eq!(
        (next_page.messages, next_page.covered_until_event_seq),
        (vec![second.clone()], 3)
    );
    for after in [None, Some(0)] {
        let page = store
            .history_messages(&AgentTurnRef::new("session", "turn"), after, 2, 16 * 1024)
            .await
            .unwrap();
        assert_eq!(page.messages, vec![first.clone(), second.clone()]);
    }
}

#[tokio::test]
async fn history_result_window_retains_its_late_completed_invocation_without_claiming_it_again() {
    use talos_agent_api::{
        AgentMessageBlock, AgentMessageBlockContent, AgentMessageBlockStatus, AgentMessageStarted,
        AgentToolCall, AgentToolCallArguments, AgentToolResult,
    };
    let directory = tempfile::tempdir().unwrap();
    let store = WorkerExecutionStore::open(&directory.path().join("companions.sqlite"))
        .await
        .unwrap();
    admit(&store).await;
    let mut call = match command() {
        AgentExecutionCommand::Input { params } => params.input,
        _ => unreachable!(),
    };
    call.message_id = "invocation".into();
    call.role = AgentMessageRole::Agent;
    call.blocks = vec![AgentMessageBlock {
        block_id: "call".into(),
        status: AgentMessageBlockStatus::Completed,
        started_at: 1,
        completed_at: Some(1),
        content: AgentMessageBlockContent::ToolCall(AgentToolCall {
            call_id: "call".into(),
            name: "read_file".into(),
            arguments: AgentToolCallArguments::Complete {
                value: serde_json::json!({}).into(),
            },
            progress: vec![],
        }),
    }];
    let mut result = call.clone();
    result.message_id = "result".into();
    result.role = AgentMessageRole::Tool;
    result.blocks[0].content = AgentMessageBlockContent::ToolResult(AgentToolResult {
        call_id: "call".into(),
        tool_name: "read_file".into(),
        content: vec![],
        is_error: false,
        metrics: None,
    });
    let mut call_start = call.clone();
    call_start.status = AgentMessageStatus::InProgress;
    call_start.completed_at = None;
    call_start.blocks.clear();
    let mut result_start = call_start.clone();
    result_start.message_id = "result".into();
    result_start.role = AgentMessageRole::Tool;
    let mut saved = checkpoint(5);
    saved.messages = vec![call.clone(), result.clone()];
    store
        .append(
            &session(),
            vec![
                event(
                    2,
                    AgentEventPayload::MessageStarted(AgentMessageStarted {
                        message: call_start,
                    }),
                ),
                event(
                    3,
                    AgentEventPayload::MessageStarted(AgentMessageStarted {
                        message: result_start,
                    }),
                ),
                event(4, AgentEventPayload::MessageCompleted(result.clone())),
                event(5, AgentEventPayload::MessageCompleted(call.clone())),
            ],
            &saved,
            None,
        )
        .await
        .unwrap();
    let page = store
        .history_messages(&AgentTurnRef::new("session", "turn"), Some(2), 1, 16 * 1024)
        .await
        .unwrap();
    assert_eq!(
        (
            page.messages,
            page.message_event_sequences,
            page.covered_until_event_seq
        ),
        (vec![call, result], vec![("result".into(), 3)], 3)
    );
}
