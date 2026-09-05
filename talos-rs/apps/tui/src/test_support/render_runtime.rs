use crate::render_runtime::render::live::LiveTextKind;
use crate::render_runtime::runtime::engine::RenderRuntimeEngine;
use talos_sdk::{
    AgentAssistantText, AgentAssistantTextPhase, AgentEventPayload, AgentMessageBlock,
    AgentMessageBlockContent, AgentMessageBlockDelta, AgentMessageBlockDeltaContent,
    AgentMessageBlockStarted, AgentMessageBlockStatus, AgentReasoning, AgentToolCall,
    AgentToolCallArguments, AgentToolProgress, TalosToolProgressKind,
};

pub(crate) fn start_live_text(
    engine: &mut RenderRuntimeEngine,
    block_id: &str,
    kind: LiveTextKind,
    text: &str,
) {
    engine.apply_agent_event_payload(AgentEventPayload::MessageBlockStarted(
        AgentMessageBlockStarted {
            message_id: "message-test".to_string(),
            block: text_block(block_id, kind, text, AgentMessageBlockStatus::Running),
        },
    ));
}

pub(crate) fn start_live_tool(
    engine: &mut RenderRuntimeEngine,
    block_id: &str,
    call_id: &str,
    name: &str,
    arguments: &str,
) {
    engine.apply_agent_event_payload(AgentEventPayload::MessageBlockStarted(
        AgentMessageBlockStarted {
            message_id: "message-test".to_string(),
            block: AgentMessageBlock {
                block_id: block_id.to_string(),
                status: AgentMessageBlockStatus::Running,
                content: AgentMessageBlockContent::ToolCall(AgentToolCall {
                    call_id: call_id.to_string(),
                    name: name.to_string(),
                    arguments: AgentToolCallArguments::Streaming {
                        json_fragment: arguments.to_string(),
                    },
                    progress: Vec::new(),
                }),
                started_at: 1,
                completed_at: None,
            },
        },
    ));
}

pub(crate) fn append_live_tool_progress(
    engine: &mut RenderRuntimeEngine,
    block_id: &str,
    text: &str,
) {
    engine.apply_agent_event_payload(AgentEventPayload::MessageBlockDelta(
        AgentMessageBlockDelta {
            message_id: "message-test".to_string(),
            block_id: block_id.to_string(),
            delta: AgentMessageBlockDeltaContent::ToolProgress(AgentToolProgress {
                kind: TalosToolProgressKind::Stdout,
                text: text.to_string(),
                occurred_at: 1,
            }),
        },
    ));
}

fn text_block(
    block_id: &str,
    kind: LiveTextKind,
    text: &str,
    status: AgentMessageBlockStatus,
) -> AgentMessageBlock {
    let content = match kind {
        LiveTextKind::Commentary => AgentMessageBlockContent::AssistantText(AgentAssistantText {
            phase: AgentAssistantTextPhase::Commentary,
            text: text.to_string(),
        }),
        LiveTextKind::Thinking => AgentMessageBlockContent::Reasoning(AgentReasoning {
            text: text.to_string(),
        }),
        LiveTextKind::FinalAnswer => AgentMessageBlockContent::AssistantText(AgentAssistantText {
            phase: AgentAssistantTextPhase::FinalAnswer,
            text: text.to_string(),
        }),
    };
    AgentMessageBlock {
        block_id: block_id.to_string(),
        status,
        content,
        started_at: 1,
        completed_at: (status == AgentMessageBlockStatus::Completed).then_some(2),
    }
}
