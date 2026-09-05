use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::test_support::{
    canonical_completed_message_events, canonical_user_admission, commit_test_running_turn,
    run_test_migrations,
};
use crate::SessionMessageQueueService;
use async_trait::async_trait;
use talos_agent::{
    AgentAdmission, AgentAdmissionPermit, AgentContext, AgentInputParams, AgentInterruptParams,
    AgentProgressEventReceiver, AgentRunExecution, AgentRunParams, AgentRunSettlement,
    AgentRunSettlementReceiver, AgentSessionContext,
};
use talos_agent_api::{
    AgentAssistantText, AgentAssistantTextPhase, AgentEvent as CanonicalAgentEvent,
    AgentEventPayload as CanonicalAgentEventPayload, AgentInterruptOutcome, AgentMessage,
    AgentMessageBlock, AgentMessageBlockContent, AgentMessageBlockStatus, AgentMessageRole,
    AgentMessageStatus, AgentRunMetrics, AgentStopReason, AgentTurnCompleted,
};
use talos_coordinator::{AgentCoordinator, AgentSelectionParams};
use talos_error::{TalosError, TalosResult};
use talos_storage::{SqliteStorage, Storage};
use talos_store::profile::{DatabaseProfileStore, ProfileStore};
use talos_store::project::MockProjectStore;
use talos_store::session::{
    AgentExecutionStore, CommitTerminalSettlementParams, DatabaseSessionStore, SessionQuery,
    SessionRecord, SessionStore,
};
use talos_types::talos::{SessionSource, SessionSourceProvider, SessionSourceType};
use talos_types::{
    AgentCaller, AgentTarget, AgentTurnRef, FollowUpSubagentRequest, InterruptSubagentRequest,
    ListSubagentsRequest, ModelRef, ModelSelection, SendSubagentMessageRequest, SessionConf,
    SpawnSubagentRequest, SubagentOutcome, SubagentRef, TalosConfig, WaitSubagentRequest,
};
use tokio::sync::mpsc;

use super::SubagentService;
use crate::session::{CreateSessionParams, SessionService};
use crate::turn::TurnService;
use crate::TimelineEventService;

#[path = "list_tests.rs"]
mod list_tests;

#[derive(Default)]
struct RecordingAgentExecution {
    requests: Mutex<Vec<AgentRunParams>>,
    inputs: Mutex<Vec<AgentInputParams>>,
    input_error: Mutex<Option<TalosError>>,
    fail_next: Mutex<bool>,
    hold_running_turn: Mutex<bool>,
    active_progress: Mutex<Vec<mpsc::Sender<CanonicalAgentEvent>>>,
    input_sequences: Mutex<HashMap<(String, String), u64>>,
    active_settlements: Mutex<
        Vec<
            tokio::sync::oneshot::Sender<talos_agent::AgentResult<talos_agent::AgentRunSettlement>>,
        >,
    >,
}

#[path = "authorization_tests.rs"]
mod authorization_tests;
#[path = "follow_up_tests.rs"]
mod follow_up_tests;
#[path = "interrupt_tests.rs"]
mod interrupt_tests;
#[path = "message_tests.rs"]
mod message_tests;
#[path = "wait_tests.rs"]
mod wait_tests;

struct RecordingExecutionCoordinator {
    execution: Arc<RecordingAgentExecution>,
    delegate: Arc<dyn AgentCoordinator>,
}

#[async_trait]
impl AgentCoordinator for RecordingExecutionCoordinator {
    async fn resolve_agent(&self, params: AgentSelectionParams) -> TalosResult<String> {
        self.delegate.resolve_agent(params).await
    }

    async fn ensure_agent(&self, agent_id: &str) -> TalosResult<()> {
        self.delegate.ensure_agent(agent_id).await
    }

    async fn start(&self, request: AgentRunParams) -> TalosResult<AgentRunExecution> {
        self.execution
            .requests
            .lock()
            .unwrap()
            .push(request.clone());
        if std::mem::take(&mut *self.execution.fail_next.lock().unwrap()) {
            return Err(TalosError::public(
                talos_error::ErrorCode::Conflict,
                "start failed".to_string(),
            ));
        }
        let identity = request.context.identity();
        let admission = canonical_user_admission(identity.clone(), request.input, 1, true);
        let assistant = AgentMessage {
            message_id: format!("assistant-{}", identity.turn_id),
            turn_id: identity.turn_id.clone(),
            role: AgentMessageRole::Agent,
            status: AgentMessageStatus::Completed,
            blocks: vec![AgentMessageBlock {
                block_id: format!("answer-{}", identity.turn_id),
                status: AgentMessageBlockStatus::Completed,
                content: AgentMessageBlockContent::AssistantText(AgentAssistantText {
                    phase: AgentAssistantTextPhase::FinalAnswer,
                    text: "AGENT_CHILD_OK".to_string(),
                }),
                started_at: 1,
                completed_at: Some(1),
            }],
            created_at: 1,
            completed_at: Some(1),
        };
        let progress = canonical_completed_message_events(
            identity.clone(),
            assistant.clone(),
            admission.last().expect("test admission").sequence + 1,
        );
        let terminal = CanonicalAgentEvent {
            identity,
            sequence: progress.last().expect("test progress").sequence + 1,
            occurred_at: 1,
            payload: CanonicalAgentEventPayload::TurnCompleted(AgentTurnCompleted {
                stop_reason: AgentStopReason::EndTurn,
            }),
        };
        let (progress_tx, progress_rx) = mpsc::channel(progress.len().max(1));
        let (settlement_tx, settlement_rx) = tokio::sync::oneshot::channel();
        let mut settlement_tx = Some(settlement_tx);
        let hold_running_turn = *self.execution.hold_running_turn.lock().unwrap();
        if hold_running_turn {
            self.execution.input_sequences.lock().unwrap().insert(
                (
                    terminal.identity.session_id.clone(),
                    terminal.identity.turn_id.clone(),
                ),
                terminal.sequence,
            );
            self.execution
                .active_progress
                .lock()
                .unwrap()
                .push(progress_tx.clone());
            self.execution
                .active_settlements
                .lock()
                .unwrap()
                .push(settlement_tx.take().unwrap());
        }
        Ok(AgentRunExecution {
            admission: AgentAdmission {
                events: admission,
                permit: AgentAdmissionPermit::new(async move {
                    for event in progress {
                        progress_tx.send(event).await.map_err(|_| {
                            talos_agent::AgentError::provider_error("test progress receiver")
                        })?;
                    }
                    if hold_running_turn {
                        return Ok(());
                    }
                    drop(progress_tx);
                    settlement_tx
                        .expect("completed test Turn settlement sender")
                        .send(Ok(AgentRunSettlement::Finished {
                            event: terminal,
                            messages: vec![assistant],
                            metrics: AgentRunMetrics::default(),
                        }))
                        .map_err(|_| talos_agent::AgentError::provider_error("test receiver"))?;
                    Ok(())
                }),
            },
            events: AgentProgressEventReceiver::new(progress_rx),
            settlement: AgentRunSettlementReceiver::new(settlement_rx),
        })
    }

    async fn submit_input(&self, params: AgentInputParams) -> TalosResult<AgentAdmission> {
        self.execution.inputs.lock().unwrap().push(params.clone());
        if let Some(error) = self.execution.input_error.lock().unwrap().take() {
            return Err(error);
        }
        let identity = params.identity.clone();
        let key = (identity.session_id.clone(), identity.turn_id.clone());
        let mut input_sequences = self.execution.input_sequences.lock().unwrap();
        let sequence = input_sequences.get(&key).copied().unwrap_or(6);
        let events = canonical_user_admission(identity, params.input, sequence, false);
        input_sequences.insert(key, events.last().unwrap().sequence + 1);
        Ok(AgentAdmission {
            events,
            permit: AgentAdmissionPermit::new(async { Ok(()) }),
        })
    }

    async fn interrupt(&self, params: AgentInterruptParams) -> TalosResult<AgentInterruptOutcome> {
        self.delegate.interrupt(params).await
    }

    async fn compact_session(&self, params: AgentContext) -> TalosResult<AgentRunExecution> {
        self.delegate.compact_session(params).await
    }

    async fn clear_session(&self, params: AgentSessionContext) -> TalosResult<()> {
        self.delegate.clear_session(params).await
    }

    async fn close_session(&self, params: AgentSessionContext) -> TalosResult<()> {
        self.delegate.close_session(params).await
    }

    async fn shutdown(&self, timeout: Duration) -> TalosResult<()> {
        self.delegate.shutdown(timeout).await
    }
}

#[derive(Default)]
struct RecordingAgentCoordinator {
    cancellations: Mutex<Vec<(String, String, Option<String>)>>,
    rejected_agent_id: Mutex<Option<String>>,
    interrupt_outcome: Mutex<Option<AgentInterruptOutcome>>,
}

#[async_trait]
impl AgentCoordinator for RecordingAgentCoordinator {
    async fn resolve_agent(&self, _params: AgentSelectionParams) -> TalosResult<String> {
        Ok("default".to_string())
    }

    async fn ensure_agent(&self, agent_id: &str) -> TalosResult<()> {
        if self.rejected_agent_id.lock().unwrap().as_deref() == Some(agent_id) {
            return Err(TalosError::public(
                talos_error::ErrorCode::ResourceNotFound,
                format!("agent {agent_id}"),
            ));
        }
        Ok(())
    }

    async fn start(&self, _params: AgentRunParams) -> TalosResult<AgentRunExecution> {
        unreachable!("execution is delegated by the scenario Coordinator")
    }

    async fn submit_input(&self, _params: AgentInputParams) -> TalosResult<AgentAdmission> {
        unreachable!("input is delegated by the scenario Coordinator")
    }

    async fn interrupt(&self, params: AgentInterruptParams) -> TalosResult<AgentInterruptOutcome> {
        self.cancellations.lock().unwrap().push((
            params.identity.session_id,
            params.identity.turn_id,
            Some(params.agent_id),
        ));
        Ok(self
            .interrupt_outcome
            .lock()
            .unwrap()
            .take()
            .unwrap_or(AgentInterruptOutcome::Cancelled))
    }

    async fn compact_session(&self, _params: AgentContext) -> TalosResult<AgentRunExecution> {
        unreachable!("not used by spawn")
    }

    async fn clear_session(&self, _params: AgentSessionContext) -> TalosResult<()> {
        Ok(())
    }

    async fn close_session(&self, _params: AgentSessionContext) -> TalosResult<()> {
        Ok(())
    }

    async fn shutdown(&self, _timeout: Duration) -> TalosResult<()> {
        Ok(())
    }
}

struct TestServices {
    subagents: SubagentService,
    sessions: Arc<SessionService>,
    store: Arc<DatabaseSessionStore>,
    storage: Arc<SqliteStorage>,
    executor: Arc<RecordingAgentExecution>,
    coordinator: Arc<RecordingAgentCoordinator>,
    timeline: Arc<TimelineEventService>,
    _workspace: tempfile::TempDir,
}

fn message_text(message: &talos_agent_api::AgentMessage) -> Option<&str> {
    match message.blocks.first()?.content {
        talos_agent_api::AgentMessageBlockContent::UserContent(
            talos_agent_api::AgentUserContent::Text { ref text },
        ) => Some(text),
        _ => None,
    }
}

async fn test_services() -> TestServices {
    test_services_uninitialized().await
}

async fn test_services_uninitialized() -> TestServices {
    let storage = Arc::new(
        SqliteStorage::new_with_max_connections(":memory:", 1)
            .await
            .unwrap(),
    );
    run_test_migrations(storage.pool()).await;
    let store = Arc::new(DatabaseSessionStore::new(storage.clone()).await.unwrap());
    let workspace = tempfile::tempdir().unwrap();
    let config = test_config().with_workspace_definition(talos_config::WorkspaceDefinition::new(
        talos_config::DEFAULT_WORKSPACE_ID,
        workspace.path().to_path_buf(),
    ));
    let sessions = Arc::new(SessionService::new(
        store.clone(),
        Arc::new(MockProjectStore::new()),
        config.clone(),
    ));
    let event_service = Arc::new(TimelineEventService::new(store.clone()));
    let executor = Arc::new(RecordingAgentExecution::default());
    let profile_store: Arc<dyn ProfileStore> =
        Arc::new(DatabaseProfileStore::new(storage.pool().clone()));
    let recording_coordinator = Arc::new(RecordingAgentCoordinator::default());
    let coordinator: Arc<dyn AgentCoordinator> = Arc::new(RecordingExecutionCoordinator {
        execution: executor.clone(),
        delegate: recording_coordinator.clone(),
    });
    let turns = Arc::new(TurnService::new(
        coordinator,
        store.clone(),
        event_service.clone(),
        profile_store,
        sessions.clone(),
        SessionMessageQueueService::new(10).expect("queue service should initialize"),
        config,
        Arc::new(tokio::sync::RwLock::new(())),
    ));
    let subagents = SubagentService::new(sessions.clone(), turns, event_service.clone());
    TestServices {
        subagents,
        sessions,
        store,
        storage,
        executor,
        coordinator: recording_coordinator,
        timeline: event_service,
        _workspace: workspace,
    }
}

fn test_config() -> TalosConfig {
    let mut config = TalosConfig::default();
    config.llm.model = "test-model".to_string();
    config.llm.anthropic.models = vec![talos_config::ProviderModelConfig {
        id: "test-model".to_string(),
        name: "Test model".to_string(),
        context_window: 32_768,
        reasoning: false,
        reasoning_effort: Default::default(),
        speed: Default::default(),
        input: Vec::new(),
        cost: Default::default(),
    }];
    config
}

async fn create_parent(services: &TestServices) -> AgentCaller {
    create_parent_named(services, "parent-1", "turn-parent").await
}

async fn create_parent_named(
    services: &TestServices,
    session_id: &str,
    turn_id: &str,
) -> AgentCaller {
    create_parent_with_agent(services, session_id, turn_id, "default").await
}

async fn create_parent_with_agent(
    services: &TestServices,
    session_id: &str,
    turn_id: &str,
    agent_id: &str,
) -> AgentCaller {
    let parent = services
        .sessions
        .create_session(CreateSessionParams {
            session_id: Some(session_id.to_string()),
            source: SessionSource {
                source_type: SessionSourceType::System,
                source_provider: SessionSourceProvider::System,
                channel_id: None,
                thread_id: None,
                environment: None,
            },
            user_id: "user-1".to_string(),
            target_agent: agent_id.to_string(),
            model_selection: Some(ModelSelection::without_reasoning(ModelRef {
                provider: "anthropic".to_string(),
                id: "test-model".to_string(),
            })),
            conf: Some(SessionConf::default()),
            workspace_id: "default".to_string(),
            project_id: None,
            current_directory: None,
        })
        .await
        .unwrap();
    let parent_ref = AgentCaller {
        session_id: parent.id,
        turn_id: turn_id.to_string(),
        agent_id: agent_id.to_string(),
    };
    let identity = talos_agent_api::AgentTurnRef::new(&parent_ref.session_id, &parent_ref.turn_id);
    let message = crate::turn::completed_agent_user_input(
        &parent_ref.turn_id,
        "message-parent",
        crate::test_support::test_user_input("running parent"),
        1,
    )
    .unwrap();
    commit_test_running_turn(
        services.store.as_ref() as &dyn AgentExecutionStore,
        identity,
        message,
    )
    .await;
    parent_ref
}

async fn create_running_child(
    services: &TestServices,
    parent: AgentCaller,
) -> (SubagentRef, String) {
    create_running_child_named(services, parent, "child-running").await
}

async fn create_running_child_named(
    services: &TestServices,
    parent: AgentCaller,
    session_id: &str,
) -> (SubagentRef, String) {
    let child = create_child_session(services, &parent, session_id).await;
    let turn_id = format!("turn-{session_id}");
    let identity = talos_agent_api::AgentTurnRef::new(&child.id, &turn_id);
    let message = crate::turn::completed_agent_user_input(
        &turn_id,
        "message-child",
        crate::test_support::test_user_input("running child"),
        1,
    )
    .unwrap();
    commit_test_running_turn(
        services.store.as_ref() as &dyn AgentExecutionStore,
        identity,
        message,
    )
    .await;
    (
        SubagentRef {
            path: format!("/root/{}", session_id.replace('-', "_"))
                .parse()
                .unwrap(),
            session_id: child.id,
            parent: parent.turn_ref(),
        },
        turn_id,
    )
}

async fn create_live_running_child(
    services: &TestServices,
    parent: AgentCaller,
) -> (SubagentRef, String) {
    let session_id = "child-live";
    let child = create_child_session(services, &parent, session_id).await;
    *services.executor.hold_running_turn.lock().unwrap() = true;
    let started = services
        .subagents
        .turns
        .submit_subagent_turn(
            child.clone(),
            talos_types::StartTurnRequest {
                session_id: child.id.clone(),
                input: crate::test_support::test_user_input("running child"),
                model_selection: None,
                user_permission_mode: None,
                project_id: None,
                current_directory: None,
                client_request_id: None,
            },
            "reviewer".to_string(),
        )
        .await
        .unwrap();
    *services.executor.hold_running_turn.lock().unwrap() = false;
    (
        SubagentRef {
            path: "/root/child_live".parse().unwrap(),
            session_id: child.id,
            parent: parent.turn_ref(),
        },
        started.turn_id,
    )
}

async fn create_child_session(
    services: &TestServices,
    parent: &AgentCaller,
    session_id: &str,
) -> SessionRecord {
    services
        .sessions
        .create_subagent_session(
            CreateSessionParams {
                session_id: Some(session_id.to_string()),
                source: SessionSource {
                    source_type: SessionSourceType::System,
                    source_provider: SessionSourceProvider::System,
                    channel_id: None,
                    thread_id: None,
                    environment: None,
                },
                user_id: "user-1".to_string(),
                target_agent: "reviewer".to_string(),
                model_selection: Some(ModelSelection::without_reasoning(ModelRef {
                    provider: "anthropic".to_string(),
                    id: "test-model".to_string(),
                })),
                conf: None,
                workspace_id: "default".to_string(),
                project_id: None,
                current_directory: None,
            },
            parent.turn_ref(),
            parent.session_id.clone(),
            format!("/root/{}", session_id.replace('-', "_"))
                .parse()
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn settle_canonical_turn(
    store: &DatabaseSessionStore,
    timeline: &TimelineEventService,
    session_id: &str,
    turn_id: &str,
    payload: CanonicalAgentEventPayload,
) {
    let identity = talos_agent_api::AgentTurnRef::new(session_id, turn_id);
    let active = store
        .get_agent_active_turn(session_id)
        .await
        .unwrap()
        .expect("canonical active turn must exist");
    let events = store.list_agent_turn_events(&identity, 64).await.unwrap();
    let completed_turn_metrics = matches!(payload, CanonicalAgentEventPayload::TurnCompleted(_))
        .then(AgentRunMetrics::default);
    let terminal = store
        .commit_terminal_settlement(CommitTerminalSettlementParams {
            event: CanonicalAgentEvent {
                identity,
                sequence: events
                    .last()
                    .expect("canonical turn must have admission events")
                    .sequence
                    .saturating_add(1),
                occurred_at: 2,
                payload,
            },
            expected_event_seq: active.last_event_seq,
            completed_turn_metrics,
        })
        .await
        .unwrap();
    timeline.publish_committed([terminal]);
}

async fn commit_agent_text(
    store: &DatabaseSessionStore,
    timeline: &TimelineEventService,
    session_id: &str,
    turn_id: &str,
    message_id: &str,
    phase: AgentAssistantTextPhase,
    text: String,
) {
    let identity = talos_agent_api::AgentTurnRef::new(session_id, turn_id);
    let active = store
        .get_agent_active_turn(session_id)
        .await
        .unwrap()
        .expect("canonical active Turn must exist");
    let events = store.list_agent_turn_events(&identity, 64).await.unwrap();
    let message = AgentMessage {
        message_id: message_id.to_string(),
        turn_id: turn_id.to_string(),
        role: AgentMessageRole::Agent,
        status: AgentMessageStatus::Completed,
        blocks: vec![AgentMessageBlock {
            block_id: format!("{message_id}-block"),
            status: AgentMessageBlockStatus::Completed,
            content: AgentMessageBlockContent::AssistantText(AgentAssistantText { phase, text }),
            started_at: 2,
            completed_at: Some(2),
        }],
        created_at: 2,
        completed_at: Some(2),
    };
    let message_events = canonical_completed_message_events(
        identity,
        message,
        events
            .last()
            .expect("canonical Turn must have admission events")
            .sequence
            .saturating_add(1),
    );
    let mut expected_event_seq = active.last_event_seq;
    for event in message_events {
        let commit = store
            .commit_progress_event(event, expected_event_seq)
            .await
            .unwrap();
        expected_event_seq = commit.active_turn.last_event_seq;
        timeline.publish_committed(commit.events);
    }
}

fn target(subagent: &SubagentRef) -> AgentTarget {
    AgentTarget::canonical(subagent.path.clone())
}

fn wait_request(
    caller: AgentCaller,
    subagent: &SubagentRef,
    timeout_ms: u64,
) -> WaitSubagentRequest {
    WaitSubagentRequest {
        caller,
        target: target(subagent),
        timeout_ms,
    }
}

#[tokio::test]
async fn spawn_creates_bound_child_and_initial_turn() {
    let services = test_services().await;
    let parent = create_parent(&services).await;

    let spawned = services
        .subagents
        .spawn(SpawnSubagentRequest {
            caller: parent.clone(),
            task_name: "research".to_string(),
            prompt: "Review the patch".to_string(),
        })
        .await
        .unwrap();
    let child = spawned.subagent.clone();

    let record = services
        .store
        .get_session_record(&child.session_id)
        .await
        .unwrap()
        .unwrap();
    let request = services.executor.requests.lock().unwrap()[0].clone();
    assert_eq!(
        (
            child,
            record.agent_id,
            record.tree_identity,
            request.context.session.agent_id,
            message_text(&request.input),
            spawned.turn,
        ),
        (
            SubagentRef {
                path: "/root/research".parse().unwrap(),
                session_id: record.id.clone(),
                parent: parent.turn_ref(),
            },
            "default".to_string(),
            talos_store::session::SessionTreeIdentity::Subagent {
                parent: parent.turn_ref(),
                tree_root_session_id: "parent-1".to_string(),
                path: "/root/research".parse().unwrap(),
            },
            "default".to_string(),
            Some("Review the patch"),
            AgentTurnRef {
                session_id: record.id.clone(),
                turn_id: services.executor.requests.lock().unwrap()[0]
                    .context
                    .turn_id
                    .clone(),
            },
        )
    );
    assert_eq!(
        services
            .store
            .list_session_records(&SessionQuery {
                tree_scope: talos_store::session::SessionTreeScope::Primary,
                ..SessionQuery::default()
            })
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn spawn_rejects_a_caller_agent_that_does_not_own_the_parent_session() {
    let services = test_services().await;
    let mut parent = create_parent(&services).await;
    parent.agent_id = "reviewer".to_string();

    let error = services
        .subagents
        .spawn(SpawnSubagentRequest {
            caller: parent,
            task_name: "research".to_string(),
            prompt: "Review the patch".to_string(),
        })
        .await
        .unwrap_err();

    assert!(error.is_code(talos_error::ErrorCode::InvalidRequest));
    assert!(services.executor.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn spawn_rejects_inactive_or_child_parent() {
    let services = test_services().await;
    let parent = create_parent(&services).await;
    settle_canonical_turn(
        services.store.as_ref(),
        services.timeline.as_ref(),
        &parent.session_id,
        &parent.turn_id,
        CanonicalAgentEventPayload::TurnCompleted(AgentTurnCompleted {
            stop_reason: AgentStopReason::EndTurn,
        }),
    )
    .await;
    let error = services
        .subagents
        .spawn(SpawnSubagentRequest {
            caller: parent,
            task_name: "research".to_string(),
            prompt: "Review the patch".to_string(),
        })
        .await
        .unwrap_err();

    assert!(error.is_code(talos_error::ErrorCode::InvalidRequest));
    assert!(services.executor.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn spawn_resolves_the_inherited_agent_before_creating_a_child_session() {
    let services = test_services().await;
    let parent = create_parent_with_agent(&services, "parent-1", "turn-parent", "missing").await;
    *services.coordinator.rejected_agent_id.lock().unwrap() = Some("missing".to_string());

    let error = services
        .subagents
        .spawn(SpawnSubagentRequest {
            caller: parent.clone(),
            task_name: "research".to_string(),
            prompt: "Review".to_string(),
        })
        .await
        .unwrap_err();
    let children = services
        .sessions
        .list_subagent_tree_records(parent.session_id, Some(100))
        .await
        .unwrap();

    assert!(error.is_code(talos_error::ErrorCode::ResourceNotFound));
    assert!(children.is_empty());
}

#[tokio::test]
async fn spawn_rejects_an_initial_prompt_over_the_hard_byte_cap() {
    let services = test_services().await;
    let parent = create_parent(&services).await;

    let error = services
        .subagents
        .spawn(SpawnSubagentRequest {
            caller: parent,
            task_name: "research".to_string(),
            prompt: "x".repeat(32 * 1024 + 1),
        })
        .await
        .unwrap_err();

    assert!(error.is_code(talos_error::ErrorCode::InvalidRequest));
}

#[tokio::test]
async fn spawn_supports_recursive_child_parent() {
    let services = test_services().await;
    let parent = create_parent(&services).await;
    let child_result = services
        .subagents
        .spawn(SpawnSubagentRequest {
            caller: parent,
            task_name: "research".to_string(),
            prompt: "Review the patch".to_string(),
        })
        .await
        .unwrap();
    let child = child_result.subagent;

    let nested = services
        .subagents
        .spawn(SpawnSubagentRequest {
            caller: AgentCaller {
                session_id: child.session_id.clone(),
                turn_id: child_result.turn.turn_id,
                agent_id: "default".to_string(),
            },
            task_name: "nested".to_string(),
            prompt: "Review recursively".to_string(),
        })
        .await
        .unwrap()
        .subagent;

    assert_eq!(nested.path.as_str(), "/root/research/nested");
    assert_eq!(nested.parent.session_id, child.session_id);
    assert_eq!(services.executor.requests.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn spawn_removes_child_when_turn_start_fails() {
    let services = test_services().await;
    let parent = create_parent(&services).await;
    *services.executor.fail_next.lock().unwrap() = true;

    let error = services
        .subagents
        .spawn(SpawnSubagentRequest {
            caller: parent.clone(),
            task_name: "research".to_string(),
            prompt: "Review the patch".to_string(),
        })
        .await
        .unwrap_err();

    let rows = services
        .storage
        .query(
            "SELECT session_key FROM sessions WHERE parent_session_id = ?",
            &[parent.session_id.as_str()],
        )
        .await
        .unwrap();
    assert!(error.is_code(talos_error::ErrorCode::Conflict));
    assert!(rows.is_empty());
}
