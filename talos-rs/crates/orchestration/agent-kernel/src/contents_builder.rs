use crate::{ExecutionBranch, InvocationSession, SessionEvent, SessionEventKind};
use talos_llm_provider::ChatMessage;

pub struct ContentsBuilder;

impl ContentsBuilder {
    pub fn build_messages(
        session: &InvocationSession,
        invocation_id: &str,
        branch: ExecutionBranch,
    ) -> Vec<ChatMessage> {
        let events = session.filter_events(Some(invocation_id), Some(branch));
        let events = Self::filter_context_events(events);
        let events = Self::rearrange_latest_tool_results(events);
        let events = Self::rearrange_tool_results_in_history(events);
        events.into_iter().filter_map(|event| event.message).collect()
    }

    fn filter_context_events(events: Vec<SessionEvent>) -> Vec<SessionEvent> {
        events
            .into_iter()
            .filter(|event| !event.partial)
            .filter(|event| event.message.is_some())
            .collect()
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
            .message
            .as_ref()
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
        event.message.as_ref()?.tool_call_id.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
                SessionEventKind::AgentMessage | SessionEventKind::ToolCall => EventAuthor::Agent,
            },
            kind,
            message: Some(message),
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

        let messages =
            ContentsBuilder::build_messages(&session, "i1", ExecutionBranch::ReactiveLoop);

        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].role, talos_llm_provider::Role::User);
        assert!(messages[1].tool_calls.as_ref().is_some_and(|calls| calls.len() == 1));
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

        let messages =
            ContentsBuilder::build_messages(&session, "i1", ExecutionBranch::ReactiveLoop);

        assert_eq!(messages.len(), 4);
        assert!(messages[1].tool_calls.as_ref().is_some_and(|calls| calls.len() == 2));
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

        let messages =
            ContentsBuilder::build_messages(&session, "i1", ExecutionBranch::ReactiveLoop);

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content, "visible");
    }
}
