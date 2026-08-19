use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::test_support::run_test_migrations;
use crate::SessionMessageQueueService;
use async_trait::async_trait;
use chrono::Utc;
use talos_coordinator::SessionRuntime;
use talos_error::{TalosError, TalosResult};
use talos_storage::{SqliteStorage, Storage};
use talos_store::profile::{DatabaseProfileStore, ProfileStore};
use talos_store::project::MockProjectStore;
use talos_store::session::{
    ActiveTurnStatus, ActiveTurnStore, ActiveTurnUpdate, DatabaseSessionStore,
    FileSessionArtifactStore, MessageIdempotencyStore, SessionArtifactStore, SessionQuery,
    SessionStore, TalosTurn, TimelineEventStore,
};
use talos_types::agent_dispatch::{
    AgentDispatchInput, AgentDispatchInputParams, AgentDispatchParams, DispatchBinding,
};
use talos_types::talos::{
    AgentEvent, AgentEventPayload, AgentEventType, RunItem, StopReason, TalosContext,
    TalosInputItem, TalosItemDeltaEvent, TalosItemStartedEvent, TalosProvider, TalosSourceType,
    TalosTimelineContent, TalosTimelineDelta, TalosTimelineItem, TalosTimelineItemKind,
    TalosTimelineItemStatus, TalosTimelinePhase, TalosTurnCompletedEvent, TalosTurnErrorEvent,
    TalosTurnStartedEvent, TokenUsage,
};
use talos_types::{
    AgentCaller, AgentTarget, AgentTurnRef, DynamicToolResultParams, FollowUpSubagentRequest,
    InterruptSubagentRequest, ListSubagentsRequest, ModelRef, ModelSelection, PendingUserAction,
    SendSubagentMessageRequest, SessionConf, SpawnSubagentRequest, SubagentOutcome, SubagentRef,
    TalosConfig, TimelineUserActionEvent, UserActionParams, UserActionState, WaitSubagentRequest,
};
use tokio::sync::mpsc;

use super::SubagentService;
use crate::session::{CreateSessionParams, SessionService};
use crate::turn::{TurnExecutor, TurnService};
use crate::{SessionHistoryService, TimelineEventRetentionConfig, TimelineEventService};

#[path = "list_tests.rs"]
mod list_tests;

#[derive(Default)]
struct RecordingTurnExecutor {
    requests: Mutex<Vec<AgentDispatchParams>>,
    inputs: Mutex<Vec<AgentDispatchInputParams>>,
    input_error: Mutex<Option<TalosError>>,
    fail_next: Mutex<bool>,
    interactive_next: Mutex<bool>,
    silent_next: Mutex<bool>,
    held_turns: Mutex<Vec<(AgentDispatchParams, mpsc::Sender<AgentEvent>)>>,
    hold_next: Mutex<usize>,
    rejected_agent_id: Mutex<Option<String>>,
    artifact_workspace: Mutex<Option<PathBuf>>,
    artifact_sequences: Mutex<HashMap<String, u64>>,
}

#[async_trait]
impl TurnExecutor for RecordingTurnExecutor {
    async fn validate_agent_id(&self, agent_id: &str) -> TalosResult<()> {
        if self.rejected_agent_id.lock().unwrap().as_deref() == Some(agent_id) {
            return Err(TalosError::public(
                talos_error::ErrorCode::ResourceNotFound,
                format!("agent {agent_id}"),
            ));
        }
        Ok(())
    }

    async fn start_turn_stream(
        &self,
        request: AgentDispatchParams,
    ) -> TalosResult<mpsc::Receiver<AgentEvent>> {
        self.requests.lock().unwrap().push(request.clone());
        if std::mem::take(&mut *self.fail_next.lock().unwrap()) {
            return Err(TalosError::public(
                talos_error::ErrorCode::Conflict,
                "start failed".to_string(),
            ));
        }
        let (tx, rx) = mpsc::channel(2);
        if std::mem::take(&mut *self.silent_next.lock().unwrap()) {
            self.held_turns.lock().unwrap().push((request, tx));
            return Ok(rx);
        }
        let interactive = std::mem::take(&mut *self.interactive_next.lock().unwrap());
        let hold = {
            let mut remaining = self.hold_next.lock().unwrap();
            if *remaining == 0 {
                false
            } else {
                *remaining -= 1;
                true
            }
        };
        let first_payload = if interactive || hold {
            AgentEventPayload::TurnStarted(TalosTurnStartedEvent {
                request_id: "request-child".to_string(),
                turn_id: request.turn_id.clone(),
                started_at_ms: 1,
            })
        } else {
            let artifact_workspace = self.artifact_workspace.lock().unwrap().clone();
            if let Some(artifact_workspace) = artifact_workspace {
                let sequence = {
                    let mut sequences = self.artifact_sequences.lock().unwrap();
                    let sequence = sequences
                        .entry(request.session.context.session_id.clone())
                        .or_default();
                    *sequence += 1;
                    *sequence
                };
                FileSessionArtifactStore::default()
                    .append_turn(
                        &artifact_workspace,
                        &TalosTurn {
                            turn_id: format!("artifact-{}", request.turn_id),
                            message_id: "message-child".to_string(),
                            session_id: request.session.context.session_id.clone(),
                            seq: sequence,
                            timestamp: Utc::now(),
                            items: vec![RunItem::OutputText {
                                text: "AGENT_CHILD_OK".to_string(),
                            }],
                            usage: TokenUsage::default(),
                            stop_reason: StopReason::EndTurn,
                        },
                    )
                    .await?;
            }
            AgentEventPayload::TurnCompleted(TalosTurnCompletedEvent {
                request_id: "request-child".to_string(),
                turn_id: request.turn_id.clone(),
                completed_at_ms: 1,
                stop_reason: talos_types::talos::StopReason::EndTurn,
                usage: TokenUsage::default(),
                metrics: None,
                artifact_seq: Some(1),
            })
        };
        tx.send(AgentEvent {
            event_seq: None,
            event_id: None,
            created_at_ms: None,
            event_type: if interactive || hold {
                AgentEventType::TurnStarted
            } else {
                AgentEventType::TurnCompleted
            },
            turn_id: request.turn_id.clone(),
            message_id: "message-child".to_string(),
            session_id: request.session.context.session_id.clone(),
            sequence: 0,
            payload: first_payload,
        })
        .await
        .unwrap();
        if hold {
            self.held_turns.lock().unwrap().push((request, tx));
            return Ok(rx);
        }
        if interactive {
            tx.send(AgentEvent {
                event_seq: None,
                event_id: None,
                created_at_ms: None,
                event_type: AgentEventType::ItemDelta,
                turn_id: request.turn_id.clone(),
                message_id: "message-child".to_string(),
                session_id: request.session.context.session_id,
                sequence: 1,
                payload: AgentEventPayload::ItemDelta(TalosItemDeltaEvent {
                    request_id: "request-child".to_string(),
                    turn_id: request.turn_id,
                    item_id: "item-tool".to_string(),
                    delta: TalosTimelineDelta::UserAction {
                        event: TimelineUserActionEvent::Requested {
                            action: PendingUserAction {
                                action_id: "ua-child".to_string(),
                                call_id: "call-1".to_string(),
                                item_id: Some("item-tool".to_string()),
                                state: UserActionState::Pending,
                                params: UserActionParams::DynamicToolResult(
                                    DynamicToolResultParams {
                                        tool_name: "ask_user".to_string(),
                                    },
                                ),
                            },
                        },
                    },
                }),
            })
            .await
            .unwrap();
        }
        Ok(rx)
    }

    async fn submit_input(&self, params: AgentDispatchInputParams) -> TalosResult<()> {
        self.inputs.lock().unwrap().push(params);
        if let Some(error) = self.input_error.lock().unwrap().take() {
            return Err(error);
        }
        Ok(())
    }
}

#[derive(Default)]
struct RecordingSessionRuntime {
    cancellations: Mutex<Vec<(String, String, Option<String>)>>,
}

#[async_trait]
impl SessionRuntime for RecordingSessionRuntime {
    async fn compact_session(
        &self,
        _params: talos_coordinator::AgentSessionCompactionParams,
    ) -> TalosResult<TokenUsage> {
        unreachable!("not used by spawn")
    }

    async fn clear_session(&self, _session: talos_types::TalosSession) -> TalosResult<()> {
        unreachable!("not used by spawn")
    }

    async fn close_session(&self, _session: talos_types::TalosSession) -> TalosResult<()> {
        unreachable!("not used by spawn")
    }

    async fn prepare_delete_session(&self, _session: talos_types::TalosSession) -> TalosResult<()> {
        unreachable!("not used by spawn")
    }

    async fn cancel_active_turn(
        &self,
        params: talos_types::CancelActiveTurnParams,
    ) -> TalosResult<()> {
        self.cancellations.lock().unwrap().push((
            params.session.context.session_id,
            params.turn_id,
            match params.binding {
                DispatchBinding::Routed => None,
                DispatchBinding::Exact { agent_id } => Some(agent_id),
            },
        ));
        Ok(())
    }
}

struct TestServices {
    subagents: SubagentService,
    sessions: Arc<SessionService>,
    store: Arc<DatabaseSessionStore>,
    storage: Arc<SqliteStorage>,
    executor: Arc<RecordingTurnExecutor>,
    runtime: Arc<RecordingSessionRuntime>,
    timeline: Arc<TimelineEventService>,
    user_workspace: PathBuf,
    _workspace: tempfile::TempDir,
}

async fn test_services() -> TestServices {
    test_services_uninitialized().await
}

async fn test_services_uninitialized() -> TestServices {
    let storage = Arc::new(SqliteStorage::new(":memory:").await.unwrap());
    run_test_migrations(storage.pool()).await;
    let store = Arc::new(DatabaseSessionStore::new(storage.clone()).await.unwrap());
    let workspace = tempfile::tempdir().unwrap();
    let mut config = test_config();
    config.agent.kernel.workspace_dir = workspace.path().to_string_lossy().to_string();
    let sessions = Arc::new(SessionService::new(
        store.clone(),
        Arc::new(MockProjectStore::new()),
        config.clone(),
    ));
    let event_service = Arc::new(TimelineEventService::new(
        store.clone() as Arc<dyn TimelineEventStore>,
        store.clone() as Arc<dyn ActiveTurnStore>,
        store.clone() as Arc<dyn talos_store::session::UserActionStore>,
        TimelineEventRetentionConfig::default(),
    ));
    let executor = Arc::new(RecordingTurnExecutor::default());
    let user_workspace =
        talos_config::resolve_effective_kernel_workspace(&config.agent.kernel, "user-1")
            .unwrap()
            .user_workspace_dir;
    *executor.artifact_workspace.lock().unwrap() = Some(user_workspace.clone());
    let profile_store: Arc<dyn ProfileStore> =
        Arc::new(DatabaseProfileStore::new(storage.pool().clone()));
    let runtime = Arc::new(RecordingSessionRuntime::default());
    let turns = Arc::new(TurnService::new(
        executor.clone(),
        event_service.clone(),
        store.clone() as Arc<dyn MessageIdempotencyStore>,
        profile_store,
        sessions.clone(),
        runtime.clone(),
        SessionMessageQueueService::new(10).expect("queue service should initialize"),
        config,
    ));
    let history = SessionHistoryService::new(
        Arc::new(FileSessionArtifactStore::default()),
        event_service.clone(),
    );
    let subagents = SubagentService::new(sessions.clone(), turns, event_service.clone(), history);
    TestServices {
        subagents,
        sessions,
        store,
        storage,
        executor,
        runtime,
        timeline: event_service,
        user_workspace,
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
    config.agent.kernel.timeline_events.first_event_timeout_ms = 25;
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
            context: TalosContext {
                source_type: TalosSourceType::System,
                provider: TalosProvider::System,
                user_id: "user-1".to_string(),
                message_id: "message-parent".to_string(),
                trace_id: "trace-parent".to_string(),
                channel_id: None,
                thread_id: None,
                project_root: None,
                environment: None,
            },
            target_agent: agent_id.to_string(),
            model_selection: Some(ModelSelection::without_reasoning(ModelRef {
                provider: "anthropic".to_string(),
                id: "test-model".to_string(),
            })),
            conf: Some(SessionConf::default()),
            workspace_id: None,
            project_id: None,
            current_directory: None,
        })
        .await
        .unwrap();
    let parent_ref = AgentCaller {
        session_id: parent.context.session_id,
        turn_id: turn_id.to_string(),
        agent_id: agent_id.to_string(),
    };
    services
        .store
        .upsert_active_turn(ActiveTurnUpdate {
            session_id: parent_ref.session_id.clone(),
            turn_id: parent_ref.turn_id.clone(),
            message_id: Some("message-parent".to_string()),
            status: ActiveTurnStatus::Running,
            last_event_id: None,
            current_action_id: None,
            updated_at: 1,
        })
        .await
        .unwrap();
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
    let child = services
        .sessions
        .create_subagent_session(
            CreateSessionParams {
                session_id: Some(session_id.to_string()),
                context: TalosContext {
                    source_type: TalosSourceType::System,
                    provider: TalosProvider::System,
                    user_id: "user-1".to_string(),
                    message_id: "message-child".to_string(),
                    trace_id: "trace-child".to_string(),
                    channel_id: None,
                    thread_id: None,
                    project_root: None,
                    environment: None,
                },
                target_agent: "reviewer".to_string(),
                model_selection: Some(ModelSelection::without_reasoning(ModelRef {
                    provider: "anthropic".to_string(),
                    id: "test-model".to_string(),
                })),
                conf: None,
                workspace_id: None,
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
        .unwrap();
    let turn_id = format!("turn-{session_id}");
    services
        .timeline
        .record_talos_event(AgentEvent {
            event_seq: None,
            event_id: None,
            created_at_ms: None,
            event_type: AgentEventType::TurnStarted,
            turn_id: turn_id.clone(),
            message_id: "message-child".to_string(),
            session_id: child.context.session_id.clone(),
            sequence: 0,
            payload: AgentEventPayload::TurnStarted(TalosTurnStartedEvent {
                request_id: "request-child".to_string(),
                turn_id: turn_id.clone(),
                started_at_ms: 2,
            }),
        })
        .await
        .unwrap();
    (
        SubagentRef {
            path: format!("/root/{}", session_id.replace('-', "_"))
                .parse()
                .unwrap(),
            session_id: child.context.session_id,
            parent: parent.turn_ref(),
        },
        turn_id,
    )
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
            request.binding,
            request.input,
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
            DispatchBinding::Exact {
                agent_id: "default".to_string(),
            },
            vec![TalosInputItem::Text {
                text: "Review the patch".to_string(),
            }],
            AgentTurnRef {
                session_id: record.id.clone(),
                turn_id: services.executor.requests.lock().unwrap()[0]
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
    services
        .store
        .clear_active_turn(&parent.session_id)
        .await
        .unwrap();
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
    *services.executor.rejected_agent_id.lock().unwrap() = Some("missing".to_string());

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
    let child = services
        .subagents
        .spawn(SpawnSubagentRequest {
            caller: parent,
            task_name: "research".to_string(),
            prompt: "Review the patch".to_string(),
        })
        .await
        .unwrap()
        .subagent;
    services
        .store
        .upsert_active_turn(ActiveTurnUpdate {
            session_id: child.session_id.clone(),
            turn_id: "turn-child".to_string(),
            message_id: Some("message-child".to_string()),
            status: ActiveTurnStatus::Running,
            last_event_id: None,
            current_action_id: None,
            updated_at: 2,
        })
        .await
        .unwrap();

    let nested = services
        .subagents
        .spawn(SpawnSubagentRequest {
            caller: AgentCaller {
                session_id: child.session_id.clone(),
                turn_id: "turn-child".to_string(),
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

#[tokio::test]
async fn wait_returns_an_already_completed_child() {
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

    let result = services
        .subagents
        .wait(wait_request(parent, &spawned.subagent, 0))
        .await
        .unwrap();

    assert!(matches!(
        result.outcome,
        SubagentOutcome::Completed {
            output,
            truncated: false,
            ..
        } if output == "AGENT_CHILD_OK"
    ));
}

#[tokio::test]
async fn send_message_dispatches_to_the_exact_running_child_turn() {
    let services = test_services().await;
    let parent = create_parent(&services).await;
    let (child, turn_id) = create_running_child(&services, parent.clone()).await;

    let result = services
        .subagents
        .send_message(SendSubagentMessageRequest {
            caller: parent,
            target: target(&child),
            message: "Focus on tests".to_string(),
        })
        .await
        .unwrap();
    let inputs = services.executor.inputs.lock().unwrap();

    assert!(result.accepted);
    assert_eq!(result.subagent, child);
    assert_eq!(result.agent_id, "reviewer");
    assert_eq!(result.turn.turn_id, turn_id);
    assert!(matches!(
        &inputs[0],
        AgentDispatchInputParams {
            binding: DispatchBinding::Exact { agent_id },
            turn_id: dispatched_turn_id,
            input: AgentDispatchInput::Message { input },
            ..
        } if agent_id == "reviewer"
            && dispatched_turn_id == &turn_id
            && input == &vec![TalosInputItem::Text {
                text: "Focus on tests".to_string(),
            }]
    ));
}

#[tokio::test]
async fn send_message_rejects_a_non_running_child() {
    let services = test_services().await;
    let parent = create_parent(&services).await;
    let spawned = services
        .subagents
        .spawn(SpawnSubagentRequest {
            caller: parent.clone(),
            task_name: "research".to_string(),
            prompt: "Review".to_string(),
        })
        .await
        .unwrap();

    let error = services
        .subagents
        .send_message(SendSubagentMessageRequest {
            caller: parent,
            target: target(&spawned.subagent),
            message: "Too late".to_string(),
        })
        .await
        .unwrap_err();

    assert!(error.is_code(talos_error::ErrorCode::Conflict));
    assert!(services.executor.inputs.lock().unwrap().is_empty());
}

#[tokio::test]
async fn send_message_propagates_a_closed_runtime_input() {
    let services = test_services().await;
    let parent = create_parent(&services).await;
    let (child, _turn_id) = create_running_child(&services, parent.clone()).await;
    *services.executor.input_error.lock().unwrap() = Some(TalosError::public(
        talos_error::ErrorCode::ResourceNotFound,
        "message input closed".to_string(),
    ));

    let error = services
        .subagents
        .send_message(SendSubagentMessageRequest {
            caller: parent,
            target: target(&child),
            message: "Too late".to_string(),
        })
        .await
        .unwrap_err();

    assert!(error.is_code(talos_error::ErrorCode::ResourceNotFound));
    assert_eq!(error.diagnostic_message(), "message input closed");
}

#[tokio::test]
async fn wait_timeout_returns_the_running_child_as_pending() {
    let services = test_services().await;
    let parent = create_parent(&services).await;
    let (child, _turn_id) = create_running_child(&services, parent.clone()).await;

    let result = services
        .subagents
        .wait(wait_request(parent, &child, 1))
        .await
        .unwrap();

    assert_eq!(result.subagent, child);
    assert_eq!(result.agent_id, "reviewer");
    assert!(matches!(result.outcome, SubagentOutcome::Running { .. }));
}

#[tokio::test]
async fn wait_resolves_a_relative_direct_child_target() {
    let services = test_services().await;
    let parent = create_parent(&services).await;
    let (child, _) = create_running_child(&services, parent.clone()).await;

    let result = services
        .subagents
        .wait(WaitSubagentRequest {
            caller: parent,
            target: AgentTarget::DirectChild {
                name: child.path.name().to_string(),
            },
            timeout_ms: 0,
        })
        .await
        .unwrap();

    assert_eq!(result.subagent, child);
    assert!(matches!(result.outcome, SubagentOutcome::Running { .. }));
}

#[tokio::test]
async fn wait_pending_snapshot_exposes_bounded_latest_commentary() {
    let services = test_services().await;
    let parent = create_parent(&services).await;
    let (child, turn_id) = create_running_child(&services, parent.clone()).await;
    services
        .timeline
        .record_talos_event(AgentEvent {
            event_seq: None,
            event_id: None,
            created_at_ms: None,
            event_type: AgentEventType::ItemStarted,
            turn_id: turn_id.clone(),
            message_id: "message-child".to_string(),
            session_id: child.session_id.clone(),
            sequence: 1,
            payload: AgentEventPayload::ItemStarted(TalosItemStartedEvent {
                request_id: "request-child".to_string(),
                turn_id: turn_id.clone(),
                item: TalosTimelineItem {
                    item_id: "commentary-1".to_string(),
                    parent_item_id: None,
                    kind: TalosTimelineItemKind::AssistantText,
                    phase: TalosTimelinePhase::Commentary,
                    status: TalosTimelineItemStatus::Running,
                    title: None,
                    started_at_ms: 2,
                    completed_at_ms: None,
                    content: TalosTimelineContent::Text {
                        text: format!("reviewing\n{}", "x".repeat(3_000)),
                    },
                },
            }),
        })
        .await
        .unwrap();

    let result = services
        .subagents
        .wait(wait_request(parent, &child, 0))
        .await
        .unwrap();
    let update = result.latest_update.as_ref().unwrap();

    assert_eq!(update.kind, talos_types::SubagentUpdateKind::Commentary);
    assert!(update.text.starts_with("reviewing "));
    assert_eq!(update.text.len(), 2 * 1024);
    assert!(update.truncated);
}

#[tokio::test]
async fn failed_outcome_exposes_a_bounded_preview_with_truncation_marker() {
    let services = test_services().await;
    let parent = create_parent(&services).await;
    let (child, turn_id) = create_running_child(&services, parent.clone()).await;
    services
        .timeline
        .record_talos_event(AgentEvent {
            event_seq: None,
            event_id: None,
            created_at_ms: None,
            event_type: AgentEventType::TurnError,
            turn_id: turn_id.clone(),
            message_id: "message-child".to_string(),
            session_id: child.session_id.clone(),
            sequence: 2,
            payload: AgentEventPayload::TurnError(TalosTurnErrorEvent {
                request_id: "request-child".to_string(),
                turn_id: turn_id.clone(),
                error: talos_error::ApiError {
                    code: talos_error::ErrorCode::InternalError,
                    message: "failed ".repeat(400),
                },
                failed_at_ms: 2,
            }),
        })
        .await
        .unwrap();

    let result = services
        .subagents
        .wait(wait_request(parent, &child, 0))
        .await
        .unwrap();

    assert!(matches!(
        &result.outcome,
        SubagentOutcome::Failed {
            error,
            truncated: true,
            ..
        } if error.len() == 2 * 1024
    ));
}

#[tokio::test]
async fn wait_rejects_a_child_owned_by_another_parent() {
    let services = test_services().await;
    let parent = create_parent(&services).await;
    let (child, _turn_id) = create_running_child(&services, parent.clone()).await;

    let error = services
        .subagents
        .wait(wait_request(
            AgentCaller {
                session_id: "other-parent".to_string(),
                turn_id: parent.turn_id,
                agent_id: parent.agent_id,
            },
            &child,
            0,
        ))
        .await
        .unwrap_err();

    assert!(error.is_code(talos_error::ErrorCode::ResourceNotFound));
}

#[tokio::test]
async fn interrupt_delegates_without_fabricating_a_cancelling_state() {
    let services = test_services().await;
    let parent = create_parent(&services).await;
    let (child, turn_id) = create_running_child(&services, parent.clone()).await;

    let result = services
        .subagents
        .interrupt(InterruptSubagentRequest {
            caller: parent,
            target: target(&child),
        })
        .await
        .unwrap();

    assert_eq!(
        result.outcome,
        SubagentOutcome::Running {
            turn_id: turn_id.clone(),
        }
    );
    assert_eq!(
        services.runtime.cancellations.lock().unwrap().as_slice(),
        &[(child.session_id, turn_id, Some("reviewer".to_string()))]
    );
}

#[tokio::test]
async fn concurrent_waiters_observe_live_completion() {
    let services = test_services().await;
    let parent = create_parent(&services).await;
    let (child, turn_id) = create_running_child(&services, parent.clone()).await;
    let request = WaitSubagentRequest {
        caller: parent,
        target: target(&child),
        timeout_ms: 1_000,
    };
    let timeline = services.timeline.clone();
    let session_id = child.session_id;
    let artifact_workspace = services.user_workspace.clone();
    let complete = tokio::spawn(async move {
        tokio::task::yield_now().await;
        FileSessionArtifactStore::default()
            .append_turn(
                Path::new(&artifact_workspace),
                &TalosTurn {
                    turn_id: format!("artifact-{turn_id}"),
                    message_id: "message-child".to_string(),
                    session_id: session_id.clone(),
                    seq: 1,
                    timestamp: Utc::now(),
                    items: vec![RunItem::OutputText {
                        text: "AGENT_CHILD_OK".to_string(),
                    }],
                    usage: TokenUsage::default(),
                    stop_reason: StopReason::EndTurn,
                },
            )
            .await
            .unwrap();
        timeline
            .record_talos_event(AgentEvent {
                event_seq: None,
                event_id: None,
                created_at_ms: None,
                event_type: AgentEventType::TurnCompleted,
                turn_id: turn_id.clone(),
                message_id: "message-child".to_string(),
                session_id,
                sequence: 2,
                payload: AgentEventPayload::TurnCompleted(TalosTurnCompletedEvent {
                    request_id: "request-child".to_string(),
                    turn_id,
                    completed_at_ms: 2,
                    stop_reason: talos_types::talos::StopReason::EndTurn,
                    usage: TokenUsage::default(),
                    metrics: None,
                    artifact_seq: None,
                }),
            })
            .await
            .unwrap();
    });

    let (first, second) = tokio::join!(
        services.subagents.wait(request.clone()),
        services.subagents.wait(request)
    );
    complete.await.unwrap();

    assert!(matches!(
        first.unwrap().outcome,
        SubagentOutcome::Completed { .. }
    ));
    assert!(matches!(
        second.unwrap().outcome,
        SubagentOutcome::Completed { .. }
    ));
}

#[tokio::test]
async fn interrupt_is_idempotent_for_a_terminal_child() {
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
    let child = spawned.subagent;

    let result = services
        .subagents
        .interrupt(InterruptSubagentRequest {
            caller: parent,
            target: target(&child),
        })
        .await
        .unwrap();

    assert!(matches!(result.outcome, SubagentOutcome::Completed { .. }));
    assert!(services.runtime.cancellations.lock().unwrap().is_empty());
}

#[tokio::test]
async fn every_existing_child_operation_rejects_a_stale_caller_turn() {
    let services = test_services().await;
    let parent = create_parent(&services).await;
    let spawned = services
        .subagents
        .spawn(SpawnSubagentRequest {
            caller: parent.clone(),
            task_name: "research".to_string(),
            prompt: "Review".to_string(),
        })
        .await
        .unwrap();
    let child = spawned.subagent;
    services
        .store
        .clear_active_turn(&parent.session_id)
        .await
        .unwrap();

    let wait = services
        .subagents
        .wait(wait_request(parent.clone(), &child, 0))
        .await;
    let interrupt = services
        .subagents
        .interrupt(InterruptSubagentRequest {
            caller: parent.clone(),
            target: target(&child),
        })
        .await;
    let follow_up = services
        .subagents
        .follow_up(FollowUpSubagentRequest {
            caller: parent.clone(),
            target: target(&child),
            prompt: "continue".to_string(),
        })
        .await;
    let list = services
        .subagents
        .list(ListSubagentsRequest { caller: parent })
        .await;

    assert!(wait
        .as_ref()
        .is_err_and(|error| error.is_code(talos_error::ErrorCode::InvalidRequest)));
    assert!(interrupt
        .as_ref()
        .is_err_and(|error| error.is_code(talos_error::ErrorCode::InvalidRequest)));
    assert!(follow_up
        .as_ref()
        .is_err_and(|error| error.is_code(talos_error::ErrorCode::InvalidRequest)));
    assert!(list
        .as_ref()
        .is_err_and(|error| error.is_code(talos_error::ErrorCode::InvalidRequest)));
}

#[tokio::test]
async fn follow_up_reuses_the_child_agent_binding() {
    let services = test_services().await;
    let parent = create_parent(&services).await;
    let child = services
        .subagents
        .spawn(SpawnSubagentRequest {
            caller: parent.clone(),
            task_name: "research".to_string(),
            prompt: "Review the patch".to_string(),
        })
        .await
        .unwrap()
        .subagent;

    let result = services
        .subagents
        .follow_up(FollowUpSubagentRequest {
            caller: parent,
            target: target(&child),
            prompt: "Check the tests too".to_string(),
        })
        .await
        .unwrap();

    let requests = services.executor.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[1].binding,
        DispatchBinding::Exact {
            agent_id: "default".to_string(),
        }
    );
    assert_eq!(
        requests[1].input,
        vec![TalosInputItem::Text {
            text: "Check the tests too".to_string(),
        }]
    );
    assert!(matches!(
        result.outcome,
        SubagentOutcome::Completed { turn_id, .. } if turn_id == requests[1].turn_id
    ));
}

#[tokio::test]
async fn follow_up_rejects_a_running_child() {
    let services = test_services().await;
    let parent = create_parent(&services).await;
    let (child, _) = create_running_child(&services, parent.clone()).await;

    let error = services
        .subagents
        .follow_up(FollowUpSubagentRequest {
            caller: parent,
            target: target(&child),
            prompt: "Continue".to_string(),
        })
        .await
        .unwrap_err();

    assert!(error.is_code(talos_error::ErrorCode::ActiveTurnRunning));
    assert!(services.executor.requests.lock().unwrap().is_empty());
}
