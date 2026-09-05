use super::RenderRuntimeEngine;
use crate::render_runtime::transcript::block::TranscriptBlockKind;
use talos_sdk::{
    AgentAssistantText, AgentAssistantTextPhase, AgentErrorInfo, AgentEventPayload,
    AgentMessageBlock, AgentMessageBlockCompleted, AgentMessageBlockContent,
    AgentMessageBlockDelta, AgentMessageBlockDeltaContent, AgentMessageBlockStarted,
    AgentMessageBlockStatus, AgentTurnFailed, AgentUserContent, ErrorCode,
};

fn pending_plain_text(engine: &RenderRuntimeEngine) -> Vec<String> {
    engine
        .pending_terminal_projection()
        .into_iter()
        .flat_map(|block| block.lines)
        .map(|line| line.plain_text())
        .collect()
}

#[test]
fn canonical_user_content_replaces_the_local_pending_input() {
    let mut engine = RenderRuntimeEngine::default();
    engine.append_user_input("local input");
    engine.apply_agent_event_payload(AgentEventPayload::MessageBlockStarted(
        AgentMessageBlockStarted {
            message_id: "message-1".to_string(),
            block: AgentMessageBlock {
                block_id: "user-1".to_string(),
                status: AgentMessageBlockStatus::Completed,
                content: AgentMessageBlockContent::UserContent(AgentUserContent::Text {
                    text: "server input".to_string(),
                }),
                started_at: 1,
                completed_at: Some(1),
            },
        },
    ));

    let user_blocks = engine
        .store()
        .blocks()
        .iter()
        .filter(|block| block.kind == TranscriptBlockKind::UserInput)
        .collect::<Vec<_>>();
    assert_eq!(user_blocks.len(), 1);
    assert_eq!(user_blocks[0].lines[0].content.plain_text(), "server input");
}

#[test]
fn canonical_assistant_block_lifecycle_finalizes_one_answer() {
    let mut engine = RenderRuntimeEngine::default();
    let started = AgentMessageBlock {
        block_id: "answer-1".to_string(),
        status: AgentMessageBlockStatus::Running,
        content: AgentMessageBlockContent::AssistantText(AgentAssistantText {
            phase: AgentAssistantTextPhase::FinalAnswer,
            text: String::new(),
        }),
        started_at: 1,
        completed_at: None,
    };
    engine.apply_agent_event_payload(AgentEventPayload::MessageBlockStarted(
        AgentMessageBlockStarted {
            message_id: "message-1".to_string(),
            block: started.clone(),
        },
    ));
    engine.apply_agent_event_payload(AgentEventPayload::MessageBlockDelta(
        AgentMessageBlockDelta {
            message_id: "message-1".to_string(),
            block_id: "answer-1".to_string(),
            delta: AgentMessageBlockDeltaContent::Text("hello".to_string()),
        },
    ));
    let mut completed = started;
    completed.status = AgentMessageBlockStatus::Completed;
    completed.completed_at = Some(2);
    completed.content = AgentMessageBlockContent::AssistantText(AgentAssistantText {
        phase: AgentAssistantTextPhase::FinalAnswer,
        text: "hello".to_string(),
    });
    engine.apply_agent_event_payload(AgentEventPayload::MessageBlockCompleted(
        AgentMessageBlockCompleted {
            message_id: "message-1".to_string(),
            block: completed,
        },
    ));

    assert_eq!(pending_plain_text(&engine), vec!["hello"]);
}

#[test]
fn canonical_turn_failure_adds_one_system_notice() {
    let mut engine = RenderRuntimeEngine::default();
    engine.apply_agent_event_payload(AgentEventPayload::TurnFailed(AgentTurnFailed {
        error: AgentErrorInfo {
            code: ErrorCode::InternalError,
            message: "boom".to_string(),
            retryable: false,
        },
    }));

    assert_eq!(pending_plain_text(&engine), vec!["Stream error: boom"]);
}
