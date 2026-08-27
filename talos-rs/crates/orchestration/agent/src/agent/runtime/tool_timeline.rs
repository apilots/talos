use talos_agent_kernel::{TimelineBuilder, ToolProgressKind, ToolStreamKey};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_types::talos::{
    AgentEvent, RunItem, TalosTimelineDelta, TalosTimelineItemStatus, TalosToolProgressKind,
};

use super::{close_active_reasoning_item, item_id_from_started, KernelTimelineState};

pub(super) fn append_text_item_delta(
    timeline: &mut TimelineBuilder,
    item_id_slot: &mut Option<String>,
    text: String,
) -> TalosResult<Vec<AgentEvent>> {
    let mut events = Vec::new();
    let item_id = match item_id_slot.clone() {
        Some(item_id) => item_id,
        None => {
            let started = timeline.start_reasoning();
            let item_id = item_id_from_started(&started)?;
            *item_id_slot = Some(item_id.clone());
            events.push(started);
            item_id
        }
    };
    events.push(timeline.append_delta(&item_id, TalosTimelineDelta::Text { text })?);
    Ok(events)
}

pub(super) fn emit_missing_tool_items_from_result(
    timeline: &mut TimelineBuilder,
    state: &mut KernelTimelineState,
    output_items: &[RunItem],
) -> TalosResult<Vec<AgentEvent>> {
    let mut events = Vec::new();
    for output_item in output_items {
        match output_item {
            RunItem::ToolCall {
                call_id,
                name,
                arguments,
            } if !state.tool_item_by_call_id.contains_key(call_id) => {
                events.extend(close_active_reasoning_item(timeline, state)?);
                let started = timeline.start_tool_call(call_id, name, arguments.to_string());
                let item_id = item_id_from_started(&started)?;
                state
                    .tool_item_by_call_id
                    .insert(call_id.clone(), item_id.clone());
                state.active_tool_items.insert(call_id.clone(), item_id);
                events.push(started);
            }
            RunItem::ToolResult {
                call_id,
                tool_name,
                content,
                is_error,
            } if !state.completed_tool_call_ids.contains(call_id) => {
                events.extend(close_active_reasoning_item(timeline, state)?);
                let item_id = match state.tool_item_by_call_id.get(call_id).cloned() {
                    Some(item_id) => item_id,
                    None => {
                        let started = timeline.start_tool_call(call_id, tool_name, String::new());
                        let item_id = item_id_from_started(&started)?;
                        state
                            .tool_item_by_call_id
                            .insert(call_id.clone(), item_id.clone());
                        state
                            .active_tool_items
                            .insert(call_id.clone(), item_id.clone());
                        events.push(started);
                        item_id
                    }
                };
                let result = timeline.append_delta(
                    &item_id,
                    TalosTimelineDelta::ToolResult {
                        content: content.clone(),
                        is_error: *is_error,
                    },
                )?;
                events.push(result);
                events.push(timeline.complete_item(
                    &item_id,
                    if *is_error {
                        TalosTimelineItemStatus::Failed
                    } else {
                        TalosTimelineItemStatus::Completed
                    },
                    None,
                )?);
                state.completed_tool_call_ids.insert(call_id.clone());
            }
            _ => {}
        }
    }
    Ok(events)
}

fn tool_item_id(
    state: &KernelTimelineState,
    stream_key: ToolStreamKey,
    call_id: Option<&str>,
) -> TalosResult<String> {
    if let Some(call_id) = call_id {
        if let Some(item_id) = state.tool_item_by_call_id.get(call_id) {
            return Ok(item_id.clone());
        }
    }
    state
        .active_tool_items
        .get(&stream_key_key(&stream_key))
        .cloned()
        .ok_or_else(|| {
            TalosError::public(
                ErrorCode::InvalidRequest,
                format!(
                    "timeline tool item not found for stream key round={} index={}",
                    stream_key.round, stream_key.index
                ),
            )
        })
}

pub(super) fn ensure_tool_item(
    timeline: &mut TimelineBuilder,
    state: &mut KernelTimelineState,
    stream_key: ToolStreamKey,
    call_id: Option<&str>,
    tool_name: Option<&str>,
    arguments: String,
) -> TalosResult<(String, Option<AgentEvent>)> {
    if let Ok(item_id) = tool_item_id(state, stream_key.clone(), call_id) {
        return Ok((item_id, None));
    }

    let call_id = call_id
        .map(ToString::to_string)
        .unwrap_or_else(|| stream_key_fallback(&stream_key));
    let tool_name = tool_name.unwrap_or("unknown");
    let started = timeline.start_tool_call(call_id.clone(), tool_name, arguments);
    let item_id = item_id_from_started(&started)?;
    state
        .active_tool_items
        .insert(stream_key_key(&stream_key), item_id.clone());
    state.tool_item_by_call_id.insert(call_id, item_id.clone());
    Ok((item_id, Some(started)))
}

pub(super) fn ensure_tool_item_for_call(
    timeline: &mut TimelineBuilder,
    state: &mut KernelTimelineState,
    call_id: &str,
    tool_name: &str,
) -> (String, Option<AgentEvent>) {
    if let Some(item_id) = state.tool_item_by_call_id.get(call_id).cloned() {
        return (item_id, None);
    }

    let started = timeline.start_tool_call(call_id, tool_name, String::new());
    let item_id = match item_id_from_started(&started) {
        Ok(item_id) => item_id,
        Err(_) => return (call_id.to_string(), Some(started)),
    };
    state
        .tool_item_by_call_id
        .insert(call_id.to_string(), item_id.clone());
    state
        .active_tool_items
        .insert(call_id.to_string(), item_id.clone());
    (item_id, Some(started))
}

pub(super) fn optional_started_with_delta(
    started: Option<AgentEvent>,
    delta: AgentEvent,
) -> Vec<AgentEvent> {
    match started {
        Some(started) => vec![started, delta],
        None => vec![delta],
    }
}

pub(super) fn map_timeline_tool_progress_kind(kind: ToolProgressKind) -> TalosToolProgressKind {
    match kind {
        ToolProgressKind::Status => TalosToolProgressKind::Status,
        ToolProgressKind::Stdout => TalosToolProgressKind::Stdout,
        ToolProgressKind::Stderr => TalosToolProgressKind::Stderr,
    }
}

pub(super) fn stream_key_fallback(stream_key: &ToolStreamKey) -> String {
    format!("tool_{}_{}", stream_key.round, stream_key.index)
}

fn stream_key_key(stream_key: &ToolStreamKey) -> String {
    format!("{}:{}", stream_key.round, stream_key.index)
}
