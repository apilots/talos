use super::*;
use crate::access::CreateSessionRequest;
use crate::test_support::run_test_migrations;
use async_trait::async_trait;
use bytes::Bytes;
use futures_util::stream;
use opendal::{services::Memory, Operator};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use talos_agent::{
    AgentAdmission, AgentAdmissionPermit, AgentContext, AgentInputParams, AgentInterruptParams,
    AgentProgressEventReceiver, AgentRunExecution, AgentRunParams, AgentRunSettlement,
    AgentRunSettlementReceiver, AgentSessionContext,
};
use talos_agent_api::{
    AgentContextCompaction, AgentContextCompactionDelta, AgentEvent as CanonicalAgentEvent,
    AgentEventPayload as CanonicalAgentEventPayload, AgentInterruptOutcome, AgentMessage,
    AgentMessageBlock, AgentMessageBlockCompleted, AgentMessageBlockContent,
    AgentMessageBlockDelta, AgentMessageBlockDeltaContent, AgentMessageBlockStarted,
    AgentMessageBlockStatus, AgentMessageRole, AgentMessageStarted, AgentMessageStatus,
    AgentRunMetrics, AgentTurnCompleted, AgentTurnRef, AgentTurnStarted,
};
use talos_auth::{AuthService, RequestIdentity, UserDto, UserService};
use talos_coordinator::{AgentCoordinator, AgentSelectionParams};
use talos_error::{TalosError, TalosResult};
use talos_storage::SqliteStorage;
use talos_store::file::DatabaseFileStore;
use talos_store::profile::{DatabaseProfileStore, ProfileStore};
use talos_store::project::StorageProjectStore;
use talos_store::session::{
    AccumulateSessionUsageParams, AgentExecutionStore, CommandReservation, DatabaseSessionStore,
    IdempotencyReservation, IdempotentCommand, SessionStore,
};
use talos_types::talos::{
    ContextCompactionOutcome, ContextCompactionStage, ContextCompactionTrigger, SessionSource,
    TokenUsage,
};

#[derive(Clone)]
enum TestAgentExecutionMode {
    Accepted,
    AdmissionBlocked {
        reached: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
    },
    StartTimeout,
    StartFailed,
    WorkerStateBlocked,
    WorkerStateUnavailable,
    WorkerStateActive,
    WorkerClearBlocked,
}

#[path = "first_turn_rollback_tests.rs"]
mod first_turn_rollback_tests;
#[path = "project_lifecycle_tests.rs"]
mod project_lifecycle_tests;
#[path = "session_patch_tests.rs"]
mod session_patch_tests;
#[path = "turn_interrupt_tests.rs"]
mod turn_interrupt_tests;

#[path = "admission_recovery_tests.rs"]
mod admission_recovery_tests;
#[path = "compaction_tests.rs"]
mod compaction_tests;
#[path = "interrupt_queue_tests.rs"]
mod interrupt_queue_tests;
#[path = "maintenance_tests.rs"]
mod maintenance_tests;

struct TestAgentExecution {
    mode: TestAgentExecutionMode,
}

struct TestExecutionCoordinator {
    execution: Arc<TestAgentExecution>,
    delegate: Arc<dyn AgentCoordinator>,
}

struct TestAgentCoordinator {
    cancel_mode: TestCancelMode,
    cancelled_turn_ids: Arc<Mutex<Vec<String>>>,
    compaction_usage: TokenUsage,
    compaction_error: Option<TalosError>,
}

#[derive(Clone, Copy)]
enum TestCancelMode {
    Cancelled,
    CleanupUnconfirmed,
    MissingLiveTurn,
    Rejected,
}

impl TalosService {
    async fn create_session(
        &self,
        actor: &Actor,
        params: CreateSessionRequest,
    ) -> ServiceResult<SessionInfo> {
        let record = self.access.create_session(actor, params).await?;
        self.sessions.session_info(&record).await
    }
}

struct TestAuthService;

fn configured_test_config() -> TalosConfig {
    let mut config = TalosConfig::default();
    config.llm.anthropic.models = vec![talos_config::ProviderModelConfig {
        id: config.llm.model.clone(),
        name: "Default test model".to_string(),
        context_window: 32_768,
        reasoning: false,
        reasoning_effort: Default::default(),
        speed: Default::default(),
        input: Vec::new(),
        cost: Default::default(),
    }];
    config
}

fn configured_test_config_with_turn_model() -> TalosConfig {
    let mut config = configured_test_config();
    config
        .llm
        .anthropic
        .models
        .push(talos_config::ProviderModelConfig {
            id: "turn-model".to_string(),
            name: "Turn Model".to_string(),
            context_window: 128_000,
            reasoning: true,
            reasoning_effort: talos_config::ReasoningEffort::Medium,
            speed: talos_config::ModelSpeed::Standard,
            input: vec![talos_config::ModelInputKind::Text],
            cost: talos_config::ModelCostConfig::default(),
        });
    config
}

#[async_trait]
impl AgentCoordinator for TestAgentCoordinator {
    async fn resolve_agent(&self, _params: AgentSelectionParams) -> TalosResult<String> {
        Ok("default".to_string())
    }

    async fn ensure_agent(&self, _agent_id: &str) -> TalosResult<()> {
        Ok(())
    }

    async fn select_execution_node(
        &self,
        _agent_id: &str,
        _user_id: &str,
        preferred_node_id: Option<&str>,
    ) -> TalosResult<String> {
        Ok(preferred_node_id.unwrap_or("embedded").to_string())
    }

    async fn start(&self, _params: AgentRunParams) -> TalosResult<AgentRunExecution> {
        unreachable!("execution is delegated by the scenario Coordinator")
    }

    async fn submit_input(&self, _params: AgentInputParams) -> TalosResult<AgentAdmission> {
        unreachable!("input is delegated by the scenario Coordinator")
    }

    async fn interrupt(&self, params: AgentInterruptParams) -> TalosResult<AgentInterruptOutcome> {
        self.cancelled_turn_ids
            .lock()
            .unwrap()
            .push(params.identity.turn_id);
        match self.cancel_mode {
            TestCancelMode::Cancelled => Ok(AgentInterruptOutcome::Cancelled),
            TestCancelMode::CleanupUnconfirmed => {
                Ok(AgentInterruptOutcome::CleanupUnconfirmed { task_count: 1 })
            }
            TestCancelMode::MissingLiveTurn => Err(TalosError::new(
                talos_error::ErrorCode::ResourceNotFound,
                "active turn for session session-interrupt-live-missing".to_string(),
            )),
            TestCancelMode::Rejected => Err(TalosError::internal("Harness rejected interruption")),
        }
    }

    async fn compact_session(&self, context: AgentContext) -> TalosResult<AgentRunExecution> {
        let identity = context.identity();
        let message_id = "message-compaction".to_string();
        let block_id = "item-compaction".to_string();
        let started_message = AgentMessage {
            message_id: message_id.clone(),
            turn_id: identity.turn_id.clone(),
            role: AgentMessageRole::System,
            status: AgentMessageStatus::InProgress,
            blocks: Vec::new(),
            created_at: 2,
            completed_at: None,
        };
        let started_block = AgentMessageBlock {
            block_id: block_id.clone(),
            status: AgentMessageBlockStatus::Running,
            content: AgentMessageBlockContent::ContextCompaction(AgentContextCompaction {
                trigger: ContextCompactionTrigger::UserRequested,
                stage: ContextCompactionStage::Preparing,
                outcome: None,
                metrics: None,
                message: None,
            }),
            started_at: 3,
            completed_at: None,
        };
        let completed_block = AgentMessageBlock {
            block_id,
            status: AgentMessageBlockStatus::Completed,
            content: AgentMessageBlockContent::ContextCompaction(AgentContextCompaction {
                trigger: ContextCompactionTrigger::UserRequested,
                stage: ContextCompactionStage::Completed,
                outcome: Some(ContextCompactionOutcome::Applied),
                metrics: Some(Default::default()),
                message: Some("Context compacted".to_string()),
            }),
            started_at: 3,
            completed_at: Some(5),
        };
        let completed_message = AgentMessage {
            status: AgentMessageStatus::Completed,
            blocks: vec![completed_block.clone()],
            completed_at: Some(6),
            ..started_message.clone()
        };
        let started = CanonicalAgentEvent {
            identity: identity.clone(),
            sequence: 1,
            occurred_at: 1,
            payload: CanonicalAgentEventPayload::TurnStarted(AgentTurnStarted {}),
        };
        let progress = vec![
            CanonicalAgentEvent {
                identity: identity.clone(),
                sequence: 2,
                occurred_at: 2,
                payload: CanonicalAgentEventPayload::MessageStarted(AgentMessageStarted {
                    message: started_message,
                }),
            },
            CanonicalAgentEvent {
                identity: identity.clone(),
                sequence: 3,
                occurred_at: 3,
                payload: CanonicalAgentEventPayload::MessageBlockStarted(
                    AgentMessageBlockStarted {
                        message_id: message_id.clone(),
                        block: started_block,
                    },
                ),
            },
            CanonicalAgentEvent {
                identity: identity.clone(),
                sequence: 4,
                occurred_at: 4,
                payload: CanonicalAgentEventPayload::MessageBlockDelta(AgentMessageBlockDelta {
                    message_id: message_id.clone(),
                    block_id: completed_block.block_id.clone(),
                    delta: AgentMessageBlockDeltaContent::ContextCompaction(
                        AgentContextCompactionDelta {
                            stage: Some(ContextCompactionStage::Summarizing),
                            outcome: None,
                            metrics: None,
                            message: Some("Summarizing context".to_string()),
                        },
                    ),
                }),
            },
            CanonicalAgentEvent {
                identity: identity.clone(),
                sequence: 5,
                occurred_at: 5,
                payload: CanonicalAgentEventPayload::MessageBlockCompleted(
                    AgentMessageBlockCompleted {
                        message_id,
                        block: completed_block,
                    },
                ),
            },
            CanonicalAgentEvent {
                identity: identity.clone(),
                sequence: 6,
                occurred_at: 6,
                payload: CanonicalAgentEventPayload::MessageCompleted(completed_message.clone()),
            },
        ];
        let result = self.compaction_error.clone().map_or(Ok(()), Err);
        let terminal = CanonicalAgentEvent {
            identity,
            sequence: 7,
            occurred_at: 7,
            payload: match &result {
                Ok(()) => CanonicalAgentEventPayload::TurnCompleted(AgentTurnCompleted {
                    stop_reason: talos_agent_api::AgentStopReason::EndTurn,
                }),
                Err(error) => {
                    CanonicalAgentEventPayload::TurnFailed(talos_agent_api::AgentTurnFailed {
                        error: talos_error::ApiError {
                            code: error.error_code(),
                            message: error.to_api_error().message,
                            localization: None,
                        },
                        retryable: false,
                    })
                }
            },
        };
        let metrics = AgentRunMetrics {
            usage: self.compaction_usage.clone(),
            ..AgentRunMetrics::default()
        };
        let (progress_tx, progress_rx) = tokio::sync::mpsc::channel(progress.len());
        let (settlement_tx, settlement_rx) = tokio::sync::oneshot::channel();
        Ok(AgentRunExecution {
            admission: AgentAdmission {
                events: vec![started],
                permit: AgentAdmissionPermit::new(async move {
                    for event in progress {
                        progress_tx.send(event).await.map_err(|_| {
                            talos_error::TalosError::new(
                                talos_error::ErrorCode::InternalError,
                                "test progress receiver",
                            )
                        })?;
                    }
                    drop(progress_tx);
                    settlement_tx
                        .send(Ok(AgentRunSettlement::Finished {
                            result,
                            event: terminal,
                            messages: vec![completed_message],
                            metrics,
                        }))
                        .map_err(|_| {
                            talos_error::TalosError::new(
                                talos_error::ErrorCode::InternalError,
                                "test receiver",
                            )
                        })?;
                    Ok(())
                }),
            },
            events: AgentProgressEventReceiver::new(progress_rx),
            settlement: AgentRunSettlementReceiver::new(settlement_rx),
        })
    }

    async fn clear_session(
        &self,
        _params: AgentSessionContext,
        _expected_history_cursor: talos_types::SessionHistoryCursor,
    ) -> TalosResult<()> {
        Ok(())
    }

    async fn close_session(
        &self,
        _params: AgentSessionContext,
        _expected_history_cursor: talos_types::SessionHistoryCursor,
    ) -> TalosResult<()> {
        Ok(())
    }

    async fn shutdown(&self, _timeout: Duration) -> TalosResult<()> {
        Ok(())
    }
}

#[async_trait]
impl AgentCoordinator for TestExecutionCoordinator {
    async fn read_session_execution_state(
        &self,
        session: AgentSessionContext,
    ) -> TalosResult<Option<agent_protocol::SessionExecutionState>> {
        match self.execution.mode {
            TestAgentExecutionMode::WorkerStateBlocked => std::future::pending().await,
            TestAgentExecutionMode::WorkerStateUnavailable => Err(TalosError::new(
                talos_error::ErrorCode::ServiceUnavailable,
                "injected Worker transport failure",
            )),
            TestAgentExecutionMode::WorkerStateActive => {
                Ok(Some(agent_protocol::SessionExecutionState {
                    history_cursor: Default::default(),
                    active_turn: Some(AgentTurnRef::new(session.session_id, "remote-running")),
                    coordinator_id: Some("remote-peer".into()),
                    handoff_ready: false,
                }))
            }
            _ => self.delegate.read_session_execution_state(session).await,
        }
    }

    async fn resolve_agent(&self, params: AgentSelectionParams) -> TalosResult<String> {
        self.delegate.resolve_agent(params).await
    }

    async fn ensure_agent(&self, agent_id: &str) -> TalosResult<()> {
        self.delegate.ensure_agent(agent_id).await
    }

    async fn select_execution_node(
        &self,
        agent_id: &str,
        user_id: &str,
        preferred_node_id: Option<&str>,
    ) -> TalosResult<String> {
        self.delegate
            .select_execution_node(agent_id, user_id, preferred_node_id)
            .await
    }

    async fn start(&self, params: AgentRunParams) -> TalosResult<AgentRunExecution> {
        let admission_gate = match &self.execution.mode {
            TestAgentExecutionMode::StartTimeout => {
                return Err(TalosError::new(
                    talos_error::ErrorCode::Timeout,
                    "injected Agent start timeout",
                ));
            }
            TestAgentExecutionMode::StartFailed => {
                return Err(TalosError::internal("injected Agent start failure"));
            }
            TestAgentExecutionMode::Accepted
            | TestAgentExecutionMode::WorkerStateBlocked
            | TestAgentExecutionMode::WorkerStateUnavailable
            | TestAgentExecutionMode::WorkerStateActive
            | TestAgentExecutionMode::WorkerClearBlocked => None,
            TestAgentExecutionMode::AdmissionBlocked { reached, release } => {
                Some((reached.clone(), release.clone()))
            }
        };
        let identity = params.context.identity();
        let admission = vec![
            CanonicalAgentEvent {
                identity: identity.clone(),
                sequence: 1,
                occurred_at: 1,
                payload: CanonicalAgentEventPayload::TurnStarted(AgentTurnStarted {}),
            },
            CanonicalAgentEvent {
                identity,
                sequence: 2,
                occurred_at: params.input.completed_at.unwrap_or(params.input.created_at),
                payload: CanonicalAgentEventPayload::MessageCompleted(params.input),
            },
        ];
        let (progress_tx, progress_rx) = tokio::sync::mpsc::channel(1);
        let (settlement_tx, settlement_rx) = tokio::sync::oneshot::channel();
        Ok(AgentRunExecution {
            admission: AgentAdmission {
                events: admission,
                permit: AgentAdmissionPermit::new(async move {
                    if let Some((reached, release)) = admission_gate {
                        reached.notify_one();
                        release.notified().await;
                    }
                    let hold = tokio::spawn(async move {
                        tokio::time::sleep(Duration::from_secs(60)).await;
                        drop(progress_tx);
                        drop(settlement_tx);
                    });
                    drop(hold);
                    Ok(())
                }),
            },
            events: AgentProgressEventReceiver::new(progress_rx),
            settlement: AgentRunSettlementReceiver::new(settlement_rx),
        })
    }

    async fn submit_input(&self, params: AgentInputParams) -> TalosResult<AgentAdmission> {
        Ok(AgentAdmission {
            events: vec![CanonicalAgentEvent {
                identity: params.identity,
                sequence: 3,
                occurred_at: params.input.completed_at.unwrap_or(params.input.created_at),
                payload: CanonicalAgentEventPayload::MessageCompleted(params.input),
            }],
            permit: AgentAdmissionPermit::new(async { Ok(()) }),
        })
    }

    async fn interrupt(&self, params: AgentInterruptParams) -> TalosResult<AgentInterruptOutcome> {
        self.delegate.interrupt(params).await
    }

    async fn compact_session(&self, params: AgentContext) -> TalosResult<AgentRunExecution> {
        self.delegate.compact_session(params).await
    }

    async fn clear_session(
        &self,
        params: AgentSessionContext,
        expected_history_cursor: talos_types::SessionHistoryCursor,
    ) -> TalosResult<()> {
        if matches!(
            self.execution.mode,
            TestAgentExecutionMode::WorkerClearBlocked
        ) {
            return std::future::pending().await;
        }
        self.delegate
            .clear_session(params, expected_history_cursor)
            .await
    }

    async fn close_session(
        &self,
        params: AgentSessionContext,
        expected_history_cursor: talos_types::SessionHistoryCursor,
    ) -> TalosResult<()> {
        self.delegate
            .close_session(params, expected_history_cursor)
            .await
    }

    async fn shutdown(&self, timeout: Duration) -> TalosResult<()> {
        self.delegate.shutdown(timeout).await
    }
}

#[async_trait]
impl AuthService for TestAuthService {
    async fn logout(&self, _actor: &RequestIdentity) -> TalosResult<()> {
        Ok(())
    }

    async fn verify_access_token(&self, token: &str) -> TalosResult<RequestIdentity> {
        Ok(RequestIdentity {
            user_id: token.to_string(),
            username: None,
            role: talos_auth::UserRole::User,
            auth_disabled: false,
            access_token: Some(token.to_string()),
        })
    }
}

#[async_trait]
impl UserService for TestAuthService {
    async fn get_current_user(&self, actor: &RequestIdentity) -> TalosResult<UserDto> {
        Ok(UserDto {
            user_id: actor.user_id.clone(),
            username: actor
                .username
                .clone()
                .unwrap_or_else(|| actor.user_id.clone()),
            email: None,
            role: actor.role,
            r#type: talos_auth::UserType::Free,
            disabled: false,
            created_at: 0,
            updated_at: 0,
        })
    }
}

async fn test_application(
    agent_execution: Arc<TestAgentExecution>,
    config: TalosConfig,
) -> (TalosService, Arc<DatabaseSessionStore>) {
    test_application_with_coordinator(
        agent_execution,
        Arc::new(TestAgentCoordinator {
            cancel_mode: TestCancelMode::Cancelled,
            cancelled_turn_ids: Arc::new(Mutex::new(Vec::new())),
            compaction_usage: TokenUsage::default(),
            compaction_error: None,
        }),
        config,
    )
    .await
}

async fn test_application_with_coordinator(
    agent_execution: Arc<TestAgentExecution>,
    coordinator: Arc<dyn AgentCoordinator>,
    config: TalosConfig,
) -> (TalosService, Arc<DatabaseSessionStore>) {
    let storage = Arc::new(SqliteStorage::new(":memory:").await.unwrap());
    run_test_migrations(storage.pool()).await;
    let profile_store: Arc<dyn ProfileStore> =
        Arc::new(DatabaseProfileStore::new(storage.pool().clone()));
    let store = Arc::new(DatabaseSessionStore::new(storage.clone()).await.unwrap());
    let file_store = Arc::new(DatabaseFileStore::new(storage.pool().clone()));
    let nodes = crate::AgentNodeService::new(Arc::new(
        talos_store::node::DatabaseAgentNodeStore::new(storage.pool().clone()),
    ));
    let project_store = Arc::new(StorageProjectStore::new(storage).await.unwrap());
    let file_operator = Operator::new(Memory::default()).unwrap().finish();
    let files = Arc::new(
        crate::FileService::new(
            file_store,
            project_store.clone(),
            BTreeMap::from([("local".to_string(), file_operator)]),
            talos_config::FileStorageConfig::default(),
        )
        .unwrap(),
    );
    let event_service = Arc::new(TimelineEventService::new(store.clone()));
    let auth = Arc::new(TestAuthService);
    let auth_service: Arc<dyn AuthService> = auth.clone();
    let user_service: Arc<dyn UserService> = auth;
    let coordinator: Arc<dyn AgentCoordinator> = Arc::new(TestExecutionCoordinator {
        execution: agent_execution,
        delegate: coordinator,
    });
    let coordinator = crate::test_support::with_environment(coordinator, config.clone());
    let service = TalosService::new(
        coordinator,
        nodes,
        store.clone(),
        store.clone(),
        project_store,
        profile_store,
        files,
        event_service,
        config,
        auth_service,
        user_service,
    );
    (service, store)
}

fn request_source() -> SessionSource {
    SessionSource::http()
}

fn start_turn_request(session_id: &str, text: &str) -> StartTurnRequest {
    StartTurnRequest {
        node_id: None,
        session_id: session_id.to_string(),
        input: crate::test_support::test_user_input(text),
        model_selection: None,
        user_permission_mode: None,
        project_id: None,
        current_directory: None,
        client_request_id: None,
    }
}

#[tokio::test]
async fn first_turn_persists_selected_permission_mode() {
    let (service, _store) = test_application(
        Arc::new(TestAgentExecution {
            mode: TestAgentExecutionMode::Accepted,
        }),
        configured_test_config(),
    )
    .await;
    let actor = Actor::regular("member-user");
    let mut request = start_turn_request("session-initial-permission", "hello");
    request.user_permission_mode = Some(talos_types::UserPermissionMode::AskForApproval);

    service
        .start_turn(&actor, request_source(), request, None)
        .await
        .expect("first turn should create the session");

    let session = service
        .get_session(&actor, "session-initial-permission")
        .await
        .expect("created session should be readable");
    assert_eq!(
        session.effective_mode,
        talos_types::UserPermissionMode::AskForApproval
    );
}

fn create_session_params(
    session_id: &str,
    source: SessionSource,
    current_directory: Option<std::path::PathBuf>,
) -> CreateSessionRequest {
    CreateSessionRequest {
        node_id: None,
        session_id: session_id.to_string(),
        source,
        model_selection: None,
        user_permission_mode: None,
        project_id: None,
        current_directory,
    }
}

#[tokio::test]
async fn delete_session_clears_its_message_queue() {
    let (service, _store) = test_application(
        Arc::new(TestAgentExecution {
            mode: TestAgentExecutionMode::StartFailed,
        }),
        configured_test_config(),
    )
    .await;
    let actor = Actor::regular("member-user");
    service
        .create_session(
            &actor,
            create_session_params("session-delete-queue", request_source(), None),
        )
        .await
        .expect("session should be created");
    service
        .message_queue
        .add(
            "session-delete-queue",
            crate::test_support::test_user_input("waiting"),
        )
        .expect("queued message should be added");

    let ack = service
        .delete_session(&actor, "session-delete-queue")
        .await
        .expect("session should be deleted");

    assert!(ack);
    assert_eq!(
        service
            .message_queue
            .list("session-delete-queue")
            .expect("deleted session queue should list")
            .messages,
        Vec::new()
    );
}

#[tokio::test]
async fn existing_session_start_turn_applies_model_selection_before_dispatch() {
    let (service, _store) = test_application(
        Arc::new(TestAgentExecution {
            mode: TestAgentExecutionMode::StartFailed,
        }),
        configured_test_config_with_turn_model(),
    )
    .await;
    let actor = Actor::regular("member-user");
    service
        .create_session(
            &actor,
            create_session_params("session-model-ref", request_source(), None),
        )
        .await
        .expect("session should be created");

    let mut request = start_turn_request("session-model-ref", "hello");
    request.model_selection = Some(talos_types::ModelSelection::without_reasoning(
        talos_types::ModelRef {
            provider: "anthropic".to_string(),
            id: "turn-model".to_string(),
        },
    ));
    let _ = service
        .start_turn(&actor, request_source(), request, None)
        .await;

    let session = service
        .get_session(&actor, "session-model-ref")
        .await
        .expect("session should still be readable");
    assert_eq!(session.model_selection.model_ref.provider, "anthropic");
    assert_eq!(session.model_selection.model_ref.id, "turn-model");
}

#[tokio::test]
async fn start_replay_returns_before_reapplying_session_metadata() {
    let (service, _store) = test_application(
        Arc::new(TestAgentExecution {
            mode: TestAgentExecutionMode::Accepted,
        }),
        configured_test_config_with_turn_model(),
    )
    .await;
    let actor = Actor::regular("member-user");
    service
        .create_session(
            &actor,
            create_session_params("session-start-replay", request_source(), None),
        )
        .await
        .unwrap();
    let mut request = start_turn_request("session-start-replay", "hello");
    request.model_selection = Some(talos_types::ModelSelection::without_reasoning(
        talos_types::ModelRef {
            provider: "anthropic".to_string(),
            id: "turn-model".to_string(),
        },
    ));

    let first = service
        .start_turn(
            &actor,
            request_source(),
            request.clone(),
            Some("start-replay-key".to_string()),
        )
        .await
        .unwrap();
    let replay = service
        .start_turn(
            &actor,
            request_source(),
            request,
            Some("start-replay-key".to_string()),
        )
        .await
        .unwrap();

    assert_eq!(replay, first);
}

#[tokio::test]
async fn start_key_rejects_different_metadata_before_session_patch() {
    let (service, _store) = test_application(
        Arc::new(TestAgentExecution {
            mode: TestAgentExecutionMode::Accepted,
        }),
        configured_test_config_with_turn_model(),
    )
    .await;
    let actor = Actor::regular("member-user");
    service
        .create_session(
            &actor,
            create_session_params("session-start-conflict", request_source(), None),
        )
        .await
        .unwrap();
    let request = start_turn_request("session-start-conflict", "hello");
    service
        .start_turn(
            &actor,
            request_source(),
            request.clone(),
            Some("start-conflict-key".to_string()),
        )
        .await
        .unwrap();
    let mut conflicting = request;
    conflicting.model_selection = Some(talos_types::ModelSelection::without_reasoning(
        talos_types::ModelRef {
            provider: "anthropic".to_string(),
            id: "turn-model".to_string(),
        },
    ));

    let error = service
        .start_turn(
            &actor,
            request_source(),
            conflicting,
            Some("start-conflict-key".to_string()),
        )
        .await
        .expect_err("same key with different metadata must conflict");

    assert!(error.is_code(talos_error::ErrorCode::IdempotencyConflict));
}

#[tokio::test]
async fn existing_session_start_turn_does_not_change_permission_mode() {
    let (service, _store) = test_application(
        Arc::new(TestAgentExecution {
            mode: TestAgentExecutionMode::StartFailed,
        }),
        configured_test_config(),
    )
    .await;
    let actor = Actor::regular("member-user");
    service
        .create_session(
            &actor,
            create_session_params("session-existing-permission", request_source(), None),
        )
        .await
        .expect("session should be created");

    let mut request = start_turn_request("session-existing-permission", "hello");
    request.user_permission_mode = Some(talos_types::UserPermissionMode::AskForApproval);
    let _ = service
        .start_turn(&actor, request_source(), request, None)
        .await;

    let session = service
        .get_session(&actor, "session-existing-permission")
        .await
        .expect("session should remain readable");
    assert_eq!(
        session.effective_mode,
        talos_types::UserPermissionMode::FullAccess
    );
}

#[tokio::test]
async fn existing_session_start_turn_rejects_a_project_override() {
    let (service, _store) = test_application(
        Arc::new(TestAgentExecution {
            mode: TestAgentExecutionMode::StartFailed,
        }),
        configured_test_config(),
    )
    .await;
    let actor = Actor::regular("member-user");
    let first_directory = tempfile::tempdir().unwrap();
    let second_directory = tempfile::tempdir().unwrap();
    let first_project = service
        .create_project(
            &actor,
            talos_types::ProjectCreateRequest {
                node_id: None,
                name: "First Project".to_string(),
                directory: first_directory.path().to_path_buf(),
            },
        )
        .await
        .unwrap();
    let second_project = service
        .create_project(
            &actor,
            talos_types::ProjectCreateRequest {
                node_id: None,
                name: "Second Project".to_string(),
                directory: second_directory.path().to_path_buf(),
            },
        )
        .await
        .unwrap();
    let mut params = create_session_params("session-project-binding", request_source(), None);
    params.project_id = Some(first_project.project_id.clone());
    service
        .create_session(&actor, params)
        .await
        .expect("project session should be created");

    let mut request = start_turn_request("session-project-binding", "hello");
    request.project_id = Some(second_project.project_id);
    let error = service
        .start_turn(&actor, request_source(), request, None)
        .await
        .unwrap_err();

    assert!(error.is_code(talos_error::ErrorCode::InvalidRequest));
    assert_eq!(
        service
            .get_session(&actor, "session-project-binding")
            .await
            .unwrap()
            .project_id,
        Some(first_project.project_id)
    );
}

async fn assert_session_absent(store: &DatabaseSessionStore, session_id: &str) {
    assert!(store
        .get_session_record(session_id)
        .await
        .expect("session read should succeed")
        .is_none());
}

fn turn_submission_reservation(
    session_id: &str,
    idempotency_key: &str,
    request_hash: &str,
) -> CommandReservation {
    let now = chrono::Utc::now().timestamp_millis();
    CommandReservation {
        session_id: session_id.to_string(),
        command: IdempotentCommand::TurnSubmission,
        idempotency_key: idempotency_key.to_string(),
        request_hash: request_hash.to_string(),
        request_id: "request-test".to_string(),
        turn_id: "turn-test".to_string(),
        message_id: "message-test".to_string(),
        lease_id: "lease-test".to_string(),
        now,
        lease_expires_at: now.saturating_add(30_000),
        record_expires_at: None,
    }
}

async fn assert_session_creation_rolled_back(
    service: &TalosService,
    store: &DatabaseSessionStore,
    session_id: &str,
    idempotency_key: Option<&str>,
) {
    assert_session_absent(store, session_id).await;
    let recovery = service
        .events
        .recover_timeline(session_id, None, 32)
        .await
        .expect("recovery should succeed");
    assert!(recovery.replay.events.is_empty());
    assert_eq!(recovery.active_turn, None);
    assert!(service
        .events
        .display_active_turn(session_id)
        .await
        .expect("active turn lookup should succeed")
        .is_none());
    if let Some(key) = idempotency_key {
        let reservation = store
            .reserve_command(turn_submission_reservation(
                session_id,
                key,
                "new-request-hash",
            ))
            .await
            .expect("idempotency key should be reusable after rollback");
        assert!(matches!(reservation, IdempotencyReservation::Reserved(_)));
    }
}

#[path = "session_deletion_tests.rs"]
mod session_deletion_tests;
