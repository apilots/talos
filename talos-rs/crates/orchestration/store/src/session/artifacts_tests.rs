use chrono::{TimeZone, Utc};
use talos_types::talos::{RunItem, StopReason, TokenUsage};

use super::{
    FileSessionArtifactStore, RecordHeader, SessionArtifactStore, SessionTranscriptActions,
    SessionTranscriptAuthor, SessionTranscriptBranch, SessionTranscriptEventKind,
    SessionTranscriptHeader, SessionTranscriptPayload, SessionTranscriptRecord, SessionTurnWindow,
    SummaryRecord, TalosTurn,
};

#[tokio::test]
async fn turn_roundtrip_preserves_sequence_and_content() {
    let workspace = tempfile::tempdir().expect("temp workspace");
    let store = FileSessionArtifactStore::default();
    let turn = TalosTurn {
        turn_id: "turn-1".to_string(),
        message_id: "message-1".to_string(),
        session_id: "session-1".to_string(),
        seq: 1,
        timestamp: Utc.with_ymd_and_hms(2026, 7, 17, 10, 0, 0).unwrap(),
        items: vec![RunItem::UserInput {
            text: "hello".to_string(),
        }],
        usage: TokenUsage::default(),
        stop_reason: StopReason::EndTurn,
    };

    store
        .append_turn(workspace.path(), &turn)
        .await
        .expect("append turn");

    assert_eq!(
        store
            .load_turns(workspace.path(), "session-1")
            .await
            .expect("load turns"),
        vec![turn]
    );
    assert_eq!(
        store
            .next_turn_seq(workspace.path(), "session-1")
            .await
            .expect("next seq"),
        2
    );
}

#[tokio::test]
async fn failed_turn_append_does_not_consume_the_sequence() {
    let workspace = tempfile::tempdir().expect("temp workspace");
    let store = FileSessionArtifactStore::default();
    let seq = store
        .next_turn_seq(workspace.path(), "session-1")
        .await
        .expect("next seq");
    let invalid_turn = TalosTurn {
        turn_id: "turn-1".to_string(),
        message_id: String::new(),
        session_id: "session-1".to_string(),
        seq,
        timestamp: Utc.with_ymd_and_hms(2026, 7, 17, 10, 0, 0).unwrap(),
        items: Vec::new(),
        usage: TokenUsage::default(),
        stop_reason: StopReason::EndTurn,
    };

    store
        .append_turn(workspace.path(), &invalid_turn)
        .await
        .expect_err("invalid turn should fail");

    assert_eq!(
        store
            .next_turn_seq(workspace.path(), "session-1")
            .await
            .expect("next seq after failed append"),
        seq
    );
}

#[tokio::test]
async fn sequence_state_failure_does_not_reverse_a_successful_artifact_append() {
    let workspace = tempfile::tempdir().expect("temp workspace");
    let session_dir = workspace.path().join("sessions/session-1");
    std::fs::create_dir_all(session_dir.join("sequence.json"))
        .expect("sequence path should block file creation");
    let store = FileSessionArtifactStore::default();
    let turn = TalosTurn {
        turn_id: "turn-1".to_string(),
        message_id: "message-1".to_string(),
        session_id: "session-1".to_string(),
        seq: 1,
        timestamp: Utc.with_ymd_and_hms(2026, 7, 17, 10, 0, 0).unwrap(),
        items: Vec::new(),
        usage: TokenUsage::default(),
        stop_reason: StopReason::EndTurn,
    };

    store
        .append_turn(workspace.path(), &turn)
        .await
        .expect("artifact append should define success");

    assert_eq!(
        store
            .load_turns(workspace.path(), "session-1")
            .await
            .expect("persisted artifact should be readable"),
        vec![turn]
    );
    assert_eq!(
        store
            .next_turn_seq(workspace.path(), "session-1")
            .await
            .expect("cache should retain the next sequence"),
        2
    );

    std::fs::remove_dir(session_dir.join("sequence.json"))
        .expect("blocking sequence directory should be removable");
    assert_eq!(
        FileSessionArtifactStore::default()
            .next_turn_seq(workspace.path(), "session-1")
            .await
            .expect("artifact should rebuild the next sequence after restart"),
        2
    );
}

#[tokio::test]
async fn transcript_summary_and_reset_follow_session_lifecycle() {
    let workspace = tempfile::tempdir().expect("temp workspace");
    let store = FileSessionArtifactStore::default();
    let timestamp = Utc.with_ymd_and_hms(2026, 7, 17, 10, 0, 0).unwrap();
    let transcript = SessionTranscriptRecord {
        header: SessionTranscriptHeader {
            id: "transcript-1".to_string(),
            session_id: "session-1".to_string(),
            invocation_id: "turn-1".to_string(),
            branch: SessionTranscriptBranch::ModelThinking,
            timestamp,
        },
        author: SessionTranscriptAuthor::Agent,
        kind: SessionTranscriptEventKind::AgentMessage,
        payload: SessionTranscriptPayload::StateOnly,
        actions: SessionTranscriptActions::default(),
        partial: false,
    };
    let summary = SummaryRecord {
        version: 2,
        header: RecordHeader {
            id: "summary-1".to_string(),
            session_id: "session-1".to_string(),
            turn_id: None,
            timestamp,
        },
        covers_until_seq: 1,
        source_turn_count: 1,
        content: "Stable session summary".to_string(),
    };

    store
        .append_transcript_record(workspace.path(), &transcript)
        .await
        .expect("append transcript");
    store
        .save_summary(workspace.path(), &summary)
        .await
        .expect("save summary");

    assert_eq!(
        store
            .load_transcript_records(
                workspace.path(),
                "session-1",
                Some("turn-1"),
                Some(SessionTranscriptBranch::ModelThinking),
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

    assert_eq!(
        store
            .next_turn_seq(workspace.path(), "session-1")
            .await
            .expect("next seq after reset"),
        1
    );
    assert_eq!(
        store
            .load_summaries(workspace.path(), "session-1", 1)
            .await
            .expect("load summaries after reset"),
        Vec::new()
    );
}

#[tokio::test]
async fn reset_preserves_the_next_turn_sequence_after_restart() {
    let workspace = tempfile::tempdir().expect("temp workspace");
    let store = FileSessionArtifactStore::default();
    let turn = TalosTurn {
        turn_id: "turn-1".to_string(),
        message_id: "message-1".to_string(),
        session_id: "session-1".to_string(),
        seq: 1,
        timestamp: Utc.with_ymd_and_hms(2026, 7, 17, 10, 0, 0).unwrap(),
        items: vec![RunItem::UserInput {
            text: "discarded by clear".to_string(),
        }],
        usage: TokenUsage::default(),
        stop_reason: StopReason::EndTurn,
    };
    store
        .append_turn(workspace.path(), &turn)
        .await
        .expect("append turn");

    store
        .reset_session(workspace.path(), "session-1")
        .await
        .expect("reset session");

    let restarted_store = FileSessionArtifactStore::default();
    assert_eq!(
        restarted_store
            .load_turns(workspace.path(), "session-1")
            .await
            .expect("load turns after reset"),
        Vec::new()
    );
    assert_eq!(
        restarted_store
            .next_turn_seq(workspace.path(), "session-1")
            .await
            .expect("next seq after restart"),
        2
    );
}

#[tokio::test]
async fn turn_windows_return_only_the_requested_sequence_range() {
    let workspace = tempfile::tempdir().expect("temp workspace");
    let store = FileSessionArtifactStore::default();
    let mut turns = Vec::new();
    for seq in 1..=5 {
        let turn = TalosTurn {
            turn_id: format!("turn-{seq}"),
            message_id: format!("message-{seq}"),
            session_id: "session-1".to_string(),
            seq,
            timestamp: Utc
                .with_ymd_and_hms(2026, 7, 17, 10, seq as u32, 0)
                .unwrap(),
            items: vec![RunItem::UserInput {
                text: format!("message {seq}"),
            }],
            usage: TokenUsage::default(),
            stop_reason: StopReason::EndTurn,
        };
        store
            .append_turn(workspace.path(), &turn)
            .await
            .expect("append turn");
        turns.push(turn);
    }

    assert_eq!(
        store
            .load_turn_window(workspace.path(), "session-1", SessionTurnWindow::Latest, 2,)
            .await
            .expect("load latest turns"),
        turns[3..].to_vec()
    );
    assert_eq!(
        store
            .load_turn_window(
                workspace.path(),
                "session-1",
                SessionTurnWindow::OldestAfter { seq: 2 },
                2,
            )
            .await
            .expect("load oldest uncovered turns"),
        turns[2..4].to_vec()
    );
}

#[tokio::test]
async fn bounded_artifact_reads_stop_after_the_requested_window() {
    let workspace = tempfile::tempdir().expect("temp workspace");
    let store = FileSessionArtifactStore::default();
    let turn = TalosTurn {
        turn_id: "turn-1".to_string(),
        message_id: "message-1".to_string(),
        session_id: "session-1".to_string(),
        seq: 1,
        timestamp: Utc.with_ymd_and_hms(2026, 7, 18, 10, 0, 0).unwrap(),
        items: vec![RunItem::UserInput {
            text: "bounded".to_string(),
        }],
        usage: TokenUsage::default(),
        stop_reason: StopReason::EndTurn,
    };
    store
        .append_turn(workspace.path(), &turn)
        .await
        .expect("append turn");
    let summary = SummaryRecord {
        version: 2,
        header: RecordHeader {
            id: "summary-1".to_string(),
            session_id: "session-1".to_string(),
            turn_id: None,
            timestamp: turn.timestamp,
        },
        covers_until_seq: 1,
        source_turn_count: 1,
        content: "bounded summary".to_string(),
    };
    store
        .save_summary(workspace.path(), &summary)
        .await
        .expect("save summary");
    talos_fs::write_file(
        &workspace.path().join("sessions/session-1/2026-07-17.jsonl"),
        "invalid old artifact\n",
    )
    .await
    .expect("write unread old artifact");

    assert_eq!(
        store
            .load_turn_window(workspace.path(), "session-1", SessionTurnWindow::Latest, 1)
            .await
            .expect("load bounded latest turn"),
        vec![turn]
    );
    assert_eq!(
        store
            .load_summaries(workspace.path(), "session-1", 1)
            .await
            .expect("load bounded summary"),
        vec![summary]
    );
}

#[tokio::test]
async fn oldest_turn_window_stops_reading_at_the_limit() {
    let workspace = tempfile::tempdir().expect("temp workspace");
    let store = FileSessionArtifactStore::default();
    let turn = TalosTurn {
        turn_id: "turn-1".to_string(),
        message_id: "message-1".to_string(),
        session_id: "session-1".to_string(),
        seq: 1,
        timestamp: Utc.with_ymd_and_hms(2026, 7, 18, 10, 0, 0).unwrap(),
        items: vec![RunItem::UserInput {
            text: "bounded".to_string(),
        }],
        usage: TokenUsage::default(),
        stop_reason: StopReason::EndTurn,
    };
    store
        .append_turn(workspace.path(), &turn)
        .await
        .expect("append turn");
    talos_fs::append_file(
        &workspace.path().join("sessions/session-1/2026-07-18.jsonl"),
        "invalid trailing artifact\n",
    )
    .await
    .expect("append unread trailing artifact");

    assert_eq!(
        store
            .load_turn_window(
                workspace.path(),
                "session-1",
                SessionTurnWindow::OldestAfter { seq: 0 },
                1,
            )
            .await
            .expect("load bounded oldest turn"),
        vec![turn]
    );
}

#[tokio::test]
async fn oldest_turn_window_skips_daily_files_before_the_cursor() {
    let workspace = tempfile::tempdir().expect("temp workspace");
    let store = FileSessionArtifactStore::default();
    let old_turn = TalosTurn {
        turn_id: "turn-1".to_string(),
        message_id: "message-1".to_string(),
        session_id: "session-1".to_string(),
        seq: 1,
        timestamp: Utc.with_ymd_and_hms(2026, 7, 17, 10, 0, 0).unwrap(),
        items: vec![RunItem::UserInput {
            text: "old".to_string(),
        }],
        usage: TokenUsage::default(),
        stop_reason: StopReason::EndTurn,
    };
    let new_turn = TalosTurn {
        turn_id: "turn-2".to_string(),
        message_id: "message-2".to_string(),
        session_id: "session-1".to_string(),
        seq: 2,
        timestamp: Utc.with_ymd_and_hms(2026, 7, 18, 10, 0, 0).unwrap(),
        items: vec![RunItem::UserInput {
            text: "new".to_string(),
        }],
        usage: TokenUsage::default(),
        stop_reason: StopReason::EndTurn,
    };
    store
        .append_turn(workspace.path(), &old_turn)
        .await
        .expect("append old turn");
    store
        .append_turn(workspace.path(), &new_turn)
        .await
        .expect("append new turn");
    let old_path = workspace.path().join("sessions/session-1/2026-07-17.jsonl");
    let old_content = talos_fs::read_file(&old_path)
        .await
        .expect("read old file")
        .expect("old file");
    talos_fs::write_file(
        &old_path,
        &format!("invalid leading artifact\n{old_content}"),
    )
    .await
    .expect("write unread old prefix");

    assert_eq!(
        store
            .load_turn_window(
                workspace.path(),
                "session-1",
                SessionTurnWindow::OldestAfter { seq: 1 },
                1,
            )
            .await
            .expect("skip old daily file"),
        vec![new_turn]
    );
}

#[tokio::test]
async fn oldest_turn_window_locates_the_first_uncovered_daily_file() {
    let workspace = tempfile::tempdir().expect("temp workspace");
    let store = FileSessionArtifactStore::default();
    let mut turns = Vec::new();
    for seq in 1..=8_u64 {
        let turn = TalosTurn {
            turn_id: format!("turn-{seq}"),
            message_id: format!("message-{seq}"),
            session_id: "session-1".to_string(),
            seq,
            timestamp: Utc
                .with_ymd_and_hms(2026, 7, 10 + seq as u32, 10, 0, 0)
                .unwrap(),
            items: vec![RunItem::UserInput {
                text: format!("message {seq}"),
            }],
            usage: TokenUsage::default(),
            stop_reason: StopReason::EndTurn,
        };
        store
            .append_turn(workspace.path(), &turn)
            .await
            .expect("append turn");
        turns.push(turn);
    }
    talos_fs::append_file(
        &workspace.path().join("sessions/session-1/2026-07-11.jsonl"),
        "invalid old tail\n",
    )
    .await
    .expect("append unread old tail");

    assert_eq!(
        store
            .load_turn_window(
                workspace.path(),
                "session-1",
                SessionTurnWindow::OldestAfter { seq: 7 },
                1,
            )
            .await
            .expect("locate first uncovered daily file"),
        turns[7..].to_vec()
    );
}
