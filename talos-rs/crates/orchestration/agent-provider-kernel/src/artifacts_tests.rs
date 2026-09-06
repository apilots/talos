use chrono::{TimeZone, Utc};

use super::*;
use talos_agent_kernel::{
    EventAuthor, ExecutionBranch, KernelArtifactHeader, KernelSessionArtifactStore,
    KernelSessionSummary, KernelTranscriptAuthPayload, KernelTranscriptEvent,
    KernelTranscriptPayload, SessionCompactionState, SessionStateUpdate,
};

fn transcript(timestamp: chrono::DateTime<Utc>) -> KernelTranscriptEvent {
    let mut event = KernelTranscriptEvent::new(
        "session-1",
        "turn-1",
        ExecutionBranch::ModelThinking,
        EventAuthor::Agent,
        KernelTranscriptPayload::SessionState {
            update: SessionStateUpdate::Compaction(SessionCompactionState::normal()),
        },
    );
    event.id = "transcript-1".to_string();
    event.timestamp = timestamp;
    event
}

fn summary(timestamp: chrono::DateTime<Utc>, id: &str) -> KernelSessionSummary {
    KernelSessionSummary {
        version: 2,
        header: KernelArtifactHeader {
            id: id.to_string(),
            session_id: "session-1".to_string(),
            turn_id: None,
            timestamp,
        },
        covers_until_seq: 7,
        source_turn_count: 1,
        content: "Stable session summary".to_string(),
    }
}

#[tokio::test]
async fn transcript_summary_and_reset_follow_provider_session_lifecycle() {
    let workspace = tempfile::tempdir().expect("temp workspace");
    let store = FileKernelSessionArtifactStore;
    let timestamp = Utc.with_ymd_and_hms(2026, 7, 17, 10, 0, 0).unwrap();
    let transcript = transcript(timestamp);
    let summary = summary(timestamp, "summary-1");

    store
        .append_transcript_event(workspace.path(), &transcript)
        .await
        .expect("append transcript");
    let stored: serde_json::Value = serde_json::from_str(
        std::fs::read_to_string(day_file(workspace.path(), "session-1", timestamp))
            .expect("read artifact")
            .lines()
            .next()
            .expect("artifact line"),
    )
    .expect("parse artifact envelope");
    assert_eq!(stored["schema_version"], ARTIFACT_SCHEMA_VERSION);
    store
        .save_summary(workspace.path(), &summary)
        .await
        .expect("save summary");

    assert_eq!(
        store
            .load_transcript_events(
                workspace.path(),
                "session-1",
                Some("turn-1"),
                Some(ExecutionBranch::ModelThinking),
            )
            .await
            .expect("load transcript"),
        vec![transcript]
    );
    assert_eq!(
        store
            .load_summaries(workspace.path(), "session-1", 1)
            .await
            .expect("load summary"),
        vec![summary]
    );

    store
        .reset_session(workspace.path(), "session-1")
        .await
        .expect("reset session");

    assert!(store
        .load_transcript_events(workspace.path(), "session-1", None, None)
        .await
        .expect("load transcript after reset")
        .is_empty());
    assert!(store
        .load_summaries(workspace.path(), "session-1", 1)
        .await
        .expect("load summaries after reset")
        .is_empty());
}

#[test]
fn legacy_session_state_is_migrated_into_the_typed_payload() {
    let timestamp = Utc.with_ymd_and_hms(2026, 7, 17, 10, 0, 0).unwrap();
    let legacy = serde_json::json!({
        "kind": "transcript",
        "record": {
            "id": "transcript-1",
            "session_id": "session-1",
            "invocation_id": "turn-1",
            "branch": "model_thinking",
            "author": "system",
            "payload": { "type": "state_only" },
            "actions": {
                "state_delta": [{
                    "namespace": "session",
                    "key": "session_compaction_state",
                    "value": SessionCompactionState::recovery(),
                }],
                "end_turn": false,
                "transfer_to": null,
                "branch_override": null,
                "rewind_before_invocation_id": null,
                "compaction": null,
            },
            "partial": false,
            "timestamp": timestamp,
        },
    });

    let record = parse_artifact_record(
        std::path::Path::new("legacy.jsonl"),
        "line 1",
        &legacy.to_string(),
        "session-1",
    )
    .expect("migrate legacy transcript");

    let SessionArtifactRecord::Transcript { record } = record else {
        panic!("expected transcript record");
    };
    assert_eq!(
        record.payload,
        KernelTranscriptPayload::SessionState {
            update: SessionStateUpdate::Compaction(SessionCompactionState::recovery()),
        }
    );
}

#[test]
fn legacy_default_actions_are_removed_from_regular_transcript_events() {
    let timestamp = Utc.with_ymd_and_hms(2026, 7, 17, 10, 0, 0).unwrap();
    let legacy = serde_json::json!({
        "kind": "transcript",
        "record": {
            "id": "transcript-1",
            "session_id": "session-1",
            "invocation_id": "turn-1",
            "branch": "model_thinking",
            "author": "system",
            "payload": {
                "type": "auth",
                "payload": { "provider": "example", "prompt": "Sign in" },
            },
            "actions": {
                "state_delta": [],
                "end_turn": false,
                "transfer_to": null,
                "branch_override": null,
                "rewind_before_invocation_id": null,
                "compaction": null,
            },
            "partial": false,
            "timestamp": timestamp,
        },
    });

    let record = parse_artifact_record(
        std::path::Path::new("legacy.jsonl"),
        "line 1",
        &legacy.to_string(),
        "session-1",
    )
    .expect("migrate legacy transcript");

    let SessionArtifactRecord::Transcript { record } = record else {
        panic!("expected transcript record");
    };
    assert_eq!(
        record.payload,
        KernelTranscriptPayload::Auth {
            payload: KernelTranscriptAuthPayload {
                provider: "example".to_string(),
                prompt: "Sign in".to_string(),
            },
        }
    );
}

#[tokio::test]
async fn summary_tail_read_is_bounded_to_the_requested_window() {
    let workspace = tempfile::tempdir().expect("temp workspace");
    let store = FileKernelSessionArtifactStore;
    let first = summary(
        Utc.with_ymd_and_hms(2026, 7, 17, 10, 0, 0).unwrap(),
        "summary-1",
    );
    let second = summary(
        Utc.with_ymd_and_hms(2026, 7, 18, 10, 0, 0).unwrap(),
        "summary-2",
    );
    store
        .save_summary(workspace.path(), &first)
        .await
        .expect("save first summary");
    store
        .save_summary(workspace.path(), &second)
        .await
        .expect("save second summary");

    assert_eq!(
        store
            .load_summaries(workspace.path(), "session-1", 1)
            .await
            .expect("load latest summary"),
        vec![second]
    );
}

#[tokio::test]
async fn invalid_transcript_identity_fails_before_writing_state() {
    let workspace = tempfile::tempdir().expect("temp workspace");
    let store = FileKernelSessionArtifactStore;
    let mut invalid = transcript(Utc.with_ymd_and_hms(2026, 7, 17, 10, 0, 0).unwrap());
    invalid.invocation_id.clear();

    store
        .append_transcript_event(workspace.path(), &invalid)
        .await
        .expect_err("invalid transcript identity should fail");
    assert_eq!(
        store
            .session_size_bytes(workspace.path(), "session-1")
            .await
            .expect("read session artifact size"),
        0
    );
}
