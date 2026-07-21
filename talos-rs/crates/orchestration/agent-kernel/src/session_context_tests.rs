use chrono::{TimeZone, Utc};
use talos_store::session::{
    FileSessionArtifactStore, RecordHeader, SessionArtifactStore, TalosTurn,
};
use talos_types::talos::{RunItem, StopReason, TokenUsage};

use super::{
    build_session_prompt_context, SessionPromptContext, SessionPromptContextConfig,
    SessionPromptRecentWindow, TalosMessage, TalosMessageRole,
};

#[tokio::test]
async fn prompt_context_uses_the_requested_latest_window() {
    let workspace =
        std::env::temp_dir().join(format!("talos-session-context-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&workspace).expect("create temp workspace");
    let store = FileSessionArtifactStore::default();
    for seq in 1..=4 {
        store
            .append_turn(
                &workspace,
                &TalosTurn {
                    turn_id: format!("turn-{seq}"),
                    message_id: format!("message-{seq}"),
                    session_id: "session-1".to_string(),
                    seq,
                    timestamp: Utc
                        .with_ymd_and_hms(2026, 7, 17, 10, seq as u32, 0)
                        .unwrap(),
                    items: vec![
                        RunItem::UserInput {
                            text: format!("user {seq}"),
                        },
                        RunItem::OutputText {
                            text: format!("assistant {seq}"),
                        },
                    ],
                    usage: TokenUsage::default(),
                    stop_reason: StopReason::EndTurn,
                },
            )
            .await
            .expect("append turn");
    }

    let context = build_session_prompt_context(
        &store,
        &workspace,
        "session-1",
        &SessionPromptContextConfig {
            compact_cursor: None,
            recent_window: SessionPromptRecentWindow::Latest,
            recent_messages_limit: 2,
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
    .expect("build prompt context");

    let timestamp = Utc.with_ymd_and_hms(2026, 7, 17, 10, 4, 0).unwrap();
    assert_eq!(
        context,
        SessionPromptContext {
            summaries: Vec::new(),
            recent_messages: vec![
                TalosMessage {
                    header: RecordHeader {
                        id: "turn-4:user:0".to_string(),
                        session_id: "session-1".to_string(),
                        turn_id: Some("turn-4".to_string()),
                        timestamp,
                    },
                    role: TalosMessageRole::User,
                    text: "user 4".to_string(),
                },
                TalosMessage {
                    header: RecordHeader {
                        id: "turn-4:assistant:1".to_string(),
                        session_id: "session-1".to_string(),
                        turn_id: Some("turn-4".to_string()),
                        timestamp,
                    },
                    role: TalosMessageRole::Assistant,
                    text: "assistant 4".to_string(),
                },
            ],
            recent_tool_results: Vec::new(),
            recent_tool_exchanges: Vec::new(),
        }
    );
    std::fs::remove_dir_all(workspace).expect("remove temp workspace");
}
