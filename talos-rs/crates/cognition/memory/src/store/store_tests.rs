use chrono::Utc;
use talos_error::TalosError;
use talos_llm_provider::ChatMessage;
use tempfile::TempDir;

use crate::Memory;
use crate::TalosMessageRole;
use crate::{
    MemorySearchQuery, MemoryStore, RecordHeader, SessionPromptContextConfig,
    SessionTranscriptActions, SessionTranscriptAuthor, SessionTranscriptBranch,
    SessionTranscriptEventKind, SessionTranscriptHeader, SessionTranscriptPayload,
    SessionTranscriptRecord, SummaryRecord, TalosTurn, WorkspaceMemoryRecord,
};
use talos_types::talos::{RunItem, StopReason, TalosContentItem, TokenUsage};

fn new_temp_store() -> (TempDir, MemoryStore) {
    let temp_dir = tempfile::tempdir().expect("temp dir should be created");
    let store = MemoryStore::new(temp_dir.path().to_path_buf());
    (temp_dir, store)
}

#[tokio::test]
async fn test_turn_roundtrip_and_context_dedup() {
    let (_guard, store) = new_temp_store();
    let session_id = "s1";
    let seq1 = store
        .next_turn_seq(session_id)
        .await
        .expect("seq should be 1");
    assert_eq!(seq1, 1);

    let turn_1 = TalosTurn {
        turn_id: "t1".to_string(),
        message_id: "msg-1".to_string(),
        session_id: session_id.to_string(),
        seq: seq1,
        timestamp: Utc::now(),
        items: vec![
            RunItem::UserInput {
                text: "hello".to_string(),
            },
            RunItem::OutputText {
                text: "world".to_string(),
            },
        ],
        usage: TokenUsage::default(),
        stop_reason: StopReason::EndTurn,
    };

    store
        .append_turn(&turn_1)
        .await
        .expect("append should succeed");

    let seq2 = store
        .next_turn_seq(session_id)
        .await
        .expect("seq should be 2");
    assert_eq!(seq2, 2);

    let context = store
        .build_session_prompt_context(
            session_id,
            &SessionPromptContextConfig {
                recent_messages_limit: 8,
                recent_tool_results_limit: 0,
                tool_output_max_chars: 0,
                recent_tool_exchanges_limit: 0,
                tool_exchange_max_chars: 0,
                include_summary: false,
                summary_recent_count: 0,
                summary_max_chars: 0,
            },
        )
        .await
        .expect("context should build");
    assert_eq!(context.recent_messages.len(), 2);
}

#[tokio::test]
async fn test_prompt_context_falls_back_to_transcript_records() {
    let (_guard, store) = new_temp_store();
    let session_id = "transcript-only-session";
    let invocation_id = "inv-transcript-only";

    for (index, message) in [
        ChatMessage::user("remember transcript-only close flush"),
        ChatMessage::assistant("noted for session close"),
        ChatMessage::tool("call-1", "MemorySearch", "{\"results\":[]}"),
    ]
    .into_iter()
    .enumerate()
    {
        store
            .append_transcript_record(&SessionTranscriptRecord {
                header: SessionTranscriptHeader {
                    id: format!("record-{index}"),
                    session_id: session_id.to_string(),
                    invocation_id: invocation_id.to_string(),
                    branch: SessionTranscriptBranch::ModelThinking,
                    timestamp: Utc::now(),
                },
                author: SessionTranscriptAuthor::Agent,
                kind: SessionTranscriptEventKind::AgentMessage,
                payload: SessionTranscriptPayload::Message { message },
                actions: SessionTranscriptActions::default(),
                partial: false,
            })
            .await
            .expect("transcript record should append");
    }

    let context = store
        .build_session_prompt_context(
            session_id,
            &SessionPromptContextConfig {
                recent_messages_limit: 8,
                recent_tool_results_limit: 8,
                tool_output_max_chars: 1024,
                recent_tool_exchanges_limit: 0,
                tool_exchange_max_chars: 0,
                include_summary: false,
                summary_recent_count: 0,
                summary_max_chars: 0,
            },
        )
        .await
        .expect("context should build from transcript records");

    assert_eq!(context.recent_messages.len(), 2);
    assert_eq!(context.recent_tool_results.len(), 1);
    assert_eq!(context.recent_tool_results[0].tool_name, "MemorySearch");
}

#[tokio::test]
async fn test_session_turns_project_cancelled_transcript_user_input() {
    let (_guard, store) = new_temp_store();
    let session_id = "cancelled-transcript-session";
    let invocation_id = "inv-cancelled";

    store
        .append_transcript_record(&SessionTranscriptRecord {
            header: SessionTranscriptHeader {
                id: "cancelled-user-record".to_string(),
                session_id: session_id.to_string(),
                invocation_id: invocation_id.to_string(),
                branch: SessionTranscriptBranch::ModelThinking,
                timestamp: Utc::now(),
            },
            author: SessionTranscriptAuthor::User,
            kind: SessionTranscriptEventKind::UserMessage,
            payload: SessionTranscriptPayload::Message {
                message: ChatMessage::user("keep this cancelled input visible"),
            },
            actions: SessionTranscriptActions::default(),
            partial: false,
        })
        .await
        .expect("transcript record should append");

    let turns = store
        .load_session_turns(session_id)
        .await
        .expect("turns should load");

    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].turn_id, invocation_id);
    assert_eq!(turns[0].stop_reason, StopReason::Cancelled);
    assert!(matches!(
        turns[0].items.first(),
        Some(RunItem::UserInput { text }) if text == "keep this cancelled input visible"
    ));
}

#[tokio::test]
async fn test_session_turns_do_not_duplicate_completed_transcripts() {
    let (_guard, store) = new_temp_store();
    let session_id = "completed-transcript-session";
    let invocation_id = "inv-completed";
    let timestamp = Utc::now();

    store
        .append_transcript_record(&SessionTranscriptRecord {
            header: SessionTranscriptHeader {
                id: "completed-user-record".to_string(),
                session_id: session_id.to_string(),
                invocation_id: invocation_id.to_string(),
                branch: SessionTranscriptBranch::ModelThinking,
                timestamp,
            },
            author: SessionTranscriptAuthor::User,
            kind: SessionTranscriptEventKind::UserMessage,
            payload: SessionTranscriptPayload::Message {
                message: ChatMessage::user("completed input"),
            },
            actions: SessionTranscriptActions::default(),
            partial: false,
        })
        .await
        .expect("transcript record should append");

    store
        .append_turn(&TalosTurn {
            turn_id: "canonical-turn".to_string(),
            message_id: "msg-1".to_string(),
            session_id: session_id.to_string(),
            seq: store
                .next_turn_seq(session_id)
                .await
                .expect("seq should load"),
            timestamp: timestamp + chrono::Duration::seconds(1),
            items: vec![
                RunItem::UserInput {
                    text: "completed input".to_string(),
                },
                RunItem::OutputText {
                    text: "completed output".to_string(),
                },
            ],
            usage: TokenUsage::default(),
            stop_reason: StopReason::EndTurn,
        })
        .await
        .expect("turn should append");

    let turns = store
        .load_session_turns(session_id)
        .await
        .expect("turns should load");

    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].turn_id, "canonical-turn");
    assert_eq!(turns[0].stop_reason, StopReason::EndTurn);
}

#[tokio::test]
async fn test_turn_schema_items_roundtrip() {
    let (_guard, store) = new_temp_store();
    let session_id = "roundtrip-items";
    let turn = TalosTurn {
        turn_id: "turn-items-1".to_string(),
        message_id: "msg-1".to_string(),
        session_id: session_id.to_string(),
        seq: store
            .next_turn_seq(session_id)
            .await
            .expect("seq should load"),
        timestamp: Utc::now(),
        items: vec![
            RunItem::UserInput {
                text: "user".to_string(),
            },
            RunItem::Reasoning {
                text: "think".to_string(),
            },
            RunItem::ToolCall {
                call_id: "call-1".to_string(),
                name: "Read".to_string(),
                arguments: serde_json::json!({"path":"/tmp/a.txt"}).into(),
            },
            RunItem::ToolResult {
                call_id: "call-1".to_string(),
                tool_name: "Read".to_string(),
                content: vec![TalosContentItem::Text {
                    text: "contents".to_string(),
                }],
                is_error: false,
            },
            RunItem::OutputText {
                text: "first answer".to_string(),
            },
            RunItem::OutputText {
                text: "second answer".to_string(),
            },
        ],
        usage: TokenUsage::default(),
        stop_reason: StopReason::EndTurn,
    };

    store
        .append_turn(&turn)
        .await
        .expect("append turn should succeed");

    let turns = store
        .load_session_turns(session_id)
        .await
        .expect("turns should load");

    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].items.len(), 6);
    assert!(matches!(
        turns[0].items.first(),
        Some(RunItem::UserInput { text }) if text == "user"
    ));
    assert!(matches!(
        turns[0].items.get(1),
        Some(RunItem::Reasoning { text }) if text == "think"
    ));
    assert!(matches!(
        turns[0].items.get(2),
        Some(RunItem::ToolCall { name, .. }) if name == "Read"
    ));
    assert!(matches!(
        turns[0].items.get(3),
        Some(RunItem::ToolResult { tool_name, .. }) if tool_name == "Read"
    ));
    assert!(matches!(
        turns[0].items.get(4),
        Some(RunItem::OutputText { text }) if text == "first answer"
    ));
    assert!(matches!(
        turns[0].items.get(5),
        Some(RunItem::OutputText { text }) if text == "second answer"
    ));
}

#[tokio::test]
async fn test_summary_records() {
    let (_guard, store) = new_temp_store();
    let record = SummaryRecord {
        version: 2,
        header: RecordHeader {
            id: "sum-1".to_string(),
            session_id: "s1".to_string(),
            turn_id: None,
            timestamp: Utc::now(),
        },
        covers_until_seq: 9,
        source_turn_count: 12,
        content: "facts".to_string(),
    };

    store
        .save_summary(&record)
        .await
        .expect("save summary should succeed");
    let loaded = store
        .load_summaries("s1", 4)
        .await
        .expect("load summary should succeed");
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].header.id, "sum-1");
}

#[tokio::test]
async fn test_reset_session_clears_turns_summaries_and_sequence_cache() {
    let (_guard, store) = new_temp_store();
    let session_id = "reset-session";
    let timestamp = Utc::now();

    store
        .append_turn(&TalosTurn {
            turn_id: "turn-1".to_string(),
            message_id: "msg-1".to_string(),
            session_id: session_id.to_string(),
            seq: store
                .next_turn_seq(session_id)
                .await
                .expect("seq should load"),
            timestamp,
            items: vec![RunItem::UserInput {
                text: "hello".to_string(),
            }],
            usage: TokenUsage::default(),
            stop_reason: StopReason::EndTurn,
        })
        .await
        .expect("append turn should succeed");
    store
        .save_summary(&SummaryRecord {
            version: 2,
            header: RecordHeader {
                id: "summary-1".to_string(),
                session_id: session_id.to_string(),
                turn_id: None,
                timestamp,
            },
            covers_until_seq: 1,
            source_turn_count: 1,
            content: "facts".to_string(),
        })
        .await
        .expect("save summary should succeed");

    store
        .reset_session(session_id)
        .await
        .expect("reset session should succeed");

    assert!(store
        .load_session_turns(session_id)
        .await
        .expect("turns should load")
        .is_empty());
    assert!(store
        .load_summaries(session_id, 4)
        .await
        .expect("summaries should load")
        .is_empty());
    assert_eq!(
        store
            .latest_seq(session_id)
            .await
            .expect("latest seq should load"),
        0
    );
    assert_eq!(
        store
            .next_turn_seq(session_id)
            .await
            .expect("next seq should load"),
        1
    );
}

#[tokio::test]
async fn test_transcript_records_roundtrip_from_jsonl_artifacts() {
    let (_guard, store) = new_temp_store();
    let timestamp = Utc::now();
    let record = SessionTranscriptRecord {
        header: SessionTranscriptHeader {
            id: "transcript-1".to_string(),
            session_id: "s1".to_string(),
            invocation_id: "inv-1".to_string(),
            branch: SessionTranscriptBranch::ModelThinking,
            timestamp,
        },
        author: SessionTranscriptAuthor::Agent,
        kind: SessionTranscriptEventKind::AgentMessage,
        payload: SessionTranscriptPayload::StateOnly,
        actions: SessionTranscriptActions::default(),
        partial: false,
    };

    store
        .append_transcript_record(&record)
        .await
        .expect("append transcript should succeed");

    let loaded = store
        .load_transcript_records("s1", Some("inv-1"), SessionTranscriptBranch::ModelThinking)
        .await
        .expect("load transcript should succeed");
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].header.id, "transcript-1");

    let session_files = talos_fs::read_directory(&_guard.path().join("sessions").join("s1"))
        .expect("session dir should read")
        .expect("session dir should exist")
        .into_iter()
        .map(|entry| entry.path)
        .collect::<Vec<_>>();
    assert_eq!(session_files.len(), 1);
    assert_eq!(
        session_files[0].extension().and_then(|ext| ext.to_str()),
        Some("jsonl")
    );
}

#[tokio::test]
async fn test_build_prompt_context_includes_summary() {
    let (_guard, store) = new_temp_store();
    let session_id = "s1";
    let timestamp = Utc::now();

    store
        .append_turn(&TalosTurn {
            turn_id: "turn-1".to_string(),
            message_id: "msg-1".to_string(),
            session_id: session_id.to_string(),
            seq: store
                .next_turn_seq(session_id)
                .await
                .expect("seq should load"),
            timestamp,
            items: vec![RunItem::UserInput {
                text: "remember this".to_string(),
            }],
            usage: TokenUsage::default(),
            stop_reason: StopReason::EndTurn,
        })
        .await
        .expect("append turn should succeed");

    store
        .save_summary(&SummaryRecord {
            version: 2,
            header: RecordHeader {
                id: "summary-1".to_string(),
                session_id: session_id.to_string(),
                turn_id: None,
                timestamp,
            },
            covers_until_seq: 1,
            source_turn_count: 1,
            content: "facts".to_string(),
        })
        .await
        .expect("save summary should succeed");

    let context = store
        .build_session_prompt_context(
            session_id,
            &SessionPromptContextConfig {
                recent_messages_limit: 10,
                recent_tool_results_limit: 0,
                tool_output_max_chars: 0,
                recent_tool_exchanges_limit: 0,
                tool_exchange_max_chars: 0,
                include_summary: true,
                summary_recent_count: 4,
                summary_max_chars: 0,
            },
        )
        .await
        .expect("context should load");
    assert_eq!(context.recent_messages.len(), 1);
    assert_eq!(context.recent_messages[0].text, "remember this");
    assert_eq!(context.summaries.len(), 1);
    assert_eq!(context.summaries[0].header.id, "summary-1");
}

#[tokio::test]
async fn test_prompt_context_still_builds_after_schema_change() {
    let (_guard, store) = new_temp_store();
    let session_id = "prompt-context-items";
    let timestamp = Utc::now();

    store
        .append_turn(&TalosTurn {
            turn_id: "turn-items-context".to_string(),
            message_id: "msg-1".to_string(),
            session_id: session_id.to_string(),
            seq: store
                .next_turn_seq(session_id)
                .await
                .expect("seq should load"),
            timestamp,
            items: vec![
                RunItem::UserInput {
                    text: "hello".to_string(),
                },
                RunItem::Reasoning {
                    text: "internal".to_string(),
                },
                RunItem::ToolCall {
                    call_id: "call-context".to_string(),
                    name: "Read".to_string(),
                    arguments: serde_json::json!({"path":"/tmp/example.txt"}).into(),
                },
                RunItem::ToolResult {
                    call_id: "call-context".to_string(),
                    tool_name: "Read".to_string(),
                    content: vec![TalosContentItem::Text {
                        text: "result".to_string(),
                    }],
                    is_error: false,
                },
                RunItem::OutputText {
                    text: "world".to_string(),
                },
            ],
            usage: TokenUsage::default(),
            stop_reason: StopReason::EndTurn,
        })
        .await
        .expect("append turn should succeed");

    let context = store
        .build_session_prompt_context(
            session_id,
            &SessionPromptContextConfig {
                recent_messages_limit: 10,
                recent_tool_results_limit: 10,
                tool_output_max_chars: 32,
                recent_tool_exchanges_limit: 10,
                tool_exchange_max_chars: 32,
                include_summary: false,
                summary_recent_count: 0,
                summary_max_chars: 0,
            },
        )
        .await
        .expect("context should build");

    assert_eq!(context.recent_messages.len(), 2);
    assert_eq!(context.recent_messages[0].text, "hello");
    assert_eq!(context.recent_messages[1].text, "world");
    assert_eq!(context.recent_tool_results.len(), 1);
    assert_eq!(context.recent_tool_results[0].tool_name, "Read");
    assert_eq!(context.recent_tool_results[0].content, "result");
    assert_eq!(context.recent_tool_exchanges.len(), 1);
    assert_eq!(context.recent_tool_exchanges[0].tool_name, "Read");
    assert_eq!(
        context.recent_tool_exchanges[0].arguments_preview,
        "{\"path\":\"/tmp/example.txt\"}"
    );
    assert!(context.recent_messages.iter().all(|message| {
        matches!(
            message.role,
            TalosMessageRole::User | TalosMessageRole::Assistant
        )
    }));
}

#[tokio::test]
async fn test_session_size_bytes_after_append_and_summary() {
    let (_guard, store) = new_temp_store();
    let session_id = "s-size";
    let timestamp = Utc::now();

    store
        .append_turn(&TalosTurn {
            turn_id: "turn-size".to_string(),
            message_id: "msg-1".to_string(),
            session_id: session_id.to_string(),
            seq: store
                .next_turn_seq(session_id)
                .await
                .expect("seq should load"),
            timestamp,
            items: vec![RunItem::UserInput {
                text: "payload".to_string(),
            }],
            usage: TokenUsage::default(),
            stop_reason: StopReason::EndTurn,
        })
        .await
        .expect("append turn should succeed");

    store
        .save_summary(&SummaryRecord {
            version: 2,
            header: RecordHeader {
                id: "summary-size".to_string(),
                session_id: session_id.to_string(),
                turn_id: None,
                timestamp,
            },
            covers_until_seq: 1,
            source_turn_count: 1,
            content: "summary".to_string(),
        })
        .await
        .expect("save summary should succeed");

    let size = store
        .session_size_bytes(session_id)
        .await
        .expect("size should compute");
    assert!(size > 0);
}

#[tokio::test]
async fn test_latest_seq_tracks_appended_turns() {
    let (_guard, store) = new_temp_store();
    let session_id = "s-seq";
    let timestamp = Utc::now();

    for turn_id in 1..=2 {
        store
            .append_turn(&TalosTurn {
                turn_id: format!("turn-{}", turn_id),
                message_id: "msg-1".to_string(),
                session_id: session_id.to_string(),
                seq: store
                    .next_turn_seq(session_id)
                    .await
                    .expect("seq should load"),
                timestamp,
                items: Vec::new(),
                usage: TokenUsage::default(),
                stop_reason: StopReason::EndTurn,
            })
            .await
            .expect("append turn should succeed");
    }

    assert_eq!(
        store
            .latest_seq(session_id)
            .await
            .expect("latest seq should load"),
        2
    );
}

#[tokio::test]
async fn test_root_memory_and_workspace_memory_search() {
    let (_guard, store) = new_temp_store();
    store
        .append_workspace_memory(&WorkspaceMemoryRecord {
            timestamp: Utc::now(),
            session_id: "session-1".to_string(),
            title: "Daily".to_string(),
            content: "**Facts:**\n- deployment decision: blue/green".to_string(),
        })
        .await
        .expect("workspace memory append should succeed");

    let _error = store
        .search_memory(&MemorySearchQuery {
            query: "deployment".to_string(),
            max_results: 5,
        })
        .await
        .expect_err("memory search should require llm");
}

#[tokio::test]
async fn test_get_memory_rejects_path_escape() {
    let (_guard, store) = new_temp_store();
    let result = store.get_memory("../secrets.txt", Some(1), Some(5)).await;
    assert!(matches!(result, Err(TalosError::ValidationError(_))));
}

#[tokio::test]
async fn test_append_workspace_memory_creates_daily_file_on_first_write() {
    let (_guard, store) = new_temp_store();
    let timestamp = Utc::now();
    let record = WorkspaceMemoryRecord {
        timestamp,
        session_id: "session-1".to_string(),
        title: "Daily Memory".to_string(),
        content: "**Facts:**\n- first memory entry".to_string(),
    };

    store
        .append_workspace_memory(&record)
        .await
        .expect("first append should succeed");

    let file_path = store.workspace_memory_file_for(timestamp);
    let content = talos_fs::read_file(&file_path)
        .await
        .expect("workspace memory file should read after first append")
        .expect("workspace memory file should exist after first append");

    assert!(content.contains("first memory entry"));
    assert!(content.contains("session: session-1"));
}

#[tokio::test]
async fn test_append_workspace_memory_dedupes_duplicate_records() {
    let (_guard, store) = new_temp_store();
    let record = WorkspaceMemoryRecord {
        timestamp: Utc::now(),
        session_id: "session-1".to_string(),
        title: "Durable Memory".to_string(),
        content: "**Facts:**\n- user prefers tea over coffee".to_string(),
    };

    store
        .append_workspace_memory(&record)
        .await
        .expect("first append should succeed");
    store
        .append_workspace_memory(&record)
        .await
        .expect("second append should dedupe");

    let file_path = store.workspace_memory_file_for(record.timestamp);
    let content = talos_fs::read_file(&file_path)
        .await
        .expect("workspace memory file should read")
        .expect("workspace memory file should exist");
    let occurrences = content.matches("user prefers tea over coffee").count();
    assert_eq!(occurrences, 1);
}

#[tokio::test]
async fn test_resolve_workspace_dir_expands_home_prefix() {
    let store = MemoryStore::new("~/talos-memory-test");
    let resolved = store.workspace_dir().to_path_buf();
    assert!(
        !resolved.to_string_lossy().starts_with('~'),
        "workspace dir should be expanded"
    );
}

#[tokio::test]
async fn test_append_workspace_memory_handles_searchable_summaries() {
    let (_guard, store) = new_temp_store();
    let timestamp = Utc::now();
    let record = WorkspaceMemoryRecord {
        timestamp,
        session_id: "session-1".to_string(),
        title: "Engineering Sync".to_string(),
        content: "**Facts:**\n- keep search results concise".to_string(),
    };

    store
        .append_workspace_memory(&record)
        .await
        .expect("append should succeed");

    let query = MemorySearchQuery {
        query: "search results".to_string(),
        max_results: 3,
    };
    let _error = store
        .search_memory(&query)
        .await
        .expect_err("memory search should require llm");
}
