use super::*;

pub struct CompactionRequestProcessor;

#[derive(Debug, Clone, PartialEq, Eq)]
struct InvocationCompactionGroup {
    invocation_id: String,
    end_index: usize,
    total_bytes: usize,
}

fn is_context_visible_event(event: &KernelTranscriptEvent) -> bool {
    !event.partial
        && !matches!(
            event.payload,
            KernelTranscriptPayload::StateOnly
                | KernelTranscriptPayload::Transfer { .. }
                | KernelTranscriptPayload::Auth { .. }
                | KernelTranscriptPayload::Confirmation { .. }
        )
        && event.payload.message().is_some_and(|message| {
            !message.content.trim().is_empty() || message.tool_calls.is_some()
        })
}

fn message_size(message: &ChatMessage) -> usize {
    let tool_call_bytes = message
        .tool_calls
        .as_ref()
        .map(|calls| calls.len() * 96)
        .unwrap_or_default();
    message.content.len() + tool_call_bytes + usize::from(message.tool_call_id.is_some()) * 32
}

fn group_events_by_invocation(events: &[KernelTranscriptEvent]) -> Vec<InvocationCompactionGroup> {
    let mut groups: Vec<InvocationCompactionGroup> = Vec::new();
    for (index, event) in events.iter().enumerate() {
        let event_bytes = event
            .payload
            .message()
            .map(message_size)
            .unwrap_or_default();
        match groups.last_mut() {
            Some(group) if group.invocation_id == event.invocation_id => {
                group.end_index = index;
                group.total_bytes += event_bytes;
            }
            _ => groups.push(InvocationCompactionGroup {
                invocation_id: event.invocation_id.clone(),
                end_index: index,
                total_bytes: event_bytes,
            }),
        }
    }
    groups
}

fn truncate_for_summary(text: &str, limit: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= limit {
        return trimmed.to_string();
    }
    let compact = trimmed.chars().take(limit).collect::<String>();
    format!("{}...", compact)
}

fn summarize_compacted_events(events: &[KernelTranscriptEvent]) -> Option<String> {
    if events.is_empty() {
        return None;
    }
    let mut lines = Vec::new();
    for event in events.iter().take(6) {
        let Some(message) = event.payload.message() else {
            continue;
        };
        let speaker = match message.role {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        };
        let body = if let Some(tool_calls) = &message.tool_calls {
            let tools = tool_calls
                .iter()
                .map(|call| call.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            format!("tool calls: {}", tools)
        } else {
            truncate_for_summary(&message.content, 120)
        };
        lines.push(format!("- {}: {}", speaker, body));
    }
    if lines.is_empty() {
        None
    } else {
        Some(format!("Compacted previous context:\n{}", lines.join("\n")))
    }
}

#[async_trait]
impl RequestProcessor for CompactionRequestProcessor {
    fn kind(&self) -> RequestProcessorKind {
        RequestProcessorKind::Compaction
    }

    async fn run(
        &self,
        ctx: &mut InvocationContext,
        request: &mut LlmRequestEnvelope,
    ) -> TalosResult<RequestProcessorOutput> {
        let threshold = ctx.limits.context_compaction_max_bytes;
        if threshold == 0 {
            return Ok(RequestProcessorOutput::default());
        }

        let visible_events = ctx
            .session
            .filter_events(None, Some(request.branch))
            .into_iter()
            .filter(|event| event.invocation_id != ctx.invocation_id)
            .filter(is_context_visible_event)
            .collect::<Vec<_>>();
        let invocation_groups = group_events_by_invocation(&visible_events);
        if invocation_groups.len() <= 1 {
            return Ok(RequestProcessorOutput::default());
        }

        let total_bytes = visible_events
            .iter()
            .filter_map(|event| event.payload.message())
            .map(message_size)
            .sum::<usize>();
        if total_bytes <= threshold {
            return Ok(RequestProcessorOutput::default());
        }

        let target_tail_bytes = threshold / 2;
        let mut keep_group_start = invocation_groups.len();
        let mut tail_bytes = 0usize;
        for (index, group) in invocation_groups.iter().enumerate().rev() {
            tail_bytes += group.total_bytes;
            let retained_groups = invocation_groups.len() - index;
            if retained_groups >= 1 && tail_bytes >= target_tail_bytes {
                keep_group_start = index;
                break;
            }
        }

        if keep_group_start == 0 || keep_group_start >= invocation_groups.len() {
            return Ok(RequestProcessorOutput::default());
        }

        let compacted_end_index = invocation_groups[keep_group_start - 1].end_index;
        let compacted_slice = &visible_events[..=compacted_end_index];
        let Some(summary) = summarize_compacted_events(compacted_slice) else {
            return Ok(RequestProcessorOutput::default());
        };
        let mut compaction_event = ExecutionEvent::new(
            ctx,
            EventAuthor::System,
            request.branch,
            ExecutionEventKind::System,
            EventPayload::CompactionSummary {
                payload: CompactionSummaryPayload { summary },
            },
            false,
        );
        compaction_event.actions.compaction = Some(crate::execution::EventCompaction {
            start_event_id: compacted_slice
                .first()
                .expect("compaction slice should not be empty")
                .id
                .clone(),
            end_event_id: compacted_slice
                .last()
                .expect("compaction slice should not be empty")
                .id
                .clone(),
        });

        Ok(RequestProcessorOutput {
            events: vec![compaction_event],
        })
    }
}
