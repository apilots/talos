use super::*;
use agent_protocol::{
    AgentExecutionCommand, AgentExecutionSettlement, AgentInputParams, AgentRunCheckpoint,
    AgentSessionContext,
};
use talos_agent_api::{
    AgentEvent, AgentMessage, AgentMessageBlock, AgentMessageBlockContent, AgentMessageBlockStatus,
    AgentMessageRole, AgentMessageStatus, AgentRunMetrics, AgentStopReason, AgentTurnCompleted,
    AgentTurnRef, AgentTurnStarted, AgentUserContent,
};

pub(crate) async fn admit_turn(
    store: &WorkerExecutionStore,
    scope: &AgentSessionContext,
    index: usize,
    bytes: usize,
    terminal: bool,
) {
    let identity = AgentTurnRef::new(&scope.session_id, format!("turn-{index}"));
    let message = AgentMessage {
        message_id: format!("message-{index}"),
        turn_id: identity.turn_id.clone(),
        role: AgentMessageRole::User,
        status: AgentMessageStatus::Completed,
        blocks: vec![AgentMessageBlock {
            block_id: format!("block-{index}"),
            status: AgentMessageBlockStatus::Completed,
            content: AgentMessageBlockContent::UserContent(AgentUserContent::Text {
                text: "inspect local results".into(),
            }),
            started_at: 1,
            completed_at: Some(1),
        }],
        created_at: 1,
        completed_at: Some(1),
    };
    let command = AgentExecutionCommand::Input {
        params: Box::new(AgentInputParams {
            agent_id: scope.agent_id.clone(),
            identity: identity.clone(),
            input: message.clone(),
            command: None,
        }),
    };
    let mut checkpoint = AgentRunCheckpoint {
        last_sequence: 1,
        messages: vec![
            message.clone(),
            AgentMessage {
                message_id: format!("tool-message-{index}"),
                turn_id: identity.turn_id.clone(),
                role: AgentMessageRole::Tool,
                status: AgentMessageStatus::Completed,
                blocks: vec![AgentMessageBlock {
                    block_id: format!("tool-result-{index}"),
                    status: AgentMessageBlockStatus::Completed,
                    content: AgentMessageBlockContent::ToolResult(
                        talos_agent_api::AgentToolResult {
                            metrics: None,
                            call_id: format!("tool-{index}"),
                            tool_name: "read_file".into(),
                            content: vec![talos_types::TalosContentItem::Text {
                                text: "x".repeat(bytes),
                            }],
                            is_error: false,
                        },
                    ),
                    started_at: 1,
                    completed_at: Some(1),
                }],
                created_at: 1,
                completed_at: Some(1),
            },
        ],
        metrics: AgentRunMetrics::default(),
    };
    store
        .admit(
            scope,
            &command,
            vec![AgentEvent {
                identity: identity.clone(),
                sequence: 1,
                occurred_at: 1,
                payload: AgentEventPayload::TurnStarted(AgentTurnStarted {}),
            }],
            &checkpoint,
        )
        .await
        .unwrap();
    if terminal {
        checkpoint.last_sequence = 2;
        store
            .append(
                scope,
                vec![AgentEvent {
                    identity,
                    sequence: 2,
                    occurred_at: 2,
                    payload: AgentEventPayload::TurnCompleted(AgentTurnCompleted {
                        stop_reason: AgentStopReason::EndTurn,
                    }),
                }],
                &checkpoint,
                Some(AgentExecutionSettlement::Finished {
                    memory_usage: Vec::new(),
                    metrics: AgentRunMetrics::default(),
                }),
            )
            .await
            .unwrap();
    }
}
pub(crate) fn scope() -> AgentSessionContext {
    AgentSessionContext {
        agent_id: "agent-1".into(),
        session_id: "session-1".into(),
        workspace: talos_config::EffectiveWorkspace {
            workspace_id: "default".into(),
            user_id: "alice".into(),
            root: "/tmp/worker-history".into(),
        },
    }
}

#[tokio::test]
async fn history_larger_than_rpc_limit_loads_locally_with_configured_window_order() {
    let directory = tempfile::tempdir().unwrap();
    let store = Arc::new(
        WorkerExecutionStore::open(&directory.path().join("worker.sqlite"))
            .await
            .unwrap(),
    );
    for index in 0..12 {
        admit_turn(&store, &scope(), index, 400_000, true).await;
    }
    let history = WorkerExecutionHistory(store.clone());
    let all = history
        .read(SessionHistoryReadParams {
            session_id: "session-1".into(),
            window: SessionHistoryWindow::Latest,
            limit: 12,
        })
        .await
        .unwrap();
    assert_eq!(all.len(), 12);
    assert!(serde_json::to_vec(&all).unwrap().len() > 4 * 1024 * 1024);
    let latest = history
        .read(SessionHistoryReadParams {
            session_id: "session-1".into(),
            window: SessionHistoryWindow::Latest,
            limit: 2,
        })
        .await
        .unwrap();
    assert_eq!(
        latest
            .iter()
            .map(|turn| turn.identity.turn_id.as_str())
            .collect::<Vec<_>>(),
        vec!["turn-10", "turn-11"]
    );
    let oldest = history
        .read(SessionHistoryReadParams {
            session_id: "session-1".into(),
            window: SessionHistoryWindow::OldestAfter {
                completed_event_seq: all[1].completed_event_seq,
            },
            limit: 2,
        })
        .await
        .unwrap();
    assert_eq!(
        oldest
            .iter()
            .map(|turn| turn.identity.turn_id.as_str())
            .collect::<Vec<_>>(),
        vec!["turn-2", "turn-3"]
    );
    use talos_agent_kernel::{
        SessionPromptContextConfig, SessionPromptContextSource, SessionPromptRecentWindow,
    };
    let source = talos_agent_provider_kernel::KernelSessionPromptContextSource::new(
        Arc::new(history.clone()),
        Arc::new(talos_agent_provider_kernel::FileKernelSessionArtifactStore),
        directory.path().to_owned(),
        "session-1".into(),
    );
    for window in [
        SessionPromptRecentWindow::Latest,
        SessionPromptRecentWindow::OldestUncovered,
    ] {
        let projected = source
            .load(SessionPromptContextConfig {
                compact_cursor: None,
                recent_window: window,
                recent_messages_limit: 12,
                recent_tool_results_limit: 12,
                recent_tool_exchanges_limit: 12,
                tool_output_max_chars: 1_000,
                tool_exchange_max_chars: 1_000,
                include_summary: false,
                summary_recent_count: 0,
                summary_max_chars: 1_000,
            })
            .await
            .unwrap();
        assert!(serde_json::to_vec(&projected).unwrap().len() < 64 * 1024);
    }
    history
        .advance_processed_until_event_seq(
            "session-1",
            0,
            all.last().unwrap().completed_event_seq as u64,
        )
        .await
        .unwrap();
    assert_eq!(
        history
            .processed_until_event_seq("session-1")
            .await
            .unwrap(),
        all.last().unwrap().completed_event_seq as u64
    );
}

#[tokio::test]
async fn worker_restart_settles_only_its_own_unfinished_execution() {
    let directory = tempfile::tempdir().unwrap();
    let store = WorkerExecutionStore::open(&directory.path().join("worker.sqlite"))
        .await
        .unwrap();
    admit_turn(&store, &scope(), 0, 10, false).await;
    recover_interrupted(&store).await.unwrap();
    let terminal = store.terminal_events("session-1", None, 256).await.unwrap();
    assert!(
        matches!(&terminal[0].1.payload,AgentEventPayload::TurnFailed(failed) if failed.error.code==ErrorCode::Cancelled)
    );
    assert!(store
        .execution_state(&scope())
        .await
        .unwrap()
        .unwrap()
        .active_turn
        .is_none());
}
