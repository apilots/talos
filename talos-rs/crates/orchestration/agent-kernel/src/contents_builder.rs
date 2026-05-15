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
                | KernelTranscriptPayload::Confirmation { .. }
                | KernelTranscriptPayload::ApprovalResponse { .. }
                | KernelTranscriptPayload::DynamicToolRequest { .. }
                | KernelTranscriptPayload::DynamicToolResponse { .. } => None,
            })
            .collect()
    }

    fn filter_context_events(events: Vec<KernelTranscriptEvent>) -> Vec<KernelTranscriptEvent> {
        events
            .into_iter()
            .filter(|event| !event.partial)
            .filter(|event| !Self::is_framework_event(event))
            .filter(|event| !Self::is_auth_event(event))
            .filter(|event| !Self::is_confirmation_event(event))
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

    fn is_confirmation_event(event: &KernelTranscriptEvent) -> bool {
        matches!(event.payload, KernelTranscriptPayload::Confirmation { .. })
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
mod tests {
    use super::*;
    use crate::execution::EventCompaction;
    use crate::session::{
        KernelTranscriptAuthPayload, KernelTranscriptCompactionPayload,
        KernelTranscriptConfirmationPayload, KernelTranscriptTransferPayload,
    };
    use crate::{EventAuthor, SessionState};
    use talos_llm_provider::FunctionCall;

    fn kernel_transcript_event(
        kind: KernelTranscriptEventKind,
        message: ChatMessage,
        branch: ExecutionBranch,
    ) -> KernelTranscriptEvent {
        KernelTranscriptEvent {
            id: format!("evt-{}", uuid::Uuid::new_v4()),
            session_id: "s1".to_string(),
            invocation_id: "i1".to_string(),
            branch,
            author: match kind {
                KernelTranscriptEventKind::UserMessage => EventAuthor::User,
                KernelTranscriptEventKind::ToolResult => EventAuthor::Tool,
                KernelTranscriptEventKind::SystemMessage => EventAuthor::System,
                KernelTranscriptEventKind::AgentMessage
                | KernelTranscriptEventKind::ToolCall
                | KernelTranscriptEventKind::Planning
                | KernelTranscriptEventKind::StateUpdate => EventAuthor::Agent,
            },
            kind,
            payload: KernelTranscriptPayload::Message { message },
            actions: crate::EventActions::default(),
            partial: false,
            timestamp: chrono::Utc::now(),
        }
    }

    fn assistant_tool_call(id: &str, name: &str) -> ChatMessage {
        ChatMessage::assistant_with_tool_calls(
            "",
            vec![FunctionCall::new(id, name, serde_json::json!({}))],
        )
    }

    fn tool_result(id: &str, name: &str, content: &str) -> ChatMessage {
        ChatMessage::tool(id, name, content)
    }

    fn state_only_event() -> KernelTranscriptEvent {
        KernelTranscriptEvent {
            id: format!("evt-{}", uuid::Uuid::new_v4()),
            session_id: "s1".to_string(),
            invocation_id: "i1".to_string(),
            branch: ExecutionBranch::ReactiveLoop,
            author: EventAuthor::Agent,
            kind: KernelTranscriptEventKind::Planning,
            payload: KernelTranscriptPayload::StateOnly,
            actions: crate::EventActions::default(),
            partial: false,
            timestamp: chrono::Utc::now(),
        }
    }

    #[test]
    fn test_build_messages_keeps_tool_result_adjacent_to_tool_call() {
        let session = InvocationSession {
            session_id: "s1".to_string(),
            events: vec![
                kernel_transcript_event(
                    KernelTranscriptEventKind::UserMessage,
                    ChatMessage::user("hello"),
                    ExecutionBranch::ReactiveLoop,
                ),
                kernel_transcript_event(
                    KernelTranscriptEventKind::ToolCall,
                    assistant_tool_call("call_1", "read"),
                    ExecutionBranch::ReactiveLoop,
                ),
                kernel_transcript_event(
                    KernelTranscriptEventKind::AgentMessage,
                    ChatMessage::assistant("intermediate"),
                    ExecutionBranch::ReactiveLoop,
                ),
                kernel_transcript_event(
                    KernelTranscriptEventKind::ToolResult,
                    tool_result("call_1", "read", "done"),
                    ExecutionBranch::ReactiveLoop,
                ),
            ],
            state: SessionState::default(),
        };

        let messages = ContentsBuilder::build_messages(
            &session,
            "i1",
            ExecutionBranch::ReactiveLoop,
            ContentsMode::CurrentInvocation,
        );

        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].role, talos_llm_provider::Role::User);
        assert!(messages[1]
            .tool_calls
            .as_ref()
            .is_some_and(|calls| calls.len() == 1));
        assert_eq!(messages[2].tool_call_id.as_deref(), Some("call_1"));
    }

    #[test]
    fn test_build_messages_keeps_parallel_tool_results_grouped_after_tool_call() {
        let session = InvocationSession {
            session_id: "s1".to_string(),
            events: vec![
                kernel_transcript_event(
                    KernelTranscriptEventKind::UserMessage,
                    ChatMessage::user("hello"),
                    ExecutionBranch::ReactiveLoop,
                ),
                kernel_transcript_event(
                    KernelTranscriptEventKind::ToolCall,
                    ChatMessage::assistant_with_tool_calls(
                        "",
                        vec![
                            FunctionCall::new("call_1", "read", serde_json::json!({})),
                            FunctionCall::new("call_2", "grep", serde_json::json!({})),
                        ],
                    ),
                    ExecutionBranch::ReactiveLoop,
                ),
                kernel_transcript_event(
                    KernelTranscriptEventKind::ToolResult,
                    tool_result("call_2", "grep", "b"),
                    ExecutionBranch::ReactiveLoop,
                ),
                kernel_transcript_event(
                    KernelTranscriptEventKind::ToolResult,
                    tool_result("call_1", "read", "a"),
                    ExecutionBranch::ReactiveLoop,
                ),
            ],
            state: SessionState::default(),
        };

        let messages = ContentsBuilder::build_messages(
            &session,
            "i1",
            ExecutionBranch::ReactiveLoop,
            ContentsMode::CurrentInvocation,
        );

        assert_eq!(messages.len(), 4);
        assert!(messages[1]
            .tool_calls
            .as_ref()
            .is_some_and(|calls| calls.len() == 2));
        assert_eq!(messages[2].tool_call_id.as_deref(), Some("call_1"));
        assert_eq!(messages[3].tool_call_id.as_deref(), Some("call_2"));
    }

    #[test]
    fn test_build_messages_filters_branch_and_partial_events() {
        let mut partial_event = kernel_transcript_event(
            KernelTranscriptEventKind::AgentMessage,
            ChatMessage::assistant("partial"),
            ExecutionBranch::ReactiveLoop,
        );
        partial_event.partial = true;

        let session = InvocationSession {
            session_id: "s1".to_string(),
            events: vec![
                kernel_transcript_event(
                    KernelTranscriptEventKind::UserMessage,
                    ChatMessage::user("visible"),
                    ExecutionBranch::ReactiveLoop,
                ),
                partial_event,
                kernel_transcript_event(
                    KernelTranscriptEventKind::UserMessage,
                    ChatMessage::user("hidden"),
                    ExecutionBranch::DirectAnswer,
                ),
            ],
            state: SessionState::default(),
        };

        let messages = ContentsBuilder::build_messages(
            &session,
            "i1",
            ExecutionBranch::ReactiveLoop,
            ContentsMode::CurrentInvocation,
        );

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content, "visible");
    }

    #[test]
    fn test_build_messages_filters_framework_and_control_payloads() {
        let now = chrono::Utc::now();
        let session = InvocationSession {
            session_id: "s1".to_string(),
            events: vec![
                kernel_transcript_event(
                    KernelTranscriptEventKind::UserMessage,
                    ChatMessage::user("hello"),
                    ExecutionBranch::ReactiveLoop,
                ),
                KernelTranscriptEvent {
                    id: "transfer".to_string(),
                    session_id: "s1".to_string(),
                    invocation_id: "i1".to_string(),
                    branch: ExecutionBranch::ReactiveLoop,
                    author: EventAuthor::Agent,
                    kind: KernelTranscriptEventKind::StateUpdate,
                    payload: KernelTranscriptPayload::Transfer {
                        payload: KernelTranscriptTransferPayload {
                            target_agent: "agent-b".to_string(),
                        },
                    },
                    actions: crate::EventActions::default(),
                    partial: false,
                    timestamp: now,
                },
                KernelTranscriptEvent {
                    id: "auth".to_string(),
                    session_id: "s1".to_string(),
                    invocation_id: "i1".to_string(),
                    branch: ExecutionBranch::ReactiveLoop,
                    author: EventAuthor::System,
                    kind: KernelTranscriptEventKind::SystemMessage,
                    payload: KernelTranscriptPayload::Auth {
                        payload: KernelTranscriptAuthPayload {
                            provider: "github".to_string(),
                            prompt: "auth required".to_string(),
                        },
                    },
                    actions: crate::EventActions::default(),
                    partial: false,
                    timestamp: now,
                },
                KernelTranscriptEvent {
                    id: "confirm".to_string(),
                    session_id: "s1".to_string(),
                    invocation_id: "i1".to_string(),
                    branch: ExecutionBranch::ReactiveLoop,
                    author: EventAuthor::System,
                    kind: KernelTranscriptEventKind::SystemMessage,
                    payload: KernelTranscriptPayload::Confirmation {
                        payload: KernelTranscriptConfirmationPayload {
                            tool_name: "rm".to_string(),
                            call_id: "call_1".to_string(),
                            prompt: "confirm".to_string(),
                            arguments: serde_json::json!({}).into(),
                        },
                    },
                    actions: crate::EventActions::default(),
                    partial: false,
                    timestamp: now,
                },
                state_only_event(),
            ],
            state: SessionState::default(),
        };

        let messages = ContentsBuilder::build_messages(
            &session,
            "i1",
            ExecutionBranch::ReactiveLoop,
            ContentsMode::CurrentInvocation,
        );

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content, "hello");
    }

    #[test]
    fn test_build_messages_applies_rewind_before_invocation() {
        let now = chrono::Utc::now();
        let mut rewind_event = state_only_event();
        rewind_event.id = "rewind".to_string();
        rewind_event.invocation_id = "i2".to_string();
        rewind_event.actions.rewind_before_invocation_id = Some("i1".to_string());
        rewind_event.timestamp = now;

        let session = InvocationSession {
            session_id: "s1".to_string(),
            events: vec![
                kernel_transcript_event(
                    KernelTranscriptEventKind::UserMessage,
                    ChatMessage::user("old"),
                    ExecutionBranch::ReactiveLoop,
                ),
                KernelTranscriptEvent {
                    id: "new-user".to_string(),
                    session_id: "s1".to_string(),
                    invocation_id: "i2".to_string(),
                    branch: ExecutionBranch::ReactiveLoop,
                    author: EventAuthor::User,
                    kind: KernelTranscriptEventKind::UserMessage,
                    payload: KernelTranscriptPayload::Message {
                        message: ChatMessage::user("new"),
                    },
                    actions: crate::EventActions::default(),
                    partial: false,
                    timestamp: now,
                },
                rewind_event,
                KernelTranscriptEvent {
                    id: "post-rewind".to_string(),
                    session_id: "s1".to_string(),
                    invocation_id: "i3".to_string(),
                    branch: ExecutionBranch::ReactiveLoop,
                    author: EventAuthor::User,
                    kind: KernelTranscriptEventKind::UserMessage,
                    payload: KernelTranscriptPayload::Message {
                        message: ChatMessage::user("after rewind"),
                    },
                    actions: crate::EventActions::default(),
                    partial: false,
                    timestamp: now + chrono::Duration::milliseconds(5),
                },
            ],
            state: SessionState::default(),
        };

        let messages = ContentsBuilder::build_messages(
            &session,
            "i2",
            ExecutionBranch::ReactiveLoop,
            ContentsMode::FullSession,
        );

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content, "after rewind");
    }

    #[test]
    fn test_build_messages_applies_compaction_summary_and_skips_raw_range() {
        let start = chrono::Utc::now();
        let middle = start + chrono::Duration::milliseconds(10);
        let end = start + chrono::Duration::milliseconds(20);

        let mut compaction_event = KernelTranscriptEvent {
            id: "compact".to_string(),
            session_id: "s1".to_string(),
            invocation_id: "i2".to_string(),
            branch: ExecutionBranch::ReactiveLoop,
            author: EventAuthor::Agent,
            kind: KernelTranscriptEventKind::StateUpdate,
            payload: KernelTranscriptPayload::Compaction {
                payload: KernelTranscriptCompactionPayload {
                    summary_message: ChatMessage::assistant("summary"),
                },
            },
            actions: crate::EventActions::default(),
            partial: false,
            timestamp: end,
        };
        compaction_event.actions.compaction = Some(EventCompaction {
            start_event_id: "u1".to_string(),
            end_event_id: "a1".to_string(),
        });

        let session = InvocationSession {
            session_id: "s1".to_string(),
            events: vec![
                KernelTranscriptEvent {
                    id: "u1".to_string(),
                    session_id: "s1".to_string(),
                    invocation_id: "i1".to_string(),
                    branch: ExecutionBranch::ReactiveLoop,
                    author: EventAuthor::User,
                    kind: KernelTranscriptEventKind::UserMessage,
                    payload: KernelTranscriptPayload::Message {
                        message: ChatMessage::user("hello"),
                    },
                    actions: crate::EventActions::default(),
                    partial: false,
                    timestamp: start,
                },
                KernelTranscriptEvent {
                    id: "a1".to_string(),
                    session_id: "s1".to_string(),
                    invocation_id: "i1".to_string(),
                    branch: ExecutionBranch::ReactiveLoop,
                    author: EventAuthor::Agent,
                    kind: KernelTranscriptEventKind::AgentMessage,
                    payload: KernelTranscriptPayload::Message {
                        message: ChatMessage::assistant("detail"),
                    },
                    actions: crate::EventActions::default(),
                    partial: false,
                    timestamp: middle,
                },
                compaction_event,
                KernelTranscriptEvent {
                    id: "u2".to_string(),
                    session_id: "s1".to_string(),
                    invocation_id: "i2".to_string(),
                    branch: ExecutionBranch::ReactiveLoop,
                    author: EventAuthor::User,
                    kind: KernelTranscriptEventKind::UserMessage,
                    payload: KernelTranscriptPayload::Message {
                        message: ChatMessage::user("fresh"),
                    },
                    actions: crate::EventActions::default(),
                    partial: false,
                    timestamp: end + chrono::Duration::milliseconds(10),
                },
            ],
            state: SessionState::default(),
        };

        let messages = ContentsBuilder::build_messages(
            &session,
            "i2",
            ExecutionBranch::ReactiveLoop,
            ContentsMode::FullSession,
        );

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].content, "summary");
        assert_eq!(messages[1].content, "fresh");
    }

    #[test]
    fn test_build_messages_compaction_uses_event_id_boundaries_not_timestamp_ranges() {
        let timestamp = chrono::Utc::now();

        let mut compaction_event = KernelTranscriptEvent {
            id: "compact".to_string(),
            session_id: "s1".to_string(),
            invocation_id: "i2".to_string(),
            branch: ExecutionBranch::ReactiveLoop,
            author: EventAuthor::Agent,
            kind: KernelTranscriptEventKind::StateUpdate,
            payload: KernelTranscriptPayload::Compaction {
                payload: KernelTranscriptCompactionPayload {
                    summary_message: ChatMessage::assistant("summary"),
                },
            },
            actions: crate::EventActions::default(),
            partial: false,
            timestamp,
        };
        compaction_event.actions.compaction = Some(EventCompaction {
            start_event_id: "u1".to_string(),
            end_event_id: "a1".to_string(),
        });

        let session = InvocationSession {
            session_id: "s1".to_string(),
            events: vec![
                KernelTranscriptEvent {
                    id: "u1".to_string(),
                    session_id: "s1".to_string(),
                    invocation_id: "i1".to_string(),
                    branch: ExecutionBranch::ReactiveLoop,
                    author: EventAuthor::User,
                    kind: KernelTranscriptEventKind::UserMessage,
                    payload: KernelTranscriptPayload::Message {
                        message: ChatMessage::user("hello"),
                    },
                    actions: crate::EventActions::default(),
                    partial: false,
                    timestamp,
                },
                KernelTranscriptEvent {
                    id: "a1".to_string(),
                    session_id: "s1".to_string(),
                    invocation_id: "i1".to_string(),
                    branch: ExecutionBranch::ReactiveLoop,
                    author: EventAuthor::Agent,
                    kind: KernelTranscriptEventKind::AgentMessage,
                    payload: KernelTranscriptPayload::Message {
                        message: ChatMessage::assistant("detail"),
                    },
                    actions: crate::EventActions::default(),
                    partial: false,
                    timestamp,
                },
                compaction_event,
                KernelTranscriptEvent {
                    id: "u2".to_string(),
                    session_id: "s1".to_string(),
                    invocation_id: "i2".to_string(),
                    branch: ExecutionBranch::ReactiveLoop,
                    author: EventAuthor::User,
                    kind: KernelTranscriptEventKind::UserMessage,
                    payload: KernelTranscriptPayload::Message {
                        message: ChatMessage::user("fresh"),
                    },
                    actions: crate::EventActions::default(),
                    partial: false,
                    timestamp,
                },
            ],
            state: SessionState::default(),
        };

        let messages = ContentsBuilder::build_messages(
            &session,
            "i2",
            ExecutionBranch::ReactiveLoop,
            ContentsMode::FullSession,
        );

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].content, "summary");
        assert_eq!(messages[1].content, "fresh");
    }
}
