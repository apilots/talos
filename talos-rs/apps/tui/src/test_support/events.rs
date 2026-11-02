use talos_sdk::{
    AgentAssistantText, AgentAssistantTextPhase, AgentContextCompaction,
    AgentContextCompactionDelta, AgentEvent, AgentEventPayload, AgentExecutionSummary,
    AgentMessage, AgentMessageBlock, AgentMessageBlockCompleted, AgentMessageBlockContent,
    AgentMessageBlockDelta, AgentMessageBlockDeltaContent, AgentMessageBlockStarted,
    AgentMessageBlockStatus, AgentMessageRole, AgentMessageStatus, AgentReasoning, AgentRunMetrics,
    AgentRunOutput, AgentRunResult, AgentStopReason, AgentToolCall, AgentToolCallArguments,
    AgentToolProgress, AgentToolResult, AgentTurnCompleted, AgentTurnRef, AgentTurnStarted,
    AgentUserContent, ContextCompactionOutcome, ContextCompactionStage, ContextCompactionTrigger,
    SessionHistoryResponse, SessionHistoryTurn, TalosContentItem, TimelineEvent,
};

pub(crate) fn completed_text_message(
    message_id: &str,
    role: AgentMessageRole,
    text: &str,
) -> AgentMessage {
    let content = match role {
        AgentMessageRole::User => AgentMessageBlockContent::UserContent(AgentUserContent::Text {
            text: text.to_string(),
        }),
        AgentMessageRole::Agent => AgentMessageBlockContent::AssistantText(AgentAssistantText {
            phase: AgentAssistantTextPhase::FinalAnswer,
            text: text.to_string(),
        }),
        AgentMessageRole::Tool | AgentMessageRole::System => {
            panic!("text fixture supports only User and Agent messages")
        }
    };
    AgentMessage {
        message_id: message_id.to_string(),
        turn_id: "turn-history".to_string(),
        role,
        status: AgentMessageStatus::Completed,
        blocks: vec![AgentMessageBlock {
            block_id: format!("block-{message_id}"),
            status: AgentMessageBlockStatus::Completed,
            content,
            started_at: 1,
            completed_at: Some(2),
        }],
        created_at: 1,
        completed_at: Some(2),
    }
}

pub(crate) fn completed_history_response(
    messages: Vec<AgentMessage>,
    duration_ms: Option<u64>,
) -> SessionHistoryResponse {
    SessionHistoryResponse {
        data: vec![SessionHistoryTurn {
            notices: vec![],
            identity: AgentTurnRef::new("sess-test", "turn-history"),
            result: AgentRunResult::Completed(AgentRunOutput {
                messages,
                metrics: AgentRunMetrics {
                    duration_ms,
                    ..AgentRunMetrics::default()
                },
                completion: AgentTurnCompleted {
                    stop_reason: AgentStopReason::EndTurn,
                },
            }),
            completed_event_seq: 9,
            completed_at: 2,
        }],
        next_cursor: None,
        last_stable_event_id: Some("ev_00000000000000000009".to_string()),
    }
}

pub(crate) fn turn_started_event(session_id: impl Into<String>, sequence: u64) -> TimelineEvent {
    timeline_event(
        session_id,
        sequence,
        AgentEventPayload::TurnStarted(AgentTurnStarted {}),
    )
}

pub(crate) fn assistant_block_started_event(
    message_id: impl Into<String>,
    session_id: impl Into<String>,
    sequence: u64,
) -> TimelineEvent {
    assistant_block_started_event_with_phase(
        message_id,
        session_id,
        sequence,
        "final-answer",
        AgentAssistantTextPhase::FinalAnswer,
        String::new(),
    )
}

pub(crate) fn assistant_text_delta_event(
    message_id: impl Into<String>,
    session_id: impl Into<String>,
    sequence: u64,
    delta: impl Into<String>,
) -> TimelineEvent {
    block_text_delta_event(message_id, session_id, sequence, "final-answer", delta)
}

pub(crate) fn assistant_block_completed_event(
    message_id: impl Into<String>,
    session_id: impl Into<String>,
    sequence: u64,
    text: impl Into<String>,
) -> TimelineEvent {
    assistant_block_completed_event_for_block(
        message_id,
        session_id,
        sequence,
        "final-answer",
        text,
    )
}

pub(crate) fn assistant_block_completed_event_for_block(
    message_id: impl Into<String>,
    session_id: impl Into<String>,
    sequence: u64,
    block_id: impl Into<String>,
    text: impl Into<String>,
) -> TimelineEvent {
    let block = AgentMessageBlock {
        block_id: block_id.into(),
        status: AgentMessageBlockStatus::Completed,
        content: AgentMessageBlockContent::AssistantText(AgentAssistantText {
            phase: AgentAssistantTextPhase::FinalAnswer,
            text: text.into(),
        }),
        started_at: sequence as i64,
        completed_at: Some(sequence as i64),
    };
    timeline_event(
        session_id,
        sequence,
        AgentEventPayload::MessageBlockCompleted(AgentMessageBlockCompleted {
            message_id: message_id.into(),
            block,
        }),
    )
}

pub(crate) fn commentary_block_started_event(
    message_id: impl Into<String>,
    session_id: impl Into<String>,
    sequence: u64,
) -> TimelineEvent {
    assistant_block_started_event_with_phase(
        message_id,
        session_id,
        sequence,
        "commentary",
        AgentAssistantTextPhase::Commentary,
        String::new(),
    )
}

pub(crate) fn commentary_delta_event(
    message_id: impl Into<String>,
    session_id: impl Into<String>,
    sequence: u64,
    delta: impl Into<String>,
) -> TimelineEvent {
    block_text_delta_event(message_id, session_id, sequence, "commentary", delta)
}

pub(crate) fn pending_assistant_block_started_event(
    message_id: impl Into<String>,
    session_id: impl Into<String>,
    sequence: u64,
) -> TimelineEvent {
    assistant_block_started_event_with_phase(
        message_id,
        session_id,
        sequence,
        "pending-answer",
        AgentAssistantTextPhase::Pending,
        String::new(),
    )
}

pub(crate) fn assistant_block_started_event_with_phase(
    message_id: impl Into<String>,
    session_id: impl Into<String>,
    sequence: u64,
    block_id: impl Into<String>,
    phase: AgentAssistantTextPhase,
    text: impl Into<String>,
) -> TimelineEvent {
    let block = AgentMessageBlock {
        block_id: block_id.into(),
        status: AgentMessageBlockStatus::Running,
        content: AgentMessageBlockContent::AssistantText(AgentAssistantText {
            phase,
            text: text.into(),
        }),
        started_at: sequence as i64,
        completed_at: None,
    };
    timeline_event(
        session_id,
        sequence,
        AgentEventPayload::MessageBlockStarted(AgentMessageBlockStarted {
            message_id: message_id.into(),
            block,
        }),
    )
}

pub(crate) fn pending_assistant_text_delta_event(
    message_id: impl Into<String>,
    session_id: impl Into<String>,
    sequence: u64,
    delta: impl Into<String>,
) -> TimelineEvent {
    block_text_delta_event(message_id, session_id, sequence, "pending-answer", delta)
}

pub(crate) fn block_text_delta_event(
    message_id: impl Into<String>,
    session_id: impl Into<String>,
    sequence: u64,
    block_id: impl Into<String>,
    delta: impl Into<String>,
) -> TimelineEvent {
    timeline_event(
        session_id,
        sequence,
        AgentEventPayload::MessageBlockDelta(AgentMessageBlockDelta {
            message_id: message_id.into(),
            block_id: block_id.into(),
            delta: AgentMessageBlockDeltaContent::Text(delta.into()),
        }),
    )
}

pub(crate) fn assistant_phase_delta_event(
    message_id: impl Into<String>,
    session_id: impl Into<String>,
    sequence: u64,
    phase: AgentAssistantTextPhase,
) -> TimelineEvent {
    assistant_phase_delta_event_for_block(message_id, session_id, sequence, "pending-answer", phase)
}

pub(crate) fn assistant_phase_delta_event_for_block(
    message_id: impl Into<String>,
    session_id: impl Into<String>,
    sequence: u64,
    block_id: impl Into<String>,
    phase: AgentAssistantTextPhase,
) -> TimelineEvent {
    timeline_event(
        session_id,
        sequence,
        AgentEventPayload::MessageBlockDelta(AgentMessageBlockDelta {
            message_id: message_id.into(),
            block_id: block_id.into(),
            delta: AgentMessageBlockDeltaContent::AssistantTextPhase(phase),
        }),
    )
}

pub(crate) fn reasoning_block_started_event(
    message_id: impl Into<String>,
    session_id: impl Into<String>,
    sequence: u64,
    text: impl Into<String>,
) -> TimelineEvent {
    let block = AgentMessageBlock {
        block_id: "reasoning".to_string(),
        status: AgentMessageBlockStatus::Running,
        content: AgentMessageBlockContent::Reasoning(AgentReasoning { text: text.into() }),
        started_at: sequence as i64,
        completed_at: None,
    };
    timeline_event(
        session_id,
        sequence,
        AgentEventPayload::MessageBlockStarted(AgentMessageBlockStarted {
            message_id: message_id.into(),
            block,
        }),
    )
}

pub(crate) fn turn_completed_event(session_id: impl Into<String>, sequence: u64) -> TimelineEvent {
    timeline_event(
        session_id,
        sequence,
        AgentEventPayload::TurnCompleted(AgentTurnCompleted {
            stop_reason: AgentStopReason::EndTurn,
        }),
    )
}

pub(crate) fn metrics_updated_event(
    session_id: impl Into<String>,
    sequence: u64,
    metrics: AgentRunMetrics,
) -> TimelineEvent {
    timeline_event(
        session_id,
        sequence,
        AgentEventPayload::MetricsUpdated(metrics),
    )
}

pub(crate) fn execution_summary_completed_event(
    message_id: impl Into<String>,
    session_id: impl Into<String>,
    sequence: u64,
    summary: impl Into<String>,
) -> TimelineEvent {
    let block = AgentMessageBlock {
        block_id: format!("summary-{sequence}"),
        status: AgentMessageBlockStatus::Completed,
        content: AgentMessageBlockContent::ExecutionSummary(AgentExecutionSummary {
            text: summary.into(),
        }),
        started_at: sequence as i64,
        completed_at: Some(sequence as i64),
    };
    timeline_event(
        session_id,
        sequence,
        AgentEventPayload::MessageBlockCompleted(AgentMessageBlockCompleted {
            message_id: message_id.into(),
            block,
        }),
    )
}

pub(crate) fn context_compaction_block_started_event(
    message_id: impl Into<String>,
    session_id: impl Into<String>,
    sequence: u64,
    stage: ContextCompactionStage,
    message: impl Into<String>,
) -> TimelineEvent {
    let block = AgentMessageBlock {
        block_id: "context-compaction".to_string(),
        status: AgentMessageBlockStatus::Running,
        content: AgentMessageBlockContent::ContextCompaction(AgentContextCompaction {
            trigger: ContextCompactionTrigger::ThresholdExceeded,
            stage,
            outcome: None,
            metrics: None,
            message: Some(message.into()),
        }),
        started_at: sequence as i64,
        completed_at: None,
    };
    timeline_event(
        session_id,
        sequence,
        AgentEventPayload::MessageBlockStarted(AgentMessageBlockStarted {
            message_id: message_id.into(),
            block,
        }),
    )
}

pub(crate) fn context_compaction_delta_event(
    message_id: impl Into<String>,
    session_id: impl Into<String>,
    sequence: u64,
    stage: ContextCompactionStage,
    outcome: Option<ContextCompactionOutcome>,
    message: impl Into<String>,
) -> TimelineEvent {
    timeline_event(
        session_id,
        sequence,
        AgentEventPayload::MessageBlockDelta(AgentMessageBlockDelta {
            message_id: message_id.into(),
            block_id: "context-compaction".to_string(),
            delta: AgentMessageBlockDeltaContent::ContextCompaction(AgentContextCompactionDelta {
                stage: Some(stage),
                outcome,
                metrics: None,
                message: Some(message.into()),
            }),
        }),
    )
}

pub(crate) fn tool_call_block_started_event(
    message_id: impl Into<String>,
    session_id: impl Into<String>,
    sequence: u64,
    call_id: impl Into<String>,
    name: impl Into<String>,
    arguments: serde_json::Value,
) -> TimelineEvent {
    let call_id = call_id.into();
    let block = AgentMessageBlock {
        block_id: format!("tool-{call_id}"),
        status: AgentMessageBlockStatus::Running,
        content: AgentMessageBlockContent::ToolCall(AgentToolCall {
            call_id,
            name: name.into(),
            arguments: AgentToolCallArguments::Streaming {
                json_fragment: arguments.to_string(),
            },
            progress: Vec::new(),
        }),
        started_at: sequence as i64,
        completed_at: None,
    };
    timeline_event(
        session_id,
        sequence,
        AgentEventPayload::MessageBlockStarted(AgentMessageBlockStarted {
            message_id: message_id.into(),
            block,
        }),
    )
}

pub(crate) fn tool_result_block_completed_event(
    message_id: impl Into<String>,
    session_id: impl Into<String>,
    sequence: u64,
    call_id: impl Into<String>,
    name: impl Into<String>,
    output: impl Into<String>,
    is_error: bool,
) -> TimelineEvent {
    let call_id = call_id.into();
    let block = AgentMessageBlock {
        block_id: format!("tool-result-{call_id}"),
        status: if is_error {
            AgentMessageBlockStatus::Failed
        } else {
            AgentMessageBlockStatus::Completed
        },
        content: AgentMessageBlockContent::ToolResult(AgentToolResult {
            metrics: None,
            call_id,
            tool_name: name.into(),
            content: vec![TalosContentItem::Text {
                text: output.into(),
            }],
            is_error,
        }),
        started_at: sequence as i64,
        completed_at: Some(sequence as i64),
    };
    timeline_event(
        session_id,
        sequence,
        AgentEventPayload::MessageBlockCompleted(AgentMessageBlockCompleted {
            message_id: message_id.into(),
            block,
        }),
    )
}

pub(crate) fn tool_progress_event(
    message_id: impl Into<String>,
    session_id: impl Into<String>,
    sequence: u64,
    call_id: impl Into<String>,
    progress: AgentToolProgress,
) -> TimelineEvent {
    timeline_event(
        session_id,
        sequence,
        AgentEventPayload::MessageBlockDelta(AgentMessageBlockDelta {
            message_id: message_id.into(),
            block_id: format!("tool-{}", call_id.into()),
            delta: AgentMessageBlockDeltaContent::ToolProgress(progress),
        }),
    )
}

pub(crate) fn timeline_event(
    session_id: impl Into<String>,
    sequence: u64,
    payload: AgentEventPayload,
) -> TimelineEvent {
    let event_id = format!("ev_{sequence}");
    TimelineEvent {
        event_id,
        event_seq: sequence as i64,
        persisted_at: sequence as i64,
        event: talos_sdk::TimelineEventPayload::Agent(AgentEvent {
            identity: AgentTurnRef::new(session_id, "turn-test"),
            sequence,
            occurred_at: sequence as i64,
            payload,
        }),
    }
}
