use super::*;
use talos_agent::SessionHistoryReadTurn;
use talos_agent_api::{
    AgentMessageBlock, AgentMessageBlockStatus, AgentMessageStatus, AgentRunMetrics,
    AgentRunOutput, AgentStopReason, AgentTurnCompleted, AgentTurnRef,
};

#[test]
fn oldest_projection_advances_only_the_retained_message_prefix() {
    let messages = (0..3)
        .map(|index| AgentMessage {
            message_id: format!("message-{index}"),
            turn_id: "turn".into(),
            role: AgentMessageRole::User,
            status: AgentMessageStatus::Completed,
            created_at: 1,
            completed_at: Some(1),
            blocks: vec![AgentMessageBlock {
                block_id: format!("block-{index}"),
                status: AgentMessageBlockStatus::Completed,
                content: AgentMessageBlockContent::UserContent(AgentUserContent::Text {
                    text: format!("fact-{index}"),
                }),
                started_at: 1,
                completed_at: Some(1),
            }],
        })
        .collect::<Vec<_>>();
    let window = SessionHistoryReadTurn {
        message_event_sequences: messages
            .iter()
            .enumerate()
            .map(|(index, message)| (message.message_id.clone(), index as i64 + 2))
            .collect(),
        read_after_event_seq: 0,
        covered_until_event_seq: 5,
        turn: SessionHistoryTurn {
            identity: AgentTurnRef::new("session", "turn"),
            automation_input: None,
            notices: vec![],
            completed_at: 1,
            completed_event_seq: 5,
            result: AgentRunResult::Completed(AgentRunOutput {
                messages,
                metrics: AgentRunMetrics::default(),
                completion: AgentTurnCompleted {
                    stop_reason: AgentStopReason::EndTurn,
                },
            }),
        },
    };
    let config = SessionPromptContextConfig {
        source_message_limit: None,
        compact_cursor: None,
        recent_window: SessionPromptRecentWindow::OldestUncovered,
        recent_messages_limit: 1,
        recent_tool_results_limit: 0,
        recent_tool_exchanges_limit: 0,
        tool_output_max_chars: 100,
        tool_exchange_max_chars: 100,
        include_summary: false,
        summary_recent_count: 0,
        summary_max_chars: 0,
    };
    let context = build_context(std::slice::from_ref(&window), &config);
    assert_eq!(
        (
            context.recent_messages[0].header.id.as_str(),
            context.source_turns[0].completed_event_seq
        ),
        ("message-0", 2)
    );
    let complete = build_context(
        &[window],
        &SessionPromptContextConfig {
            source_message_limit: None,
            recent_messages_limit: 3,
            ..config
        },
    );
    assert_eq!(complete.source_turns[0].completed_event_seq, 5);
}
