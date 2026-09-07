use super::*;
use opendal::{services::Memory, Operator};
use std::collections::BTreeMap;
use std::path::Path;
use talos_agent::{
    AgentAdmission, AgentAdmissionPermit, AgentEventPipeline, AgentProgressEventReceiver,
    AgentRunExecution, AgentRunParams, AgentRunSettlement, AgentRunSettlementReceiver,
};
use talos_agent_api::{
    AgentAssistantText, AgentAssistantTextPhase, AgentEventPayload as CanonicalAgentEventPayload,
    AgentMessage, AgentMessageBlockContent, AgentMessageBlockDeltaContent, AgentMessageBlockStatus,
    AgentMessageRole, AgentMessageStatus, AgentPendingInput, AgentRunMetrics, AgentStopReason,
    AgentToolCall, AgentToolCallArguments, AgentToolCallArgumentsDelta, AgentToolProgress,
    AgentTurnCompleted, AgentTurnRef, AgentTurnStarted,
};
use talos_service::FileService;
use talos_store::file::DatabaseFileStore;
use talos_store::migration::run_migrations;
use talos_store::project::ProjectStore;
use talos_store::session::{
    AgentExecutionStore, DatabaseSessionStore, SessionRecord, SessionStore, SessionTreeIdentity,
};
use talos_types::talos::{SessionSourceProvider, SessionSourceType, TalosToolProgressKind};

pub(crate) fn test_config_reload_hook(
    config: talos_types::TalosConfig,
) -> talos_service::ConfigReloadHook {
    let active = Arc::new(std::sync::RwLock::new(config));
    Arc::new(move |trigger| {
        let mut next = active
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        if let talos_service::ConfigReloadTrigger::Updated(value) = trigger {
            match value {
                talos_types::ConfigValue::Llm(llm) => next.llm = *llm,
                talos_types::ConfigValue::WebSearch(web_search) => {
                    next.agent.kernel.tools.web_search = *web_search;
                }
            }
        }
        let active = active.clone();
        Ok(talos_service::ConfigReloadPlan::new(
            next.clone(),
            move || {
                *active
                    .write()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) = next;
            },
        ))
    })
}

pub(crate) async fn test_file_service(
    projects: std::sync::Arc<dyn ProjectStore>,
    config: talos_config::FileStorageConfig,
) -> std::sync::Arc<FileService> {
    let storage = talos_storage::SqliteStorage::new_with_max_connections(":memory:", 1)
        .await
        .unwrap();
    let pool = storage.pool().clone();
    let migration_dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../conf/migrations/sqlite");
    run_migrations(&pool, &migration_dir).await.unwrap();
    let operator = Operator::new(Memory::default()).unwrap().finish();
    std::sync::Arc::new(
        FileService::new(
            std::sync::Arc::new(DatabaseFileStore::new(pool)),
            projects,
            BTreeMap::from([("local".to_string(), operator)]),
            config,
        )
        .unwrap(),
    )
}

pub(crate) async fn test_agent_execution_store() -> Arc<DatabaseSessionStore> {
    let storage = Arc::new(talos_storage::SqliteStorage::new(":memory:").await.unwrap());
    let migration_dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../conf/migrations/sqlite");
    run_migrations(storage.pool(), &migration_dir)
        .await
        .unwrap();
    Arc::new(DatabaseSessionStore::new(storage).await.unwrap())
}

pub(crate) async fn test_session_store() -> Arc<DatabaseSessionStore> {
    let store = test_agent_execution_store().await;
    store
        .insert_session_record(&SessionRecord {
            id: "mock-session".to_string(),
            agent_id: "default".to_string(),
            tree_identity: SessionTreeIdentity::Primary,
            workspace_id: "default".to_string(),
            title: "Untitled Session".to_string(),
            user_id: "static-token".to_string(),
            source_type: SessionSourceType::Http,
            source_provider: SessionSourceProvider::Http,
            channel_id: Some("test-channel".to_string()),
            thread_id: None,
            environment: None,
            created_at: 0,
            last_activity: 0,
            message_count: 1,
            latest_turn_seq: 0,
            memory_processed_until_event_seq: 0,
            usage: talos_types::SessionUsage {
                tokens: TokenUsage {
                    input_tokens: 100,
                    output_tokens: 20,
                    total_tokens: 120,
                    ..TokenUsage::default()
                },
                context: talos_types::ContextUsage::new(80, 128_000, 120_000, 3),
            },
            conf: talos_types::SessionConf {
                model_selection: Some(talos_types::ModelSelection::without_reasoning(
                    talos_types::ModelRef {
                        provider: "bailian".to_string(),
                        id: "qwen-plus".to_string(),
                    },
                )),
                ..talos_types::SessionConf::default()
            },
            project_id: None,
            current_directory: None,
        })
        .await
        .unwrap();
    store
}

pub(crate) async fn seed_running_agent_execution(
    store: &dyn AgentExecutionStore,
    session_id: &str,
    turn_id: &str,
) {
    let identity = AgentTurnRef::new(session_id, turn_id);
    let (pipeline, _sink, _progress) = AgentEventPipeline::fresh(identity);
    let mut admission = vec![pipeline
        .record(CanonicalAgentEventPayload::TurnStarted(AgentTurnStarted {}))
        .await
        .expect("test TurnStarted should be valid")];
    admission.extend(
        pipeline
            .record_user_message(AgentMessage {
                message_id: "message-initial".to_string(),
                turn_id: turn_id.to_string(),
                role: AgentMessageRole::User,
                status: AgentMessageStatus::Completed,
                blocks: Vec::new(),
                created_at: 1,
                completed_at: Some(1),
            })
            .await
            .expect("test initial User message should be valid"),
    );
    store
        .commit_initial_admission(talos_store::session::CommitInitialAdmissionParams {
            events: admission,
            command: None,
        })
        .await
        .expect("test initial admission should persist");
}

pub(crate) async fn seed_waiting_agent_execution(
    store: &dyn AgentExecutionStore,
    session_id: &str,
    turn_id: &str,
    action: PendingUserAction,
) {
    let identity = AgentTurnRef::new(session_id, turn_id);
    let (pipeline, sink, mut progress) = AgentEventPipeline::fresh(identity.clone());
    let mut admission = vec![pipeline
        .record(CanonicalAgentEventPayload::TurnStarted(AgentTurnStarted {}))
        .await
        .expect("test TurnStarted should be valid")];
    admission.extend(
        pipeline
            .record_user_message(AgentMessage {
                message_id: "message-initial".to_string(),
                turn_id: turn_id.to_string(),
                role: AgentMessageRole::User,
                status: AgentMessageStatus::Completed,
                blocks: Vec::new(),
                created_at: 1,
                completed_at: Some(1),
            })
            .await
            .expect("test initial User message should be valid"),
    );
    let initial = store
        .commit_initial_admission(talos_store::session::CommitInitialAdmissionParams {
            events: admission,
            command: None,
        })
        .await
        .expect("test initial admission should persist");
    let message = sink
        .start_message(AgentMessageRole::Agent)
        .await
        .expect("test Provider message should start");
    let message_started = progress
        .recv()
        .await
        .expect("test Provider message event should be emitted");
    let message_commit = store
        .commit_progress_event(message_started, initial.active_turn.last_event_seq)
        .await
        .expect("test Provider message should persist");
    let tool = sink
        .start_block(
            &message,
            AgentMessageBlockContent::ToolCall(AgentToolCall {
                call_id: action.call_id.clone(),
                name: "Tool".to_string(),
                arguments: AgentToolCallArguments::Streaming {
                    json_fragment: "{}".to_string(),
                },
                progress: Vec::new(),
            }),
        )
        .await
        .expect("test Tool block should start");
    let tool_started = progress
        .recv()
        .await
        .expect("test Tool block event should be emitted");
    let tool_commit = store
        .commit_progress_event(tool_started, message_commit.active_turn.last_event_seq)
        .await
        .expect("test Tool block should persist");
    let mut action = action;
    action.item_id = Some(tool.block_id().to_string());
    let waiting = pipeline
        .record(CanonicalAgentEventPayload::WaitingForInput(
            AgentPendingInput::UserAction { action },
        ))
        .await
        .expect("test waiting event should be valid");
    store
        .commit_waiting_settlement(
            waiting,
            talos_agent_api::AgentContinuationRef {
                opaque_id: "continuation-test".to_string(),
            },
            tool_commit.active_turn.last_event_seq,
        )
        .await
        .expect("test waiting settlement should persist");
}

pub(crate) async fn mock_agent_execution(params: AgentRunParams) -> TalosResult<AgentRunExecution> {
    let identity = params.context.identity();
    let resumed_lifecycles = params
        .checkpoint
        .as_ref()
        .map(|checkpoint| {
            checkpoint
                .messages
                .iter()
                .filter(|message| message.status == AgentMessageStatus::InProgress)
                .map(|message| {
                    (
                        message.message_id.clone(),
                        message
                            .blocks
                            .iter()
                            .filter(|block| {
                                matches!(
                                    block.status,
                                    AgentMessageBlockStatus::Running
                                        | AgentMessageBlockStatus::WaitingForInput
                                )
                            })
                            .map(|block| block.block_id.clone())
                            .collect::<Vec<_>>(),
                    )
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let (pipeline, sink, progress, mut admission_events) = match params.checkpoint {
        Some(checkpoint) => {
            let (pipeline, sink, progress) = AgentEventPipeline::resumed(identity, checkpoint)?;
            (pipeline, sink, progress, Vec::new())
        }
        None => {
            let (pipeline, sink, progress) = AgentEventPipeline::fresh(identity);
            let started = pipeline
                .record(CanonicalAgentEventPayload::TurnStarted(AgentTurnStarted {}))
                .await?;
            (pipeline, sink, progress, vec![started])
        }
    };
    admission_events.extend(pipeline.record_user_message(params.input).await?);
    let (settlement_tx, settlement_rx) = tokio::sync::oneshot::channel();
    let settlement_pipeline = pipeline.clone();
    let permit = AgentAdmissionPermit::new(async move {
        for (message_id, block_ids) in resumed_lifecycles {
            let resumed_message = sink.resume_message(&message_id)?;
            for block_id in block_ids {
                let resumed_block = sink.resume_block(&message_id, &block_id)?;
                sink.complete_block(&resumed_block, AgentMessageBlockStatus::Completed)
                    .await?;
            }
            sink.complete_message(&resumed_message, AgentMessageStatus::Completed)
                .await?;
        }
        let message = sink.start_message(AgentMessageRole::Agent).await?;
        let assistant = sink
            .start_block(
                &message,
                AgentMessageBlockContent::AssistantText(AgentAssistantText {
                    phase: AgentAssistantTextPhase::FinalAnswer,
                    text: String::new(),
                }),
            )
            .await?;
        sink.append_block_delta(
            &assistant,
            AgentMessageBlockDeltaContent::Text("stream only content".to_string()),
        )
        .await?;
        sink.complete_block(&assistant, AgentMessageBlockStatus::Completed)
            .await?;
        let tool = sink
            .start_block(
                &message,
                AgentMessageBlockContent::ToolCall(AgentToolCall {
                    call_id: "call-1".to_string(),
                    name: "Bash".to_string(),
                    arguments: AgentToolCallArguments::Streaming {
                        json_fragment: String::new(),
                    },
                    progress: Vec::new(),
                }),
            )
            .await?;
        sink.append_block_delta(
            &tool,
            AgentMessageBlockDeltaContent::ToolCallArguments(AgentToolCallArgumentsDelta {
                json_fragment: "{}".to_string(),
            }),
        )
        .await?;
        sink.append_block_delta(
            &tool,
            AgentMessageBlockDeltaContent::ToolProgress(AgentToolProgress {
                kind: TalosToolProgressKind::Status,
                text: "execution started".to_string(),
                occurred_at: chrono::Utc::now().timestamp(),
            }),
        )
        .await?;
        sink.complete_block(&tool, AgentMessageBlockStatus::Completed)
            .await?;
        sink.complete_message(&message, AgentMessageStatus::Completed)
            .await?;
        sink.update_metrics(AgentRunMetrics::default()).await?;
        let terminal = settlement_pipeline
            .record(CanonicalAgentEventPayload::TurnCompleted(
                AgentTurnCompleted {
                    stop_reason: AgentStopReason::EndTurn,
                },
            ))
            .await?;
        let checkpoint = settlement_pipeline.checkpoint().await;
        settlement_tx
            .send(Ok(AgentRunSettlement::Finished {
                result: Ok(()),
                event: terminal,
                messages: checkpoint.messages,
                metrics: checkpoint.metrics,
            }))
            .map_err(|_| {
                talos_error::TalosError::new(
                    talos_error::ErrorCode::InternalError,
                    "test settlement receiver",
                )
            })?;
        Ok(())
    });
    Ok(AgentRunExecution {
        admission: AgentAdmission {
            events: admission_events,
            permit,
        },
        events: AgentProgressEventReceiver::new(progress),
        settlement: AgentRunSettlementReceiver::new(settlement_rx),
    })
}
