use super::*;
use talos_memory::{UserWorkspaceMemoryEntry, UserWorkspaceMemorySection};
use talos_types::MemoryKind;

#[test]
fn reflection_runs_only_for_successful_end_turn_without_a_successful_memory_tool() {
    let completed = GeneralTurnOutcome::Completed {
        output_text: "done".to_string(),
        output_items: Vec::new(),
        stop_reason: StopReason::EndTurn,
        usage: Default::default(),
        metrics: Default::default(),
    };
    let output_limited = GeneralTurnOutcome::Completed {
        output_text: "partial".to_string(),
        output_items: Vec::new(),
        stop_reason: StopReason::ModelOutputLimit,
        usage: Default::default(),
        metrics: Default::default(),
    };
    let successful_tool = RunItem::ToolResult {
        call_id: "call-1".to_string(),
        tool_name: MEMORY_TOOL_PUBLIC_NAME.to_string(),
        content: Vec::new(),
        is_error: false,
    };
    let failed_tool = RunItem::ToolResult {
        call_id: "call-2".to_string(),
        tool_name: MEMORY_TOOL_PUBLIC_NAME.to_string(),
        content: Vec::new(),
        is_error: true,
    };

    assert!(reflection_is_eligible(&completed));
    assert!(!reflection_is_eligible(&output_limited));
    assert!(has_successful_memory_tool_result(&[successful_tool]));
    assert!(!has_successful_memory_tool_result(&[failed_tool]));
}

#[test]
fn reflection_parser_accepts_only_noop_upsert_and_remove() {
    assert!(parse_reflection_update(r#"{"operation":"noop"}"#).is_none());
    assert!(matches!(
        parse_reflection_update(
            r#"{"operation":"remove_entry","key":"stale-context"}"#
        ),
        Some(UserWorkspaceMemoryUpdate::RemoveEntry { key }) if key == "stale-context"
    ));
    assert!(parse_reflection_update(
        r#"{"operation":"create_entry","entry":{"key":"x","section":"key_knowledge","title":"X","kind":"fact","summary":"X"}}"#
    )
    .is_none());
    assert!(
        parse_reflection_update(r#"{"operation":"remove_entry","key":"x","unexpected":true}"#)
            .is_none()
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
    let document = talos_memory::UserWorkspaceMemoryDocument::template()
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
    let mut duplicate = snapshot
        .document()
        .and_then(|document| document.get_entry("stable-preference"))
        .expect("entry should exist");
    duplicate.key = "different-model-key".to_string();
    assert!(update_is_noop(
        &snapshot,
        &UserWorkspaceMemoryUpdate::UpsertEntry { entry: duplicate }
    ));

    let desired = UserWorkspaceMemoryEntry::builder()
        .key("stable-preference")
        .section(UserWorkspaceMemorySection::InteractionGuidance)
        .title("Updated preference")
        .kind(MemoryKind::Preference)
        .summary("Prefer direct answers.")
        .build()
        .unwrap();
    let other = UserWorkspaceMemoryEntry {
        key: "other-preference".to_string(),
        ..desired.clone()
    };
    let document = snapshot
        .document()
        .expect("document should exist")
        .clone()
        .upsert_entry(other)
        .unwrap();
    let target_update_snapshot = UserWorkspaceMemorySnapshot::Valid {
        raw_content: document.render().unwrap(),
        updated_at: None,
        version_token: "v2".to_string(),
        document,
    };
    assert!(!update_is_noop(
        &target_update_snapshot,
        &UserWorkspaceMemoryUpdate::UpsertEntry { entry: desired }
    ));

    assert!(update_is_noop(
        &snapshot,
        &UserWorkspaceMemoryUpdate::RemoveEntry {
            key: "unknown".to_string()
        }
    ));
}
