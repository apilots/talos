use super::*;
use talos_agent_api::{
    AgentEventPayload, AgentMessageBlock, AgentMessageBlockStatus, AgentMessageRole,
    AgentMessageStatus, AgentToolResult, AgentTurnCompleted, AgentTurnRef,
};
use talos_memory::{
    MemoryAgentId, UserWorkspaceMemoryEntry, UserWorkspaceMemoryProducer,
    UserWorkspaceMemorySection,
};
use talos_types::MemoryKind;

#[test]
fn reflection_runs_only_for_successful_end_turn_without_a_successful_memory_tool() {
    let event = AgentEvent {
        identity: AgentTurnRef::new("session-1", "turn-1"),
        sequence: 4,
        occurred_at: 1,
        payload: AgentEventPayload::TurnCompleted(AgentTurnCompleted {
            stop_reason: AgentStopReason::EndTurn,
        }),
    };
    let successful_tool = message(AgentMessageBlockContent::ToolResult(AgentToolResult {
        call_id: "call-1".to_string(),
        tool_name: MEMORY_TOOL_PUBLIC_NAME.to_string(),
        content: Vec::new(),
        is_error: false,
    }));
    let failed_tool = message(AgentMessageBlockContent::ToolResult(AgentToolResult {
        call_id: "call-2".to_string(),
        tool_name: MEMORY_TOOL_PUBLIC_NAME.to_string(),
        content: Vec::new(),
        is_error: true,
    }));

    assert!(reflection_is_eligible(&event));
    assert!(has_successful_memory_tool_result(&[successful_tool]));
    assert!(!has_successful_memory_tool_result(&[failed_tool]));
}

#[test]
fn reflection_parser_accepts_only_noop_upsert_and_remove() {
    assert!(parse_reflection_update(r#"{"operation":"noop"}"#)
        .unwrap()
        .is_none());
    assert!(matches!(
        parse_reflection_update(r#"{"operation":"remove_entry","key":"stale-context"}"#),
        Ok(Some(UserWorkspaceMemoryUpdate::RemoveEntry { key })) if key == "stale-context"
    ));
    assert!(parse_reflection_update(
        r#"{"operation":"create_entry","entry":{"key":"x","section":"key_knowledge","title":"X","kind":"fact","summary":"X"}}"#
    )
    .is_err());
    assert!(
        parse_reflection_update(r#"{"operation":"remove_entry","key":"x","unexpected":true}"#)
            .is_err()
    );
}

#[test]
fn reflection_skips_semantically_unchanged_updates() {
    let entry = UserWorkspaceMemoryEntry::builder()
        .key("stable-preference")
        .section(UserWorkspaceMemorySection::InteractionGuidance)
        .title("Stable preference")
        .kind(MemoryKind::Preference)
        .summary("Keep responses concise.")
        .build()
        .unwrap();
    let document = talos_memory::UserWorkspaceMemoryDocument::template(
        UserWorkspaceMemoryProducer::new(MemoryAgentId::parse("agent-1").unwrap(), 1_784_251_800),
    )
    .unwrap()
    .upsert_entry(entry.clone())
    .unwrap();
    let snapshot = UserWorkspaceMemorySnapshot::Valid {
        raw_content: document.render().unwrap(),
        updated_at: None,
        version_token: "v1".to_string(),
        document,
    };

    assert!(update_is_noop(
        &snapshot,
        &UserWorkspaceMemoryUpdate::UpsertEntry { entry }
    ));
    assert!(update_is_noop(
        &snapshot,
        &UserWorkspaceMemoryUpdate::RemoveEntry {
            key: "unknown".to_string()
        }
    ));
}

fn message(content: AgentMessageBlockContent) -> AgentMessage {
    AgentMessage {
        message_id: "message-1".to_string(),
        turn_id: "turn-1".to_string(),
        role: AgentMessageRole::Tool,
        status: AgentMessageStatus::Completed,
        blocks: vec![AgentMessageBlock {
            block_id: "block-1".to_string(),
            status: AgentMessageBlockStatus::Completed,
            content,
            started_at: 1,
            completed_at: Some(1),
        }],
        created_at: 1,
        completed_at: Some(1),
    }
}
