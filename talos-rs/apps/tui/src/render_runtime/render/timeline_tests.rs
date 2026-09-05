use super::{apply_block_completed, apply_block_delta, apply_block_started};
use crate::render_runtime::render::live::LiveReducer;
use crate::render_runtime::transcript::block::TranscriptBlockKind;
use talos_sdk::{
    AgentAssistantText, AgentAssistantTextPhase, AgentMessageBlock, AgentMessageBlockContent,
    AgentMessageBlockDeltaContent, AgentMessageBlockStatus, AgentToolCall, AgentToolCallArguments,
    AgentToolProgress, AgentToolResult, TalosContentItem, TalosToolProgressKind,
};

fn block(block_id: &str, content: AgentMessageBlockContent) -> AgentMessageBlock {
    AgentMessageBlock {
        block_id: block_id.to_string(),
        status: AgentMessageBlockStatus::Running,
        content,
        started_at: 1,
        completed_at: None,
    }
}

#[test]
fn assistant_text_remains_live_until_the_canonical_block_completes() {
    let mut live = LiveReducer::default();
    let started = block(
        "answer-1",
        AgentMessageBlockContent::AssistantText(AgentAssistantText {
            phase: AgentAssistantTextPhase::FinalAnswer,
            text: String::new(),
        }),
    );

    assert!(apply_block_started(&mut live, &started).is_empty());
    assert!(apply_block_delta(
        &mut live,
        &started,
        AgentMessageBlockDeltaContent::Text("hello".to_string()),
    )
    .is_empty());

    let mut completed = started;
    completed.status = AgentMessageBlockStatus::Completed;
    completed.completed_at = Some(2);
    completed.content = AgentMessageBlockContent::AssistantText(AgentAssistantText {
        phase: AgentAssistantTextPhase::FinalAnswer,
        text: "hello".to_string(),
    });
    let finalized = apply_block_completed(&mut live, &completed, None);

    assert_eq!(finalized.len(), 1);
    assert_eq!(finalized[0].kind, TranscriptBlockKind::FinalAnswer);
    assert_eq!(finalized[0].lines[0].plain_text(), "hello");
}

#[test]
fn tool_result_completes_the_call_identified_by_call_id() {
    let mut live = LiveReducer::default();
    let tool = block(
        "tool-1",
        AgentMessageBlockContent::ToolCall(AgentToolCall {
            call_id: "call-1".to_string(),
            name: "Read".to_string(),
            arguments: AgentToolCallArguments::Streaming {
                json_fragment: r#"{"file_path":"/tmp/demo.py"}"#.to_string(),
            },
            progress: Vec::new(),
        }),
    );
    assert!(apply_block_started(&mut live, &tool).is_empty());
    assert!(apply_block_delta(
        &mut live,
        &tool,
        AgentMessageBlockDeltaContent::ToolProgress(AgentToolProgress {
            kind: TalosToolProgressKind::Stdout,
            text: "Read 1 line".to_string(),
            occurred_at: 2,
        }),
    )
    .is_empty());

    let result = AgentMessageBlock {
        block_id: "result-1".to_string(),
        status: AgentMessageBlockStatus::Completed,
        content: AgentMessageBlockContent::ToolResult(AgentToolResult {
            call_id: "call-1".to_string(),
            tool_name: "Read".to_string(),
            content: vec![TalosContentItem::Text {
                text: "ok".to_string(),
            }],
            is_error: false,
        }),
        started_at: 3,
        completed_at: Some(3),
    };
    let finalized = apply_block_completed(&mut live, &result, Some("tool-1"));

    assert_eq!(finalized[0].kind, TranscriptBlockKind::Tool);
    assert_eq!(
        finalized[0].lines[0].plain_text(),
        "● Read(/tmp/demo.py) completed"
    );
}
