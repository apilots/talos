use crate::execution::ContentsMode;
use crate::{
    ExecutionBranch, InvocationSession, SessionEvent, SessionEventKind, SessionEventPayload,
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
                SessionEventPayload::Message { message } => Some(message),
                SessionEventPayload::Compaction { payload } => Some(payload.summary_message),
                SessionEventPayload::StateOnly
                | SessionEventPayload::Transfer { .. }
                | SessionEventPayload::Auth { .. }
                | SessionEventPayload::Confirmation { .. }
                | SessionEventPayload::ApprovalResponse { .. }
                | SessionEventPayload::DynamicToolRequest { .. }
                | SessionEventPayload::DynamicToolResponse { .. } => None,
            })
            .collect()
    }

    fn filter_context_events(events: Vec<SessionEvent>) -> Vec<SessionEvent> {
        events
            .into_iter()
            .filter(|event| !event.partial)
            .filter(|event| !Self::is_framework_event(event))
            .filter(|event| !Self::is_auth_event(event))
            .filter(|event| !Self::is_confirmation_event(event))
            .filter(|event| !Self::contains_empty_content(event))
            .collect()
    }

    fn apply_rewind(events: Vec<SessionEvent>) -> Vec<SessionEvent> {
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

    fn apply_compaction_ranges(events: Vec<SessionEvent>) -> Vec<SessionEvent> {
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

    fn contains_empty_content(event: &SessionEvent) -> bool {
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

    fn is_framework_event(event: &SessionEvent) -> bool {
        matches!(
            event.payload,
            SessionEventPayload::StateOnly | SessionEventPayload::Transfer { .. }
        )
    }

    fn is_auth_event(event: &SessionEvent) -> bool {
        matches!(event.payload, SessionEventPayload::Auth { .. })
    }

    fn is_confirmation_event(event: &SessionEvent) -> bool {
        matches!(event.payload, SessionEventPayload::Confirmation { .. })
    }

    fn rearrange_latest_tool_results(events: Vec<SessionEvent>) -> Vec<SessionEvent> {
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

    fn rearrange_tool_results_in_history(events: Vec<SessionEvent>) -> Vec<SessionEvent> {
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

    fn tool_call_ids(event: &SessionEvent) -> Vec<String> {
        if event.kind != SessionEventKind::ToolCall {
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

    fn tool_result_id(event: &SessionEvent) -> Option<String> {
        if event.kind != SessionEventKind::ToolResult {
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
        SessionAuthPayload, SessionCompactionPayload, SessionConfirmationPayload,
        SessionTransferPayload,
    };
    use crate::{EventAuthor, SessionState};
    use talos_llm_provider::FunctionCall;

    fn session_event(
        kind: SessionEventKind,
        message: ChatMessage,
        branch: ExecutionBranch,
    ) -> SessionEvent {
        SessionEvent {
            id: format!("evt-{}", uuid::Uuid::new_v4()),
            session_id: "s1".to_string(),
            invocation_id: "i1".to_string(),
            branch,
            author: match kind {
                SessionEventKind::UserMessage => EventAuthor::User,
                SessionEventKind::ToolResult => EventAuthor::Tool,
                SessionEventKind::SystemMessage => EventAuthor::System,
                SessionEventKind::AgentMessage
                | SessionEventKind::ToolCall
                | SessionEventKind::Planning
                | SessionEventKind::StateUpdate => EventAuthor::Agent,
            },
            kind,
            payload: SessionEventPayload::Message { message },
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

    fn state_only_event() -> SessionEvent {
        SessionEvent {
            id: format!("evt-{}", uuid::Uuid::new_v4()),
            session_id: "s1".to_string(),
            invocation_id: "i1".to_string(),
            branch: ExecutionBranch::ReactiveLoop,
            author: EventAuthor::Agent,
            kind: SessionEventKind::Planning,
            payload: SessionEventPayload::StateOnly,
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
                session_event(
                    SessionEventKind::UserMessage,
                    ChatMessage::user("hello"),
                    ExecutionBranch::ReactiveLoop,
                ),
                session_event(
                    SessionEventKind::ToolCall,
                    assistant_tool_call("call_1", "read"),
                    ExecutionBranch::ReactiveLoop,
                ),
                session_event(
                    SessionEventKind::AgentMessage,
                    ChatMessage::assistant("intermediate"),
                    ExecutionBranch::ReactiveLoop,
                ),
                session_event(
                    SessionEventKind::ToolResult,
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
                session_event(
                    SessionEventKind::UserMessage,
                    ChatMessage::user("hello"),
                    ExecutionBranch::ReactiveLoop,
                ),
                session_event(
                    SessionEventKind::ToolCall,
                    ChatMessage::assistant_with_tool_calls(
                        "",
                        vec![
                            FunctionCall::new("call_1", "read", serde_json::json!({})),
                            FunctionCall::new("call_2", "grep", serde_json::json!({})),
                        ],
                    ),
                    ExecutionBranch::ReactiveLoop,
                ),
                session_event(
                    SessionEventKind::ToolResult,
                    tool_result("call_2", "grep", "b"),
                    ExecutionBranch::ReactiveLoop,
                ),
                session_event(
                    SessionEventKind::ToolResult,
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
        let mut partial_event = session_event(
            SessionEventKind::AgentMessage,
            ChatMessage::assistant("partial"),
            ExecutionBranch::ReactiveLoop,
        );
        partial_event.partial = true;

        let session = InvocationSession {
            session_id: "s1".to_string(),
            events: vec![
                session_event(
                    SessionEventKind::UserMessage,
                    ChatMessage::user("visible"),
                    ExecutionBranch::ReactiveLoop,
                ),
                partial_event,
                session_event(
                    SessionEventKind::UserMessage,
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
                session_event(
                    SessionEventKind::UserMessage,
                    ChatMessage::user("hello"),
                    ExecutionBranch::ReactiveLoop,
                ),
                SessionEvent {
                    id: "transfer".to_string(),
                    session_id: "s1".to_string(),
                    invocation_id: "i1".to_string(),
                    branch: ExecutionBranch::ReactiveLoop,
                    author: EventAuthor::Agent,
                    kind: SessionEventKind::StateUpdate,
                    payload: SessionEventPayload::Transfer {
                        payload: SessionTransferPayload {
                            target_agent: "agent-b".to_string(),
                        },
                    },
                    actions: crate::EventActions::default(),
                    partial: false,
                    timestamp: now,
                },
                SessionEvent {
                    id: "auth".to_string(),
                    session_id: "s1".to_string(),
                    invocation_id: "i1".to_string(),
                    branch: ExecutionBranch::ReactiveLoop,
                    author: EventAuthor::System,
                    kind: SessionEventKind::SystemMessage,
                    payload: SessionEventPayload::Auth {
                        payload: SessionAuthPayload {
                            provider: "github".to_string(),
                            prompt: "auth required".to_string(),
                        },
                    },
                    actions: crate::EventActions::default(),
                    partial: false,
                    timestamp: now,
                },
                SessionEvent {
                    id: "confirm".to_string(),
                    session_id: "s1".to_string(),
                    invocation_id: "i1".to_string(),
                    branch: ExecutionBranch::ReactiveLoop,
                    author: EventAuthor::System,
                    kind: SessionEventKind::SystemMessage,
                    payload: SessionEventPayload::Confirmation {
                        payload: SessionConfirmationPayload {
                            tool_name: "rm".to_string(),
                            call_id: "call_1".to_string(),
                            prompt: "confirm".to_string(),
                            arguments: serde_json::json!({}),
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
                session_event(
                    SessionEventKind::UserMessage,
                    ChatMessage::user("old"),
                    ExecutionBranch::ReactiveLoop,
                ),
                SessionEvent {
                    id: "new-user".to_string(),
                    session_id: "s1".to_string(),
                    invocation_id: "i2".to_string(),
                    branch: ExecutionBranch::ReactiveLoop,
                    author: EventAuthor::User,
                    kind: SessionEventKind::UserMessage,
                    payload: SessionEventPayload::Message {
                        message: ChatMessage::user("new"),
                    },
                    actions: crate::EventActions::default(),
                    partial: false,
                    timestamp: now,
                },
                rewind_event,
                SessionEvent {
                    id: "post-rewind".to_string(),
                    session_id: "s1".to_string(),
                    invocation_id: "i3".to_string(),
                    branch: ExecutionBranch::ReactiveLoop,
                    author: EventAuthor::User,
                    kind: SessionEventKind::UserMessage,
                    payload: SessionEventPayload::Message {
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

        let mut compaction_event = SessionEvent {
            id: "compact".to_string(),
            session_id: "s1".to_string(),
            invocation_id: "i2".to_string(),
            branch: ExecutionBranch::ReactiveLoop,
            author: EventAuthor::Agent,
            kind: SessionEventKind::StateUpdate,
            payload: SessionEventPayload::Compaction {
                payload: SessionCompactionPayload {
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
                SessionEvent {
                    id: "u1".to_string(),
                    session_id: "s1".to_string(),
                    invocation_id: "i1".to_string(),
                    branch: ExecutionBranch::ReactiveLoop,
                    author: EventAuthor::User,
                    kind: SessionEventKind::UserMessage,
                    payload: SessionEventPayload::Message {
                        message: ChatMessage::user("hello"),
                    },
                    actions: crate::EventActions::default(),
                    partial: false,
                    timestamp: start,
                },
                SessionEvent {
                    id: "a1".to_string(),
                    session_id: "s1".to_string(),
                    invocation_id: "i1".to_string(),
                    branch: ExecutionBranch::ReactiveLoop,
                    author: EventAuthor::Agent,
                    kind: SessionEventKind::AgentMessage,
                    payload: SessionEventPayload::Message {
                        message: ChatMessage::assistant("detail"),
                    },
                    actions: crate::EventActions::default(),
                    partial: false,
                    timestamp: middle,
                },
                compaction_event,
                SessionEvent {
                    id: "u2".to_string(),
                    session_id: "s1".to_string(),
                    invocation_id: "i2".to_string(),
                    branch: ExecutionBranch::ReactiveLoop,
                    author: EventAuthor::User,
                    kind: SessionEventKind::UserMessage,
                    payload: SessionEventPayload::Message {
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

        let mut compaction_event = SessionEvent {
            id: "compact".to_string(),
            session_id: "s1".to_string(),
            invocation_id: "i2".to_string(),
            branch: ExecutionBranch::ReactiveLoop,
            author: EventAuthor::Agent,
            kind: SessionEventKind::StateUpdate,
            payload: SessionEventPayload::Compaction {
                payload: SessionCompactionPayload {
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
                SessionEvent {
                    id: "u1".to_string(),
                    session_id: "s1".to_string(),
                    invocation_id: "i1".to_string(),
                    branch: ExecutionBranch::ReactiveLoop,
                    author: EventAuthor::User,
                    kind: SessionEventKind::UserMessage,
                    payload: SessionEventPayload::Message {
                        message: ChatMessage::user("hello"),
                    },
                    actions: crate::EventActions::default(),
                    partial: false,
                    timestamp,
                },
                SessionEvent {
                    id: "a1".to_string(),
                    session_id: "s1".to_string(),
                    invocation_id: "i1".to_string(),
                    branch: ExecutionBranch::ReactiveLoop,
                    author: EventAuthor::Agent,
                    kind: SessionEventKind::AgentMessage,
                    payload: SessionEventPayload::Message {
                        message: ChatMessage::assistant("detail"),
                    },
                    actions: crate::EventActions::default(),
                    partial: false,
                    timestamp,
                },
                compaction_event,
                SessionEvent {
                    id: "u2".to_string(),
                    session_id: "s1".to_string(),
                    invocation_id: "i2".to_string(),
                    branch: ExecutionBranch::ReactiveLoop,
                    author: EventAuthor::User,
                    kind: SessionEventKind::UserMessage,
                    payload: SessionEventPayload::Message {
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
