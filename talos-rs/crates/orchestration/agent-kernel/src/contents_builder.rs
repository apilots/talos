use crate::execution::ContentsMode;
use crate::{
    ExecutionBranch, InvocationSession, KernelTranscriptEvent, KernelTranscriptEventKind,
    KernelTranscriptPayload,
};
use talos_llm_provider::ChatMessage;

pub struct ContentsBuilder;

impl ContentsBuilder {
    pub fn build_messages(
        session: &InvocationSession,
        invocation_id: &str,
        branch: ExecutionBranch,
        mode: ContentsMode,
    ) -> Vec<ChatMessage> {
        let invocation_filter = match mode {
            ContentsMode::CurrentInvocation => Some(invocation_id),
            ContentsMode::FullSession => None,
        };
        let events = session.filter_events(invocation_filter, Some(branch));
        let events = Self::apply_rewind(events);
        let events = Self::filter_context_events(events);
        let events = Self::apply_compaction_ranges(events);
        let events = Self::rearrange_latest_tool_results(events);
        let events = Self::rearrange_tool_results_in_history(events);
        events
            .into_iter()
            .filter_map(|event| match event.payload {
                KernelTranscriptPayload::Message { message } => Some(message),
                KernelTranscriptPayload::Compaction { payload } => Some(payload.summary_message),
                KernelTranscriptPayload::StateOnly
                | KernelTranscriptPayload::Transfer { .. }
                | KernelTranscriptPayload::Auth { .. }
                | KernelTranscriptPayload::UserActionRequest { .. }
                | KernelTranscriptPayload::UserActionResponse { .. } => None,
            })
            .collect()
    }

    fn filter_context_events(events: Vec<KernelTranscriptEvent>) -> Vec<KernelTranscriptEvent> {
        events
            .into_iter()
            .filter(|event| !event.partial)
            .filter(|event| !Self::is_framework_event(event))
            .filter(|event| !Self::is_auth_event(event))
            .filter(|event| !Self::contains_empty_content(event))
            .collect()
    }

    fn apply_rewind(events: Vec<KernelTranscriptEvent>) -> Vec<KernelTranscriptEvent> {
        let mut filtered = Vec::new();
        let mut index = events.len();
        while index > 0 {
            index -= 1;
            let event = &events[index];
            if let Some(rewind_invocation_id) = &event.actions.rewind_before_invocation_id {
                if let Some(target_index) = events
                    .iter()
                    .position(|candidate| &candidate.invocation_id == rewind_invocation_id)
                {
                    index = target_index;
                }
                continue;
            }
            filtered.push(event.clone());
        }
        filtered.reverse();
        filtered
    }

    fn apply_compaction_ranges(events: Vec<KernelTranscriptEvent>) -> Vec<KernelTranscriptEvent> {
        let mut compaction_ranges = Vec::new();

        for event in &events {
            if let Some(compaction) = &event.actions.compaction {
                let Some(start_index) = events
                    .iter()
                    .position(|candidate| candidate.id == compaction.start_event_id)
                else {
                    continue;
                };
                let Some(end_index) = events
                    .iter()
                    .position(|candidate| candidate.id == compaction.end_event_id)
                else {
                    continue;
                };
                compaction_ranges.push((start_index.min(end_index), start_index.max(end_index)));
            }
        }

        if compaction_ranges.is_empty() {
            return events;
        }

        events
            .into_iter()
            .enumerate()
            .filter_map(|(index, event)| {
                if event.actions.compaction.is_some() {
                    return Some(event);
                }
                let is_compacted = compaction_ranges
                    .iter()
                    .any(|(start, end)| *start <= index && index <= *end);
                (!is_compacted).then_some(event)
            })
            .collect()
    }

    fn contains_empty_content(event: &KernelTranscriptEvent) -> bool {
        let Some(message) = event.payload.message() else {
            return true;
        };
        let has_text = !message.content.trim().is_empty();
        let has_tool_calls = message
            .tool_calls
            .as_ref()
            .is_some_and(|tool_calls| !tool_calls.is_empty());
        let has_tool_response = message.tool_call_id.is_some();
        !has_text && !has_tool_calls && !has_tool_response
    }

    fn is_framework_event(event: &KernelTranscriptEvent) -> bool {
        matches!(
            event.payload,
            KernelTranscriptPayload::StateOnly | KernelTranscriptPayload::Transfer { .. }
        )
    }

    fn is_auth_event(event: &KernelTranscriptEvent) -> bool {
        matches!(event.payload, KernelTranscriptPayload::Auth { .. })
    }

    fn rearrange_latest_tool_results(
        events: Vec<KernelTranscriptEvent>,
    ) -> Vec<KernelTranscriptEvent> {
        let Some(last_event) = events.last() else {
            return events;
        };
        let Some(last_tool_call_id) = Self::tool_result_id(last_event) else {
            return events;
        };

        let previous_matches = events
            .iter()
            .rev()
            .nth(1)
            .is_some_and(|event| Self::tool_call_ids(event).contains(&last_tool_call_id));
        if previous_matches {
            return events;
        }

        let Some(call_index) = events[..events.len().saturating_sub(1)]
            .iter()
            .rposition(|event| Self::tool_call_ids(event).contains(&last_tool_call_id))
        else {
            return events;
        };

        let expected_tool_call_ids = Self::tool_call_ids(&events[call_index]);
        if expected_tool_call_ids.is_empty() {
            return events;
        }

        let mut prefix = events[..=call_index].to_vec();
        let mut tool_results = Vec::new();
        for event in &events[call_index + 1..] {
            match Self::tool_result_id(event) {
                Some(tool_call_id) if expected_tool_call_ids.contains(&tool_call_id) => {
                    tool_results.push(event.clone());
                }
                _ => {}
            }
        }
        prefix.extend(tool_results);
        prefix
    }

    fn rearrange_tool_results_in_history(
        events: Vec<KernelTranscriptEvent>,
    ) -> Vec<KernelTranscriptEvent> {
        let all_events = events.clone();
        let mut tool_result_index_by_call_id = std::collections::HashMap::new();
        for (index, event) in all_events.iter().enumerate() {
            if let Some(tool_call_id) = Self::tool_result_id(event) {
                tool_result_index_by_call_id.insert(tool_call_id, index);
            }
        }

        let mut result = Vec::new();
        for event in events {
            if Self::tool_result_id(&event).is_some() {
                continue;
            }

            let tool_call_ids = Self::tool_call_ids(&event);
            if tool_call_ids.is_empty() {
                result.push(event);
                continue;
            }

            result.push(event);
            for tool_call_id in tool_call_ids {
                if let Some(index) = tool_result_index_by_call_id.get(&tool_call_id) {
                    result.push(all_events[*index].clone());
                }
            }
        }

        result
    }

    fn tool_call_ids(event: &KernelTranscriptEvent) -> Vec<String> {
        if event.kind != KernelTranscriptEventKind::ToolCall {
            return Vec::new();
        }
        event
            .payload
            .message()
            .and_then(|message| message.tool_calls.as_ref())
            .map(|tool_calls| {
                tool_calls
                    .iter()
                    .map(|tool_call| tool_call.id.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn tool_result_id(event: &KernelTranscriptEvent) -> Option<String> {
        if event.kind != KernelTranscriptEventKind::ToolResult {
            return None;
        }
        event.payload.message()?.tool_call_id.clone()
    }
}

#[cfg(test)]
#[path = "contents_builder_tests.rs"]
mod tests;
