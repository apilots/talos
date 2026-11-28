use super::*;
use agent_protocol::{
    AgentExecutionCommand, AgentExecutionSettlement, AgentInputParams, AgentRunCheckpoint,
    AgentSessionEventsParams,
};
use talos_agent_api::{
    AgentEvent, AgentEventPayload, AgentMessage, AgentMessageRole, AgentMessageStatus,
    AgentRunMetrics, AgentTurnRef, AgentTurnStarted,
};

fn session() -> AgentSessionContext {
    AgentSessionContext {
        agent_id: "agent".into(),
        session_id: "session".into(),
        workspace: talos_config::EffectiveWorkspace {
            workspace_id: "workspace".into(),
            user_id: "alice".into(),
            root: "/tmp/workspace/alice".into(),
        },
    }
}
fn event(sequence: u64, payload: AgentEventPayload) -> AgentEvent {
    AgentEvent {
        identity: AgentTurnRef::new("session", "turn"),
        sequence,
        occurred_at: 1,
        payload,
    }
}
fn command() -> AgentExecutionCommand {
    AgentExecutionCommand::Input {
        params: Box::new(AgentInputParams {
            command: None,
            agent_id: "agent".into(),
            identity: AgentTurnRef::new("session", "turn"),
            input: AgentMessage {
                message_id: "message".into(),
                turn_id: "turn".into(),
                role: AgentMessageRole::User,
                status: AgentMessageStatus::Completed,
                blocks: vec![],
                created_at: 1,
                completed_at: Some(1),
            },
        }),
    }
}
fn checkpoint(sequence: u64) -> AgentRunCheckpoint {
    AgentRunCheckpoint {
        last_sequence: sequence,
        messages: vec![],
        metrics: AgentRunMetrics::default(),
    }
}
async fn admit(store: &WorkerExecutionStore) {
    store
        .admit(
            &session(),
            &command(),
            vec![event(
                1,
                AgentEventPayload::TurnStarted(AgentTurnStarted {}),
            )],
            &checkpoint(1),
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn execution_survives_restart_with_only_execution_domain_tables() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("execution.sqlite");
    let store = WorkerExecutionStore::open(&path).await.unwrap();
    admit(&store).await;
    store.pool.close().await;
    let reopened = WorkerExecutionStore::open(&path).await.unwrap();
    let result = reopened
        .read_events(AgentSessionEventsParams {
            session: session(),
            after_cursor: None,
            limit: 10,
        })
        .await
        .unwrap();
    assert_eq!(result.events.len(), 1);
    assert_eq!(
        result.active_turn,
        Some(AgentTurnRef::new("session", "turn"))
    );
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name <> '_sqlx_migrations' ORDER BY name",
    )
    .fetch_all(&reopened.pool)
    .await
    .unwrap();
    assert_eq!(
        tables,
        [
            "agent_events",
            "dream_formations",
            "session_history_supplements",
            "sessions",
            "turn_admissions",
            "turn_checkpoints",
        ]
    );
}

#[tokio::test]
async fn retiring_an_already_cleared_source_preserves_its_generation_and_fence() {
    let directory = tempfile::tempdir().unwrap();
    let store = WorkerExecutionStore::open(&directory.path().join("worker.sqlite"))
        .await
        .unwrap();
    admit(&store).await;
    store
        .append(
            &session(),
            vec![event(
                2,
                AgentEventPayload::TurnCompleted(talos_agent_api::AgentTurnCompleted {
                    stop_reason: talos_agent_api::AgentStopReason::EndTurn,
                }),
            )],
            &checkpoint(2),
            Some(AgentExecutionSettlement::Finished {
                memory_usage: vec![],
                metrics: AgentRunMetrics::default(),
            }),
        )
        .await
        .unwrap();
    store.clear(&session()).await.unwrap();
    store.retire(&session()).await.unwrap();
    store.retire(&session()).await.unwrap();
    let state = store.execution_state(&session()).await.unwrap().unwrap();
    assert_eq!(state.history_cursor.history_generation, 1);
    assert!(store.is_retired(&session().session_id).await.unwrap());
    assert!(store
        .admit(
            &session(),
            &command(),
            vec![event(
                1,
                AgentEventPayload::TurnStarted(AgentTurnStarted {})
            )],
            &checkpoint(1),
        )
        .await
        .unwrap_err()
        .is_code(ErrorCode::SessionNotFound));
}

#[tokio::test]
async fn clearing_cannot_reopen_a_retired_source() {
    let directory = tempfile::tempdir().unwrap();
    let store = WorkerExecutionStore::open(&directory.path().join("worker.sqlite"))
        .await
        .unwrap();
    store.retire(&session()).await.unwrap();
    assert!(store
        .clear(&session())
        .await
        .unwrap_err()
        .is_code(ErrorCode::SessionNotFound));
    assert_eq!(
        store
            .execution_state(&session())
            .await
            .unwrap()
            .unwrap()
            .history_cursor
            .history_generation,
        1
    );
    assert!(store.is_retired(&session().session_id).await.unwrap());
    assert!(store
        .admit(
            &session(),
            &command(),
            vec![event(
                1,
                AgentEventPayload::TurnStarted(AgentTurnStarted {})
            )],
            &checkpoint(1),
        )
        .await
        .unwrap_err()
        .is_code(ErrorCode::SessionNotFound));
}

#[tokio::test]
async fn cursor_and_binding_cannot_read_another_user_or_generation() {
    let directory = tempfile::tempdir().unwrap();
    let store = WorkerExecutionStore::open(&directory.path().join("execution.sqlite"))
        .await
        .unwrap();
    admit(&store).await;
    let mut other = session();
    other.workspace.user_id = "bob".into();
    let failure = store
        .read_events(AgentSessionEventsParams {
            session: other,
            after_cursor: None,
            limit: 1,
        })
        .await
        .unwrap_err();
    assert!(failure.is_code(ErrorCode::PermissionDenied));
    let failure = store
        .read_events(AgentSessionEventsParams {
            session: session(),
            after_cursor: Some(cursor(&session(), 2, 0)),
            limit: 1,
        })
        .await
        .unwrap_err();
    assert!(failure.is_code(ErrorCode::EventCursorExpired));
}

#[tokio::test]
async fn idempotent_input_rejects_changed_payload_without_writing_more_events() {
    let directory = tempfile::tempdir().unwrap();
    let store = WorkerExecutionStore::open(&directory.path().join("execution.sqlite"))
        .await
        .unwrap();
    admit(&store).await;
    assert!(store
        .admission(
            &session(),
            &AgentTurnRef::new("session", "turn"),
            &command()
        )
        .await
        .unwrap()
        .is_some());
    let mut changed = command();
    if let AgentExecutionCommand::Input { params } = &mut changed {
        params.input.created_at = 2;
    }
    let failure = store
        .admission(&session(), &AgentTurnRef::new("session", "turn"), &changed)
        .await
        .unwrap_err();
    assert!(failure.is_code(ErrorCode::IdempotencyConflict));
    assert_eq!(
        store
            .turn_events(&AgentTurnRef::new("session", "turn"))
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn oversized_fact_rolls_back_and_memory_progress_advances_atomically() {
    let directory = tempfile::tempdir().unwrap();
    let store = WorkerExecutionStore::open(&directory.path().join("worker.sqlite"))
        .await
        .unwrap();
    admit(&store).await;
    let mut huge = match command() {
        AgentExecutionCommand::Input { params } => params.input,
        _ => unreachable!(),
    };
    huge.message_id = "x".repeat(agent_protocol::MAX_EXECUTION_EVENT_BYTES);
    let failure = store
        .append(
            &session(),
            vec![event(2, AgentEventPayload::MessageCompleted(huge))],
            &checkpoint(2),
            None,
        )
        .await
        .unwrap_err();
    assert!(failure.is_code(ErrorCode::InvalidRequest));
    let page = store
        .read_events(AgentSessionEventsParams {
            session: session(),
            after_cursor: None,
            limit: 256,
        })
        .await
        .unwrap();
    assert_eq!(page.events.len(), 1);
    store
        .advance_memory_progress("session", 0, page.next_cursor.offset as u64)
        .await
        .unwrap();
    assert_eq!(
        store.memory_progress("session").await.unwrap(),
        page.next_cursor.offset as u64
    );
    assert!(store
        .advance_memory_progress("session", 0, page.next_cursor.offset as u64)
        .await
        .is_err());
}

#[tokio::test]
async fn replay_paginates_without_skipping_global_offsets_or_repeating_facts() {
    let directory = tempfile::tempdir().unwrap();
    let store = WorkerExecutionStore::open(&directory.path().join("worker.sqlite"))
        .await
        .unwrap();
    admit(&store).await;
    for sequence in 2..7 {
        store
            .append(
                &session(),
                vec![event(
                    sequence,
                    AgentEventPayload::MetricsUpdated(AgentRunMetrics::default()),
                )],
                &checkpoint(sequence),
                None,
            )
            .await
            .unwrap();
    }
    let mut cursor = None;
    let mut offsets = Vec::new();
    loop {
        let page = store
            .read_events(AgentSessionEventsParams {
                session: session(),
                after_cursor: cursor,
                limit: 2,
            })
            .await
            .unwrap();
        offsets.extend(page.events.iter().map(|fact| fact.cursor.offset));
        cursor = Some(page.next_cursor);
        if !page.has_more {
            break;
        }
    }
    assert_eq!(offsets.len(), 6);
    assert!(offsets.windows(2).all(|pair| pair[0] < pair[1]));
}

fn history_supplement() -> (
    talos_agent_api::trajectory::SessionHistorySupplement,
    talos_agent_api::trajectory::TrajectoryPayload,
) {
    use talos_agent_api::trajectory::{
        ClockRelation, ClockSegment, SessionHistorySupplement, TrajectoryPayload,
    };
    let mut supplement = SessionHistorySupplement {
        identity: AgentTurnRef::new("session", "turn"),
        clock_segments: vec![ClockSegment {
            segment_id: "clock".into(),
            source_node_id: "embedded".into(),
            started_at: 1,
            relation: ClockRelation::TurnStart,
            observed_through_offset_ms: 1.0,
        }],
        requests: vec![],
        tools: vec![],
        payloads: Default::default(),
    };
    let payload = TrajectoryPayload::Prompt {
        messages: vec![talos_agent_api::trajectory::SnapshotMessage {
            index: 0,
            role: talos_agent_api::trajectory::SnapshotRole::User,
            content: "unicode sample \u{4e2d}\u{6587}🙂".repeat(1024),
            reasoning: None,
            name: None,
            tool_call_id: None,
            tool_calls: None,
            attachments: None,
            cache_control: None,
            input_origin: None,
        }],
    };
    supplement.requests.push(
        talos_agent_api::trajectory::TrajectoryReadModelRequestResponse {
            identity: supplement.identity.clone(),
            request: talos_agent_api::trajectory::ModelRequestTrace {
                model_request_id: "request".into(),
                ordinal: 1,
                purpose: talos_agent_api::trajectory::ModelRequestPurpose::Working,
                connection_id: "main".into(),
                model_id: "model".into(),
                call_records: vec![],
            },
            prompt: Some(talos_agent_api::trajectory::PayloadRef {
                payload_id: "payload".into(),
                complete: Some(true),
            }),
            tools: None,
            parameters: None,
            outputs: vec![],
        },
    );
    supplement
        .payloads
        .insert("payload".into(), payload.clone());
    (supplement, payload)
}

#[tokio::test]
async fn sealed_capture_keeps_clock_metadata_and_pages_source_payload_without_product_history() {
    use talos_agent_api::trajectory::TrajectoryPayload;
    let directory = tempfile::tempdir().unwrap();
    let store = WorkerExecutionStore::open(&directory.path().join("worker.sqlite"))
        .await
        .unwrap();
    admit(&store).await;
    let (mut supplement, payload) = history_supplement();
    store.append_supplement(supplement.clone()).await.unwrap();
    supplement.clock_segments[0].observed_through_offset_ms = 5.0;
    store.append_supplement(supplement).await.unwrap();
    store
        .append(
            &session(),
            vec![event(
                2,
                AgentEventPayload::TurnFailed(talos_agent_api::AgentTurnFailed {
                    error: TalosError::new(ErrorCode::Cancelled, "Cancelled.").to_api_error(),
                    retryable: false,
                }),
            )],
            &checkpoint(2),
            Some(agent_protocol::AgentExecutionSettlement::Finished {
                memory_usage: vec![],
                metrics: Default::default(),
            }),
        )
        .await
        .unwrap();
    let page = store
        .read_events(AgentSessionEventsParams {
            session: session(),
            after_cursor: None,
            limit: 256,
        })
        .await
        .unwrap();
    let capture = page
        .events
        .last()
        .unwrap()
        .history_supplement
        .as_ref()
        .unwrap();
    assert_eq!(capture.clock_segments.len(), 1);
    assert_eq!(capture.clock_segments[0].observed_through_offset_ms, 5.0);
    assert!(capture.payloads.is_empty());
    let mut offset = 0;
    let mut body = String::new();
    loop {
        let chunk = store
            .read_payload(agent_protocol::AgentExecutionPayloadParams {
                session: session(),
                identity: AgentTurnRef::new("session", "turn"),
                payload_id: "payload".into(),
                offset,
                limit: 17,
            })
            .await
            .unwrap();
        assert!(chunk.data.len() <= 17);
        body.push_str(&chunk.data);
        match chunk.next_offset {
            Some(next) => {
                assert!(next > offset);
                offset = next;
            }
            None => break,
        }
    }
    assert_eq!(
        serde_json::from_str::<TrajectoryPayload>(&body).unwrap(),
        payload
    );
    let mut other = session();
    other.workspace.user_id = "bob".into();
    assert!(store
        .read_payload(agent_protocol::AgentExecutionPayloadParams {
            session: other,
            identity: AgentTurnRef::new("session", "turn"),
            payload_id: "payload".into(),
            offset: 0,
            limit: 100
        })
        .await
        .unwrap_err()
        .is_code(ErrorCode::PermissionDenied));
    store.clear(&session()).await.unwrap();
    assert!(store
        .read_payload(agent_protocol::AgentExecutionPayloadParams {
            session: session(),
            identity: AgentTurnRef::new("session", "turn"),
            payload_id: "payload".into(),
            offset: 0,
            limit: 100
        })
        .await
        .unwrap_err()
        .is_code(ErrorCode::ResourceNotFound));
}

#[tokio::test]
async fn replay_of_more_than_four_mib_keeps_each_fact_and_page_within_wire_budgets() {
    let directory = tempfile::tempdir().unwrap();
    let store = WorkerExecutionStore::open(&directory.path().join("worker.sqlite"))
        .await
        .unwrap();
    admit(&store).await;
    for sequence in 2..20 {
        let message = AgentMessage {
            message_id: format!("message-{sequence}"),
            turn_id: "turn".into(),
            role: AgentMessageRole::User,
            status: AgentMessageStatus::Completed,
            blocks: vec![talos_agent_api::AgentMessageBlock {
                block_id: format!("block-{sequence}"),
                status: talos_agent_api::AgentMessageBlockStatus::Completed,
                content: talos_agent_api::AgentMessageBlockContent::UserContent(
                    talos_agent_api::AgentUserContent::Text {
                        text: "x".repeat(300_000),
                    },
                ),
                started_at: 1,
                completed_at: Some(1),
            }],
            created_at: 1,
            completed_at: Some(1),
        };
        store
            .append(
                &session(),
                vec![event(
                    sequence,
                    AgentEventPayload::MessageCompleted(message),
                )],
                &checkpoint(sequence),
                None,
            )
            .await
            .unwrap();
    }
    let mut cursor = None;
    let mut bytes = 0;
    let mut count = 0;
    let mut pages = 0;
    loop {
        let page = store
            .read_events(AgentSessionEventsParams {
                session: session(),
                after_cursor: cursor,
                limit: 256,
            })
            .await
            .unwrap();
        assert!(
            serde_json::to_vec(&page).unwrap().len() <= agent_protocol::MAX_EXECUTION_PAGE_BYTES
        );
        for fact in &page.events {
            let size = serde_json::to_vec(fact).unwrap().len();
            assert!(size <= agent_protocol::MAX_EXECUTION_EVENT_BYTES);
            bytes += size;
            count += 1;
        }
        cursor = Some(page.next_cursor);
        pages += 1;
        if !page.has_more {
            break;
        }
    }
    assert_eq!(count, 19);
    assert!(bytes > 4 * 1024 * 1024);
    assert!(pages >= 3);
}

#[tokio::test]
async fn overflowing_message_snapshot_keeps_last_safe_checkpoint_for_terminal_failure() {
    let directory = tempfile::tempdir().unwrap();
    let store = WorkerExecutionStore::open(&directory.path().join("worker.sqlite"))
        .await
        .unwrap();
    admit(&store).await;
    let mut overflow = checkpoint(2);
    let AgentExecutionCommand::Input { params } = command() else {
        unreachable!()
    };
    let mut message = params.input;
    message.blocks = vec![talos_agent_api::AgentMessageBlock {
        block_id: "large".into(),
        status: talos_agent_api::AgentMessageBlockStatus::Running,
        content: talos_agent_api::AgentMessageBlockContent::AssistantText(
            talos_agent_api::AgentAssistantText {
                text: "x".repeat(agent_protocol::MAX_EXECUTION_EVENT_BYTES),
                phase: talos_agent_api::AgentAssistantTextPhase::FinalAnswer,
            },
        ),
        started_at: 1,
        completed_at: None,
    }];
    overflow.messages.push(message);
    let error = store
        .append(
            &session(),
            vec![event(
                2,
                AgentEventPayload::MetricsUpdated(Default::default()),
            )],
            &overflow,
            None,
        )
        .await
        .unwrap_err();
    assert!(error.is_code(ErrorCode::SessionBudgetExceeded));
    assert_eq!(
        store
            .checkpoint(&AgentTurnRef::new("session", "turn"))
            .await
            .unwrap()
            .unwrap()
            .last_sequence,
        1
    );
    store
        .append(
            &session(),
            vec![event(
                2,
                AgentEventPayload::TurnFailed(talos_agent_api::AgentTurnFailed {
                    error: error.to_api_error(),
                    retryable: false,
                }),
            )],
            &checkpoint(2),
            Some(agent_protocol::AgentExecutionSettlement::Finished {
                memory_usage: vec![],
                metrics: Default::default(),
            }),
        )
        .await
        .unwrap();
    assert!(store
        .execution_state(&session())
        .await
        .unwrap()
        .unwrap()
        .active_turn
        .is_none());
}

#[tokio::test]
async fn waiting_private_reference_requires_its_exact_committed_fact_and_survives_restart() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("execution.sqlite");
    let store = WorkerExecutionStore::open(&path).await.unwrap();
    admit(&store).await;
    let waiting = event(
        2,
        AgentEventPayload::WaitingForInput(talos_agent_api::AgentPendingInput::Message {
            action_id: "action".into(),
            prompt: "Continue?".into(),
        }),
    );
    let reference = talos_agent_api::AgentContinuationRef {
        opaque_id: "private-worker-reference".into(),
    };
    let settlement = AgentExecutionSettlement::WaitingForInput {
        metrics: AgentRunMetrics::default(),
    };
    assert!(store
        .append(
            &session(),
            vec![waiting.clone()],
            &checkpoint(2),
            Some(settlement.clone())
        )
        .await
        .unwrap_err()
        .is_code(ErrorCode::Conflict));
    assert_eq!(
        store.checkpoint(&waiting.identity).await.unwrap(),
        Some(checkpoint(1))
    );
    store
        .record_waiting_continuation(&session(), &waiting.identity, 2, &reference)
        .await
        .unwrap();
    assert!(store
        .waiting_continuation(&session(), &waiting.identity, 2)
        .await
        .unwrap_err()
        .is_code(ErrorCode::Conflict));
    let changed = talos_agent_api::AgentContinuationRef {
        opaque_id: "changed-private-reference".into(),
    };
    assert!(store
        .record_waiting_continuation(&session(), &waiting.identity, 2, &changed)
        .await
        .unwrap_err()
        .is_code(ErrorCode::Conflict));
    let mut wrong_sequence = waiting.clone();
    wrong_sequence.sequence = 3;
    assert!(store
        .append(
            &session(),
            vec![wrong_sequence],
            &checkpoint(3),
            Some(settlement.clone())
        )
        .await
        .unwrap_err()
        .is_code(ErrorCode::Conflict));
    store
        .append(
            &session(),
            vec![waiting.clone()],
            &checkpoint(2),
            Some(settlement),
        )
        .await
        .unwrap();
    store.pool.close().await;
    let store = WorkerExecutionStore::open(&path).await.unwrap();
    assert_eq!(
        store
            .waiting_continuation(&session(), &waiting.identity, 2)
            .await
            .unwrap(),
        reference
    );
    assert!(store
        .waiting_continuation(&session(), &waiting.identity, 3)
        .await
        .unwrap_err()
        .is_code(ErrorCode::Conflict));
    let page = store
        .read_events(AgentSessionEventsParams {
            session: session(),
            after_cursor: None,
            limit: 10,
        })
        .await
        .unwrap();
    let public_json = serde_json::to_string(&page).unwrap();
    assert!(!public_json.contains("private-worker-reference"));
    assert!(!public_json.contains("continuation"));
    let terminal = event(
        3,
        AgentEventPayload::TurnCompleted(talos_agent_api::AgentTurnCompleted {
            stop_reason: talos_agent_api::AgentStopReason::EndTurn,
        }),
    );
    store
        .append(
            &session(),
            vec![terminal],
            &checkpoint(3),
            Some(AgentExecutionSettlement::Finished {
                metrics: AgentRunMetrics::default(),
                memory_usage: vec![],
            }),
        )
        .await
        .unwrap();
    let private: (Option<i64>, Option<String>) = sqlx::query_as("SELECT waiting_sequence,provider_continuation_json FROM turn_checkpoints WHERE session_id='session' AND generation=0 AND turn_id='turn'").fetch_one(&store.pool).await.unwrap();
    assert_eq!(private, (None, None));
    assert!(store
        .waiting_continuation(&session(), &waiting.identity, 2)
        .await
        .unwrap_err()
        .is_code(ErrorCode::Conflict));
}

#[path = "migration_tests.rs"]
mod migration;

#[path = "history_window_tests.rs"]
mod history_window;

#[path = "dream_tests.rs"]
mod dream;
