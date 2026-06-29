use super::*;
use crate::execution::EventCompaction;
use crate::session::{
    KernelTranscriptAuthPayload, KernelTranscriptCompactionPayload, KernelTranscriptTransferPayload,
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
