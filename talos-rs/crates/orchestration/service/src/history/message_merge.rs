use chrono::Utc;
use talos_store::session::TalosTurn;
use talos_types::talos::{
    AgentEvent, AgentEventPayload, RunItem, SessionHistoryMessage, StopReason, TalosContentItem,
    TalosTimelineContent, TalosTimelineDelta, TalosTimelineItem, TalosTimelineItemKind,
    TalosTimelineItemStatus, TalosTimelinePhase, TalosToolProgressEntry, TalosToolResult,
    TokenUsage,
};

pub(crate) fn merge_completed_timeline_message(
    messages: &mut Vec<SessionHistoryMessage>,
    timeline_message: SessionHistoryMessage,
) {
    if timeline_message.items.is_empty() {
        return;
    }
    if let Some(index) = messages
        .iter()
        .position(|message| message.message_id == timeline_message.message_id)
    {
        messages.retain(|message| message.message_id != timeline_message.message_id);
        messages.insert(index.min(messages.len()), timeline_message);
        return;
    }

    let timeline_input = first_input_text(&timeline_message).map(str::to_string);
    messages.retain(|message| {
        !(message.message_id.is_empty()
            && message.stop_reason == StopReason::Cancelled
            && timeline_input
                .as_deref()
                .is_some_and(|input| first_input_text(message) == Some(input)))
    });
    messages.push(timeline_message);
    messages.sort_by_key(|message| {
        message
            .items
            .first()
            .map(|item| item.started_at_ms)
            .unwrap_or_default()
    });
}

fn first_input_text(message: &SessionHistoryMessage) -> Option<&str> {
    message.items.iter().find_map(|item| {
        if item.phase != TalosTimelinePhase::Input {
            return None;
        }
        match &item.content {
            TalosTimelineContent::Text { text } => Some(text.as_str()),
            TalosTimelineContent::UserInput { text, .. } => Some(text.as_str()),
            TalosTimelineContent::Empty
            | TalosTimelineContent::ToolCall { .. }
            | TalosTimelineContent::ExecutionSummary { .. }
            | TalosTimelineContent::ContextCompaction { .. }
            | TalosTimelineContent::SystemNotice { .. } => None,
        }
    })
}

pub(crate) fn history_messages_from_completed_events(
    events: Vec<AgentEvent>,
) -> Vec<SessionHistoryMessage> {
    let mut messages = Vec::new();
    let mut current = Vec::new();
    let mut current_turn_id = String::new();
    let mut current_message_id = String::new();

    for event in events {
        let same_turn = event.turn_id == current_turn_id && event.message_id == current_message_id;
        if !same_turn && !current.is_empty() {
            if let Some(message) = history_message_from_completed_turn_events(
                &current_turn_id,
                &current_message_id,
                std::mem::take(&mut current),
            ) {
                messages.push(message);
            }
        }
        current_turn_id = event.turn_id.clone();
        current_message_id = event.message_id.clone();
        current.push(event);
    }

    if !current.is_empty() {
        if let Some(message) = history_message_from_completed_turn_events(
            &current_turn_id,
            &current_message_id,
            current,
        ) {
            messages.push(message);
        }
    }

    messages
}

fn history_message_from_completed_turn_events(
    turn_id: &str,
    message_id: &str,
    events: Vec<AgentEvent>,
) -> Option<SessionHistoryMessage> {
    let items = timeline_items_from_events(&events);
    let mut usage = TokenUsage::default();
    let mut metrics = None;
    let mut stop_reason = StopReason::EndTurn;
    let mut completed = false;

    for event in events {
        match event.payload {
            AgentEventPayload::TurnStarted(_) => {
                completed = false;
            }
            AgentEventPayload::TurnCompleted(turn_completed) => {
                usage = turn_completed.usage;
                metrics = turn_completed.metrics;
                stop_reason = turn_completed.stop_reason;
                completed = stop_reason != StopReason::WaitingUserAction;
            }
            AgentEventPayload::TurnError(_) => {
                completed = false;
            }
            AgentEventPayload::TurnMetrics(_)
            | AgentEventPayload::ItemStarted(_)
            | AgentEventPayload::ItemDelta(_)
            | AgentEventPayload::ItemCompleted(_)
            | AgentEventPayload::AssistantTextCommitted(_) => {}
        }
    }

    if !completed
        || items.is_empty()
        || items.iter().any(|item| {
            matches!(
                item.status,
                TalosTimelineItemStatus::Queued
                    | TalosTimelineItemStatus::Running
                    | TalosTimelineItemStatus::WaitingUserAction
            )
        })
    {
        return None;
    }

    Some(SessionHistoryMessage {
        message_id: message_id.to_string(),
        turn_id: turn_id.to_string(),
        items,
        stop_reason,
        usage,
        metrics,
    })
}

pub(crate) fn timeline_items_from_events(events: &[AgentEvent]) -> Vec<TalosTimelineItem> {
    let mut items = Vec::new();
    for event in events {
        match &event.payload {
            AgentEventPayload::ItemStarted(started) => {
                items.push(started.item.clone());
            }
            AgentEventPayload::ItemDelta(delta) => {
                if let Some(item) = items
                    .iter_mut()
                    .rev()
                    .find(|item| item.item_id == delta.item_id)
                {
                    apply_history_delta(item, delta.delta.clone());
                }
            }
            AgentEventPayload::ItemCompleted(item_completed) => {
                if let Some(item) = items
                    .iter_mut()
                    .rev()
                    .find(|item| item.item_id == item_completed.item_id)
                {
                    item.status = item_completed.status;
                    item.completed_at_ms = Some(item_completed.completed_at_ms);
                    if let Some(final_content) = &item_completed.final_content {
                        item.content = final_content.clone();
                    }
                }
            }
            AgentEventPayload::AssistantTextCommitted(committed) => {
                if let Some(item) = items
                    .iter_mut()
                    .rev()
                    .find(|item| item.item_id == committed.item_id)
                {
                    item.phase = committed.phase;
                }
            }
            AgentEventPayload::TurnCompleted(_)
            | AgentEventPayload::TurnStarted(_)
            | AgentEventPayload::TurnMetrics(_)
            | AgentEventPayload::TurnError(_) => {}
        }
    }
    items
}

fn apply_history_delta(item: &mut TalosTimelineItem, delta: TalosTimelineDelta) {
    match delta {
        TalosTimelineDelta::Text { text } => match &mut item.content {
            TalosTimelineContent::Text { text: existing } => existing.push_str(&text),
            TalosTimelineContent::Empty => {
                item.content = TalosTimelineContent::Text { text };
            }
            TalosTimelineContent::ToolCall { .. }
            | TalosTimelineContent::UserInput { .. }
            | TalosTimelineContent::ExecutionSummary { .. }
            | TalosTimelineContent::ContextCompaction { .. }
            | TalosTimelineContent::SystemNotice { .. } => {}
        },
        TalosTimelineDelta::ToolArguments { arguments_delta } => {
            if let TalosTimelineContent::ToolCall { arguments, .. } = &mut item.content {
                arguments.push_str(&arguments_delta);
            }
        }
        TalosTimelineDelta::ToolProgress {
            progress_kind,
            text_delta,
        } => {
            if let TalosTimelineContent::ToolCall { progress, .. } = &mut item.content {
                progress.push(TalosToolProgressEntry {
                    progress_kind,
                    text: text_delta,
                    timestamp_ms: Utc::now().timestamp_millis().max(0) as u64,
                });
            }
        }
        TalosTimelineDelta::ToolResult { content, is_error } => {
            if let TalosTimelineContent::ToolCall { result, .. } = &mut item.content {
                *result = Some(TalosToolResult { content, is_error });
            }
        }
        TalosTimelineDelta::UserAction { event } => {
            if matches!(
                event,
                talos_types::TimelineUserActionEvent::Requested { .. }
            ) {
                item.status = TalosTimelineItemStatus::WaitingUserAction;
            }
        }
        TalosTimelineDelta::ContextCompaction {
            stage,
            outcome,
            metrics,
            message,
        } => {
            if let TalosTimelineContent::ContextCompaction {
                stage: current_stage,
                outcome: current_outcome,
                metrics: current_metrics,
                message: current_message,
                ..
            } = &mut item.content
            {
                if let Some(stage) = stage {
                    *current_stage = stage;
                }
                if outcome.is_some() {
                    *current_outcome = outcome;
                }
                if metrics.is_some() {
                    *current_metrics = metrics;
                }
                if message.is_some() {
                    *current_message = message;
                }
            }
        }
        TalosTimelineDelta::Status { status } => {
            item.status = status;
        }
    }
}

pub(crate) fn map_turns_to_history_messages(turns: Vec<TalosTurn>) -> Vec<SessionHistoryMessage> {
    turns
        .into_iter()
        .map(|turn| {
            let items = map_turn_items(&turn);
            SessionHistoryMessage {
                message_id: turn.message_id,
                turn_id: turn.turn_id,
                items,
                stop_reason: turn.stop_reason,
                usage: turn.usage,
                metrics: None,
            }
        })
        .collect()
}

fn map_turn_items(turn: &TalosTurn) -> Vec<TalosTimelineItem> {
    let timestamp_ms = turn.timestamp.timestamp_millis().max(0) as u64;
    let mut items: Vec<TalosTimelineItem> = Vec::new();

    for (index, output) in turn.items.iter().enumerate() {
        match output {
            RunItem::UserInput { text, file_ids } => {
                items.push(TalosTimelineItem {
                    item_id: history_item_id(turn, index, "input"),
                    parent_item_id: None,
                    kind: TalosTimelineItemKind::UserInput,
                    phase: TalosTimelinePhase::Input,
                    status: TalosTimelineItemStatus::Completed,
                    title: None,
                    started_at_ms: timestamp_ms,
                    completed_at_ms: Some(timestamp_ms),
                    content: TalosTimelineContent::UserInput {
                        text: text.clone(),
                        file_ids: file_ids.clone(),
                    },
                });
            }
            RunItem::Reasoning { text } => {
                items.push(history_text_item(
                    turn,
                    index,
                    TalosTimelineItemKind::Reasoning,
                    TalosTimelinePhase::Thinking,
                    Some("Thinking".to_string()),
                    text.clone(),
                    timestamp_ms,
                ));
            }
            RunItem::ExecutionSummary { text } => {
                items.push(TalosTimelineItem {
                    item_id: history_item_id(turn, index, "summary"),
                    parent_item_id: None,
                    kind: TalosTimelineItemKind::ExecutionSummary,
                    phase: TalosTimelinePhase::System,
                    status: TalosTimelineItemStatus::Completed,
                    title: Some("Execution summary".to_string()),
                    started_at_ms: timestamp_ms,
                    completed_at_ms: Some(timestamp_ms),
                    content: TalosTimelineContent::ExecutionSummary {
                        summary: text.clone(),
                    },
                });
            }
            RunItem::OutputText { text } => {
                items.push(history_text_item(
                    turn,
                    index,
                    TalosTimelineItemKind::AssistantText,
                    TalosTimelinePhase::FinalAnswer,
                    None,
                    text.clone(),
                    timestamp_ms,
                ));
            }
            RunItem::ToolCall {
                call_id,
                name,
                arguments,
            } => {
                items.push(TalosTimelineItem {
                    item_id: history_item_id(turn, index, "tool"),
                    parent_item_id: None,
                    kind: TalosTimelineItemKind::ToolCall,
                    phase: TalosTimelinePhase::Acting,
                    status: TalosTimelineItemStatus::Completed,
                    title: None,
                    started_at_ms: timestamp_ms,
                    completed_at_ms: Some(timestamp_ms),
                    content: TalosTimelineContent::ToolCall {
                        call_id: call_id.clone(),
                        name: name.clone(),
                        arguments: arguments.to_string(),
                        progress: Vec::new(),
                        result: None,
                    },
                });
            }
            RunItem::ToolResult {
                call_id,
                tool_name,
                content,
                is_error,
            } => {
                if let Some(tool_item) = items.iter_mut().rev().find(|item| {
                    matches!(
                        &item.content,
                        TalosTimelineContent::ToolCall { call_id: existing, .. }
                            if existing == call_id
                    )
                }) {
                    tool_item.status = if *is_error {
                        TalosTimelineItemStatus::Failed
                    } else {
                        TalosTimelineItemStatus::Completed
                    };
                    if let TalosTimelineContent::ToolCall { result, .. } = &mut tool_item.content {
                        *result = Some(TalosToolResult {
                            content: content.clone(),
                            is_error: *is_error,
                        });
                    }
                } else {
                    items.push(history_tool_result_item(
                        turn,
                        index,
                        call_id,
                        tool_name,
                        content,
                        *is_error,
                        timestamp_ms,
                    ));
                }
            }
        }
    }

    items
}

fn history_text_item(
    turn: &TalosTurn,
    index: usize,
    kind: TalosTimelineItemKind,
    phase: TalosTimelinePhase,
    title: Option<String>,
    text: String,
    timestamp_ms: u64,
) -> TalosTimelineItem {
    TalosTimelineItem {
        item_id: history_item_id(turn, index, item_prefix(kind)),
        parent_item_id: None,
        kind,
        phase,
        status: TalosTimelineItemStatus::Completed,
        title,
        started_at_ms: timestamp_ms,
        completed_at_ms: Some(timestamp_ms),
        content: TalosTimelineContent::Text { text },
    }
}

fn history_tool_result_item(
    turn: &TalosTurn,
    index: usize,
    call_id: &str,
    tool_name: &str,
    content: &[TalosContentItem],
    is_error: bool,
    timestamp_ms: u64,
) -> TalosTimelineItem {
    TalosTimelineItem {
        item_id: history_item_id(turn, index, "tool"),
        parent_item_id: None,
        kind: TalosTimelineItemKind::ToolCall,
        phase: TalosTimelinePhase::Acting,
        status: if is_error {
            TalosTimelineItemStatus::Failed
        } else {
            TalosTimelineItemStatus::Completed
        },
        title: None,
        started_at_ms: timestamp_ms,
        completed_at_ms: Some(timestamp_ms),
        content: TalosTimelineContent::ToolCall {
            call_id: call_id.to_string(),
            name: tool_name.to_string(),
            arguments: String::new(),
            progress: Vec::new(),
            result: Some(TalosToolResult {
                content: content.to_vec(),
                is_error,
            }),
        },
    }
}

fn history_item_id(turn: &TalosTurn, index: usize, prefix: &str) -> String {
    format!("history-{}-{prefix}-{index}", turn.turn_id)
}

fn item_prefix(kind: TalosTimelineItemKind) -> &'static str {
    match kind {
        TalosTimelineItemKind::UserInput => "input",
        TalosTimelineItemKind::Reasoning => "reasoning",
        TalosTimelineItemKind::AssistantText => "assistant",
        TalosTimelineItemKind::ToolCall => "tool",
        TalosTimelineItemKind::ExecutionSummary => "summary",
        TalosTimelineItemKind::ContextCompaction => "compaction",
        TalosTimelineItemKind::SystemNotice => "system",
    }
}
