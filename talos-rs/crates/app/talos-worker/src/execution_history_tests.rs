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
        last_sequence: 3,
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
            std::iter::once(AgentEvent {
                identity: identity.clone(),
                sequence: 1,
                occurred_at: 1,
                payload: AgentEventPayload::TurnStarted(AgentTurnStarted {}),
            })
            .chain(
                checkpoint
                    .messages
                    .iter()
                    .enumerate()
                    .map(|(index, message)| AgentEvent {
                        identity: identity.clone(),
                        sequence: index as u64 + 2,
                        occurred_at: 1,
                        payload: AgentEventPayload::MessageCompleted(message.clone()),
                    }),
            )
            .collect(),
            &checkpoint,
        )
        .await
        .unwrap();
    if terminal {
        checkpoint.last_sequence = 4;
        store
            .append(
                scope,
                vec![AgentEvent {
                    identity,
                    sequence: 4,
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
            message_limit: 64,
        })
        .await
        .unwrap();
    assert_eq!(all.len(), 12);
    assert!(
        serde_json::to_vec(&all.iter().map(|window| &window.turn).collect::<Vec<_>>())
            .unwrap()
            .len()
            > 4 * 1024 * 1024
    );
    let latest = history
        .read(SessionHistoryReadParams {
            session_id: "session-1".into(),
            window: SessionHistoryWindow::Latest,
            limit: 2,
            message_limit: 64,
        })
        .await
        .unwrap();
    assert_eq!(
        latest
            .iter()
            .map(|turn| turn.turn.identity.turn_id.as_str())
            .collect::<Vec<_>>(),
        vec!["turn-10", "turn-11"]
    );
    let oldest = history
        .read(SessionHistoryReadParams {
            session_id: "session-1".into(),
            window: SessionHistoryWindow::OldestAfter {
                completed_event_seq: all[1].turn.completed_event_seq,
            },
            limit: 2,
            message_limit: 64,
        })
        .await
        .unwrap();
    assert_eq!(
        oldest
            .iter()
            .map(|turn| turn.turn.identity.turn_id.as_str())
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
                source_message_limit: None,
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
            all.last().unwrap().turn.completed_event_seq as u64,
        )
        .await
        .unwrap();
    assert_eq!(
        history
            .processed_until_event_seq("session-1")
            .await
            .unwrap(),
        all.last().unwrap().turn.completed_event_seq as u64
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

#[tokio::test]
async fn oversized_persisted_turn_remains_readable_in_local_windows() {
    let directory = tempfile::tempdir().unwrap();
    let store = Arc::new(
        WorkerExecutionStore::open(&directory.path().join("worker.sqlite"))
            .await
            .unwrap(),
    );
    let scope = scope();
    admit_turn(&store, &scope, 0, 400_000, false).await;
    let identity = AgentTurnRef::new(&scope.session_id, "turn-0");
    let mut checkpoint = store.checkpoint(&identity).await.unwrap().unwrap();
    checkpoint.metrics.model_rounds = 7;
    let template = checkpoint.messages[1].clone();
    let mut events = Vec::new();
    for index in 0..44 {
        let mut message = template.clone();
        message.message_id = format!("large-message-{index}");
        checkpoint.messages.push(message.clone());
        checkpoint.last_sequence += 1;
        events.push(AgentEvent {
            identity: identity.clone(),
            sequence: checkpoint.last_sequence,
            occurred_at: 2,
            payload: AgentEventPayload::MessageCompleted(message),
        });
    }
    checkpoint.last_sequence += 1;
    events.push(AgentEvent {
        identity: identity.clone(),
        sequence: checkpoint.last_sequence,
        occurred_at: 3,
        payload: AgentEventPayload::TurnCompleted(AgentTurnCompleted {
            stop_reason: AgentStopReason::EndTurn,
        }),
    });
    store
        .append(
            &scope,
            events,
            &checkpoint,
            Some(AgentExecutionSettlement::Finished {
                memory_usage: Vec::new(),
                metrics: checkpoint.metrics.clone(),
            }),
        )
        .await
        .unwrap();
    assert!(serde_json::to_vec(&checkpoint).unwrap().len() > MAX_LOCAL_HISTORY_BYTES);
    let history = WorkerExecutionHistory(store.clone());
    let terminal_offset = store
        .terminal_events(&scope.session_id, None, 1)
        .await
        .unwrap()[0]
        .0;
    let latest = history
        .read(SessionHistoryReadParams {
            session_id: scope.session_id.clone(),
            window: SessionHistoryWindow::Latest,
            limit: 1,
            message_limit: 3,
        })
        .await
        .unwrap();
    let AgentRunResult::Completed(output) = &latest[0].turn.result else {
        panic!("expected completed history")
    };
    assert_eq!(output.metrics, checkpoint.metrics);
    let latest_ids = &latest[0].message_event_sequences;
    assert_eq!(
        latest_ids
            .iter()
            .map(|(id, _)| id.as_str())
            .collect::<Vec<_>>(),
        vec!["large-message-41", "large-message-42", "large-message-43"]
    );
    assert_eq!(latest[0].turn.completed_event_seq, terminal_offset);
    let mut after = 0;
    let mut consumed = Vec::new();
    while after < terminal_offset {
        let windows = history
            .read(SessionHistoryReadParams {
                session_id: scope.session_id.clone(),
                window: SessionHistoryWindow::OldestAfter {
                    completed_event_seq: after,
                },
                limit: 1,
                message_limit: 3,
            })
            .await
            .unwrap();
        let window = &windows[0];
        assert_eq!(window.turn.completed_event_seq, terminal_offset);
        assert!(window.covered_until_event_seq > after);
        assert!(window
            .message_event_sequences
            .iter()
            .all(|(_, sequence)| *sequence > after));
        consumed.extend(
            window
                .message_event_sequences
                .iter()
                .map(|(id, _)| id.clone()),
        );
        history
            .advance_processed_until_event_seq(
                &scope.session_id,
                after as u64,
                window.covered_until_event_seq as u64,
            )
            .await
            .unwrap();
        after = window.covered_until_event_seq;
    }
    let mut expected = vec!["message-0".to_owned(), "tool-message-0".to_owned()];
    expected.extend((0..44).map(|index| format!("large-message-{index}")));
    assert_eq!(consumed, expected);
    assert_eq!(
        history
            .processed_until_event_seq(&scope.session_id)
            .await
            .unwrap(),
        terminal_offset as u64
    );
    assert!(history
        .read(SessionHistoryReadParams {
            session_id: scope.session_id,
            window: SessionHistoryWindow::OldestAfter {
                completed_event_seq: terminal_offset
            },
            limit: 1,
            message_limit: 3
        })
        .await
        .unwrap()
        .is_empty());
}

#[path = "execution_history_budget_tests.rs"]
mod budget_tests;
