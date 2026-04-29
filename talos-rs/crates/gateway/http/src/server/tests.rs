use super::*;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, Request, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde_json::Value;
use talos_auth::{
    AuthCacheConfig, AuthManager, AuthService, CreateUserTokenRequest, InMemoryUserStore,
    InMemoryUserTokenStore, JwtAuth, RequestIdentity, UserService,
};
use talos_gateway::GatewayRequest;
use talos_llm_provider::{ChatRequest, ChatResponse, ChatStream, LlmProvider, Usage};
use talos_session_store::{
    ActiveTurnRecord, ActiveTurnStatus, ActiveTurnStore, ActiveTurnUpdate, IdempotencyRecord,
    IdempotencyReservation, IdempotencyStatus, MessageCommandAck, MessageIdempotencyStore,
    NewSessionEvent, PendingDynamicToolResult, PendingToolApproval, PendingUserAction,
    SessionEventId, SessionEventRecord, SessionEventRetentionConfig, SessionEventService,
    SessionEventStore,
};
use talos_types::talos::{
    StopReason, TalosContext, TalosEvent, TalosEventPayload, TalosEventType, TalosInputItem,
    TalosMessageCompletedEvent, TalosMessageOutputDeltaEvent, TalosMessageRequest,
    TalosMessageStartedEvent, TalosMessageToolCallExecutingEvent,
    TalosMessageToolCallProgressDeltaEvent, TalosMessageToolCallQueuedEvent, TalosOutputItem,
    TalosProvider, TalosReasoning, TalosSourceType, TalosToolProgressKind, TalosToolStreamKey,
    TalosUsage,
};
use talos_types::SessionQuery;
use tower::util::ServiceExt;

use crate::server::admin_users::{
    delete_admin_session_handler, get_admin_llm_handler, list_admin_sessions_handler,
};
use crate::server::events::{get_active_turn_handler, subscribe_session_events_handler};
use crate::server::messages::{
    retry_message_attempt_handler, session_message_handler, submit_message_command,
};
use crate::server::user_profile::{
    create_user_token_handler, list_admin_users_handler, list_user_tokens_handler,
};

struct MockGateway;
struct SilentGateway;
struct MockSessionService;
struct MockLlmProvider;

fn test_model_info() -> ModelInfo {
    ModelInfo {
        id: "qwen-plus".to_string(),
        name: "Qwen Plus".to_string(),
        provider: "bailian".to_string(),
        context_window: 32_768,
        reasoning: true,
        input: vec![talos_types::ModelInputKind::Text],
        cost: talos_types::ModelCostConfig::default(),
        compat: talos_types::ModelCompatConfig::default(),
    }
}

fn test_config() -> talos_types::TalosConfig {
    talos_types::TalosConfig::default()
}

fn isolated_test_config() -> (talos_types::TalosConfig, std::path::PathBuf) {
    let workspace_dir =
        std::env::temp_dir().join(format!("talos-http-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_dir).expect("workspace dir should exist");
    let mut config = test_config();
    config.agent.kernel.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();
    (config, workspace_dir)
}

fn test_state(auth_disabled: bool) -> Arc<HttpApiState> {
    let (auth_service, user_service) = test_auth_services();
    let event_store = Arc::new(RecordingEventStore::default());
    Arc::new(HttpApiState::new(
        Arc::new(MockGateway),
        Arc::new(MockSessionService),
        event_service_from_store(event_store.clone()),
        event_store,
        test_config(),
        Arc::new(MockLlmProvider),
        auth_service,
        user_service,
        "test-secret".to_string(),
        auth_disabled,
        vec![test_model_info()],
    ))
}

fn test_state_with_config(
    config: talos_types::TalosConfig,
    auth_disabled: bool,
) -> Arc<HttpApiState> {
    let (auth_service, user_service) = test_auth_services();
    let event_store = Arc::new(RecordingEventStore::default());
    Arc::new(HttpApiState::new(
        Arc::new(MockGateway),
        Arc::new(MockSessionService),
        event_service_from_store(event_store.clone()),
        event_store,
        config,
        Arc::new(MockLlmProvider),
        auth_service,
        user_service,
        "test-secret".to_string(),
        auth_disabled,
        vec![test_model_info()],
    ))
}

fn test_event_service() -> Arc<SessionEventService> {
    let store = Arc::new(RecordingEventStore::default());
    event_service_from_store(store)
}

fn event_service_from_store(store: Arc<RecordingEventStore>) -> Arc<SessionEventService> {
    let event_store: Arc<dyn SessionEventStore> = store.clone();
    let active_turn_store: Arc<dyn ActiveTurnStore> = store;
    Arc::new(SessionEventService::new(
        event_store,
        active_turn_store,
        SessionEventRetentionConfig::default(),
    ))
}

fn test_state_with_event_store(
    auth_disabled: bool,
    store: Arc<RecordingEventStore>,
) -> Arc<HttpApiState> {
    test_state_with_gateway_event_store_config(
        Arc::new(MockGateway),
        auth_disabled,
        store,
        test_config(),
    )
}

fn test_state_with_gateway_event_store_config(
    gateway: Arc<dyn Gateway>,
    auth_disabled: bool,
    store: Arc<RecordingEventStore>,
    config: talos_types::TalosConfig,
) -> Arc<HttpApiState> {
    let (auth_service, user_service) = test_auth_services();
    Arc::new(HttpApiState::new(
        gateway,
        Arc::new(MockSessionService),
        event_service_from_store(store.clone()),
        store,
        config,
        Arc::new(MockLlmProvider),
        auth_service,
        user_service,
        "test-secret".to_string(),
        auth_disabled,
        vec![test_model_info()],
    ))
}

#[derive(Default)]
struct RecordingEventStore {
    events: std::sync::Mutex<Vec<SessionEventRecord>>,
    active_turns: std::sync::Mutex<std::collections::HashMap<String, ActiveTurnRecord>>,
    idempotency_records:
        std::sync::Mutex<std::collections::HashMap<(String, String), IdempotencyRecord>>,
}

#[async_trait::async_trait]
impl SessionEventStore for RecordingEventStore {
    async fn append_event(&self, event: NewSessionEvent) -> TalosResult<SessionEventRecord> {
        let mut events = self.events.lock().unwrap();
        let event_seq = (events.len() as i64) + 1;
        let record = SessionEventRecord {
            event_seq,
            event_id: SessionEventId::from_seq(event_seq),
            session_id: event.session_id,
            turn_id: event.turn_id,
            message_id: event.message_id,
            sequence: event.sequence,
            event_type: event.event_type,
            payload: event.payload,
            created_at: event.created_at,
        };
        events.push(record.clone());
        Ok(record)
    }

    async fn list_events_after(
        &self,
        session_id: &str,
        after_event_id: Option<&SessionEventId>,
        limit: usize,
    ) -> TalosResult<Vec<SessionEventRecord>> {
        let after = match after_event_id {
            Some(event_id) => event_id.event_seq()?,
            None => 0,
        };
        Ok(self
            .events
            .lock()
            .unwrap()
            .iter()
            .filter(|event| event.session_id == session_id && event.event_seq > after)
            .take(limit)
            .cloned()
            .collect())
    }

    async fn latest_event_id(&self, session_id: &str) -> TalosResult<Option<SessionEventId>> {
        Ok(self
            .events
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find(|event| event.session_id == session_id)
            .map(|event| event.event_id.clone()))
    }

    async fn oldest_event_id(&self, session_id: &str) -> TalosResult<Option<SessionEventId>> {
        Ok(self
            .events
            .lock()
            .unwrap()
            .iter()
            .find(|event| event.session_id == session_id)
            .map(|event| event.event_id.clone()))
    }

    async fn delete_completed_events_before(
        &self,
        _before_timestamp_ms: i64,
        _limit: usize,
    ) -> TalosResult<u64> {
        Ok(0)
    }

    async fn delete_overflow_events_per_session(
        &self,
        _max_events_per_session: usize,
        _limit: usize,
    ) -> TalosResult<u64> {
        Ok(0)
    }
}

#[async_trait::async_trait]
impl ActiveTurnStore for RecordingEventStore {
    async fn upsert_active_turn(&self, update: ActiveTurnUpdate) -> TalosResult<()> {
        self.active_turns.lock().unwrap().insert(
            update.session_id.clone(),
            ActiveTurnRecord {
                session_id: update.session_id,
                turn_id: update.turn_id,
                message_id: update.message_id,
                status: update.status,
                last_event_id: update.last_event_id,
                pending_user_action: update.pending_user_action,
                updated_at: update.updated_at,
            },
        );
        Ok(())
    }

    async fn get_active_turn(&self, session_id: &str) -> TalosResult<Option<ActiveTurnRecord>> {
        Ok(self.active_turns.lock().unwrap().get(session_id).cloned())
    }

    async fn clear_active_turn(&self, session_id: &str) -> TalosResult<()> {
        self.active_turns.lock().unwrap().remove(session_id);
        Ok(())
    }
}

#[async_trait::async_trait]
impl MessageIdempotencyStore for RecordingEventStore {
    async fn get(
        &self,
        session_id: &str,
        idempotency_key: &str,
    ) -> TalosResult<Option<IdempotencyRecord>> {
        Ok(self
            .idempotency_records
            .lock()
            .unwrap()
            .get(&(session_id.to_string(), idempotency_key.to_string()))
            .cloned())
    }

    async fn reserve(
        &self,
        session_id: &str,
        idempotency_key: &str,
        request_hash: &str,
        turn_id: &str,
        now_ms: i64,
        expires_at_ms: Option<i64>,
    ) -> TalosResult<IdempotencyReservation> {
        let mut records = self.idempotency_records.lock().unwrap();
        let key = (session_id.to_string(), idempotency_key.to_string());
        if let Some(record) = records.get(&key) {
            if record
                .expires_at
                .is_some_and(|expires_at| expires_at <= now_ms)
            {
                records.remove(&key);
            } else {
                return Ok(IdempotencyReservation::Existing(record.clone()));
            }
        }
        let record = IdempotencyRecord {
            session_id: session_id.to_string(),
            idempotency_key: idempotency_key.to_string(),
            request_hash: request_hash.to_string(),
            turn_id: turn_id.to_string(),
            message_id: None,
            status: IdempotencyStatus::Pending,
            ack: None,
            created_at: now_ms,
            updated_at: now_ms,
            expires_at: expires_at_ms,
        };
        records.insert(key, record.clone());
        Ok(IdempotencyReservation::Reserved(record))
    }

    async fn complete(
        &self,
        session_id: &str,
        idempotency_key: &str,
        status: IdempotencyStatus,
        message_id: Option<&str>,
        ack: &MessageCommandAck,
        now_ms: i64,
    ) -> TalosResult<()> {
        let mut records = self.idempotency_records.lock().unwrap();
        let key = (session_id.to_string(), idempotency_key.to_string());
        if let Some(record) = records.get_mut(&key) {
            record.status = status;
            record.message_id = message_id.map(ToString::to_string);
            record.ack = Some(ack.clone());
            record.updated_at = now_ms;
        }
        Ok(())
    }

    async fn mark_stale_pending_failed(
        &self,
        older_than_ms: i64,
        limit: usize,
    ) -> TalosResult<u64> {
        let mut changed = 0;
        let mut records = self.idempotency_records.lock().unwrap();
        for record in records.values_mut() {
            if changed >= limit {
                break;
            }
            if record.status == IdempotencyStatus::Pending && record.updated_at < older_than_ms {
                record.status = IdempotencyStatus::FailedNoFirstEvent;
                changed += 1;
            }
        }
        Ok(changed as u64)
    }
}

fn test_auth_services() -> (Arc<dyn AuthService>, Arc<dyn UserService>) {
    let manager = Arc::new(AuthManager::new(
        Arc::new(InMemoryUserStore::default()),
        Arc::new(InMemoryUserTokenStore::default()),
        Arc::new(JwtAuth::new("test-secret")),
        true,
        AuthCacheConfig::default(),
    ));
    let auth_service: Arc<dyn AuthService> = manager.clone();
    let user_service: Arc<dyn UserService> = manager;
    (auth_service, user_service)
}

#[async_trait::async_trait]
impl LlmProvider for MockLlmProvider {
    fn name(&self) -> &str {
        "mock"
    }

    fn default_model(&self) -> &str {
        "mock-model"
    }

    async fn chat(&self, _request: ChatRequest) -> TalosResult<ChatResponse> {
        let content = if _request.messages.iter().any(|message| {
            message
                .content
                .contains("Respond ONLY with a valid JSON array of file paths.")
        }) {
            "[\"memory/2026-04-04.md\"]".to_string()
        } else if _request.messages.iter().any(|message| {
            message
                .content
                .contains("Respond ONLY with a valid JSON array.")
        }) {
            "[{\"path\":\"memory/2026-04-04.md\",\"from\":1,\"to\":5}]".to_string()
        } else {
            String::new()
        };
        Ok(ChatResponse {
            content,
            reasoning: Vec::new(),
            model: "mock-model".to_string(),
            usage: Usage::default(),
            finish_reason: None,
            tool_calls: None,
        })
    }

    async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        drop(tx);
        Ok(ChatStream { rx })
    }
}

#[async_trait::async_trait]
impl Gateway for MockGateway {
    async fn handle_request(
        &self,
        _request: GatewayRequest,
    ) -> TalosResult<talos_gateway::GatewayResponse> {
        Ok(talos_gateway::GatewayResponse {
            content: "mock content".to_string(),
            output_items: vec![TalosOutputItem::OutputText {
                text: "mock content".to_string(),
            }],
            reasoning: vec![TalosReasoning {
                text: "mock reasoning".to_string(),
            }],
            stop_reason: StopReason::EndTurn,
            usage: TalosUsage::default(),
            success: true,
            model: "qwen-plus".to_string(),
            session_id: "mock-session".to_string(),
        })
    }

    async fn handle_request_stream(
        &self,
        _request: GatewayRequest,
    ) -> TalosResult<tokio::sync::mpsc::Receiver<TalosEvent>> {
        let (tx, rx) = tokio::sync::mpsc::channel(8);
        tokio::spawn(async move {
            let started = TalosEvent {
                event_type: TalosEventType::MessageStarted,
                message_id: "msg_mock".to_string(),
                session_id: "mock-session".to_string(),
                sequence: 0,
                payload: TalosEventPayload::MessageStarted(TalosMessageStartedEvent {
                    request_id: "req_mock".to_string(),
                    model: "qwen-plus".to_string(),
                    usage: TalosUsage::default(),
                    iteration: 0,
                    step_id: "start".to_string(),
                }),
            };
            let _ = tx.send(started).await;

            let output = TalosEvent {
                event_type: TalosEventType::MessageOutputDelta,
                message_id: "msg_mock".to_string(),
                session_id: "mock-session".to_string(),
                sequence: 1,
                payload: TalosEventPayload::MessageOutputDelta(TalosMessageOutputDeltaEvent {
                    request_id: "req_mock".to_string(),
                    delta: "stream only content".to_string(),
                    item_index: 0,
                    iteration: 1,
                    step_id: "step-1".to_string(),
                }),
            };
            let _ = tx.send(output).await;

            let queued = TalosEvent {
                event_type: TalosEventType::MessageToolCallQueued,
                message_id: "msg_mock".to_string(),
                session_id: "mock-session".to_string(),
                sequence: 2,
                payload: TalosEventPayload::MessageToolCallQueued(
                    TalosMessageToolCallQueuedEvent {
                        request_id: "req_mock".to_string(),
                        stream_key: TalosToolStreamKey { round: 0, index: 0 },
                        call_id: "call-1".to_string(),
                        tool_name: "Bash".to_string(),
                        item_index: 1,
                        iteration: 2,
                        step_id: "step-2".to_string(),
                    },
                ),
            };
            let _ = tx.send(queued).await;

            let executing = TalosEvent {
                event_type: TalosEventType::MessageToolCallExecuting,
                message_id: "msg_mock".to_string(),
                session_id: "mock-session".to_string(),
                sequence: 3,
                payload: TalosEventPayload::MessageToolCallExecuting(
                    TalosMessageToolCallExecutingEvent {
                        request_id: "req_mock".to_string(),
                        stream_key: TalosToolStreamKey { round: 0, index: 0 },
                        call_id: "call-1".to_string(),
                        tool_name: "Bash".to_string(),
                        item_index: 2,
                        iteration: 3,
                        step_id: "step-3".to_string(),
                    },
                ),
            };
            let _ = tx.send(executing).await;

            let progress = TalosEvent {
                event_type: TalosEventType::MessageToolCallProgressDelta,
                message_id: "msg_mock".to_string(),
                session_id: "mock-session".to_string(),
                sequence: 4,
                payload: TalosEventPayload::MessageToolCallProgressDelta(
                    TalosMessageToolCallProgressDeltaEvent {
                        request_id: "req_mock".to_string(),
                        stream_key: TalosToolStreamKey { round: 0, index: 0 },
                        call_id: "call-1".to_string(),
                        tool_name: "Bash".to_string(),
                        message: "execution started".to_string(),
                        progress_kind: TalosToolProgressKind::Status,
                        item_index: 3,
                        iteration: 4,
                        step_id: "step-4".to_string(),
                    },
                ),
            };
            let _ = tx.send(progress).await;

            let completed = TalosEvent {
                event_type: TalosEventType::MessageCompleted,
                message_id: "msg_mock".to_string(),
                session_id: "mock-session".to_string(),
                sequence: 5,
                payload: TalosEventPayload::MessageCompleted(TalosMessageCompletedEvent {
                    request_id: "req_mock".to_string(),
                    stop_reason: StopReason::EndTurn,
                    usage: TalosUsage::default(),
                    total_output_items: 4,
                    iteration: 5,
                    step_id: "completed".to_string(),
                    task_count: 1,
                }),
            };
            let _ = tx.send(completed).await;
        });
        Ok(rx)
    }

    async fn start(&self) -> TalosResult<()> {
        Ok(())
    }

    async fn cancel_active_turn(&self, _session_id: &str) -> TalosResult<()> {
        Ok(())
    }
}

#[async_trait::async_trait]
impl Gateway for SilentGateway {
    async fn handle_request(
        &self,
        _request: GatewayRequest,
    ) -> TalosResult<talos_gateway::GatewayResponse> {
        unreachable!("silent gateway is only used by async stream tests")
    }

    async fn handle_request_stream(
        &self,
        _request: GatewayRequest,
    ) -> TalosResult<tokio::sync::mpsc::Receiver<TalosEvent>> {
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            drop(tx);
        });
        Ok(rx)
    }

    async fn start(&self) -> TalosResult<()> {
        Ok(())
    }

    async fn cancel_active_turn(&self, _session_id: &str) -> TalosResult<()> {
        Ok(())
    }
}

#[async_trait::async_trait]
impl SessionService for MockSessionService {
    async fn create_session(
        &self,
        context: TalosContext,
        model: Option<String>,
        conf: Option<talos_types::SessionConf>,
        project_id: Option<String>,
        current_directory: Option<std::path::PathBuf>,
    ) -> TalosResult<talos_types::TalosSession> {
        Ok(talos_types::TalosSession {
            title: "Untitled Session".to_string(),
            user_id: context.user_id.clone(),
            context: talos_types::SessionContext {
                session_id: "mock-session".to_string(),
                workspace_id: None,
                source_type: context.source_type,
                provider: context.provider,
                user_id: context.user_id,
                message_id: context.message_id,
                trace_id: context.trace_id,
                channel_id: context.channel_id,
                thread_id: context.thread_id,
                project_id: project_id.clone(),
                current_directory: current_directory.clone(),
                project_root: context.project_root,
                environment: context.environment,
            },
            conf: conf.unwrap_or_default(),
            project_id,
            current_directory,
            config: {
                let mut config = talos_types::TalosConfig::default();
                let resolved = model.unwrap_or_else(|| "qwen-plus".to_string());
                config.agent.model = resolved.clone();
                config.llm.model = resolved;
                config
            },
            state: talos_types::SessionState::default(),
        })
    }

    async fn list_sessions(
        &self,
        _query: talos_types::SessionQuery,
    ) -> TalosResult<Vec<talos_types::TalosSession>> {
        Ok(Vec::new())
    }

    async fn count_sessions(&self, _query: talos_types::SessionQuery) -> TalosResult<usize> {
        Ok(0)
    }

    async fn get_session(
        &self,
        session_id: &str,
    ) -> TalosResult<Option<talos_types::TalosSession>> {
        if session_id != "mock-session" {
            return Ok(None);
        }
        Ok(Some(talos_types::TalosSession {
            title: "Untitled Session".to_string(),
            user_id: "static-token".to_string(),
            context: talos_types::SessionContext {
                session_id: "mock-session".to_string(),
                workspace_id: None,
                source_type: TalosSourceType::Http,
                provider: TalosProvider::Http,
                user_id: "static-token".to_string(),
                message_id: "message-1".to_string(),
                trace_id: "trace-1".to_string(),
                channel_id: Some("test-channel".to_string()),
                thread_id: None,
                project_id: None,
                current_directory: None,
                project_root: None,
                environment: None,
            },
            conf: talos_types::SessionConf::default(),
            project_id: None,
            current_directory: None,
            config: {
                let mut config = talos_types::TalosConfig::default();
                config.agent.model = "qwen-plus".to_string();
                config.llm.model = "qwen-plus".to_string();
                config
            },
            state: talos_types::SessionState {
                created_at: 0,
                last_activity: 0,
                message_count: 1,
                usage: talos_types::talos::TalosUsage::default(),
            },
        }))
    }

    async fn patch_session(
        &self,
        _session_id: &str,
        _patch: talos_types::SessionPatch,
    ) -> TalosResult<talos_types::SessionPatchAck> {
        Ok(talos_types::SessionPatchAck { success: true })
    }

    async fn compact_session(&self, session_id: &str) -> TalosResult<talos_types::TalosSession> {
        self.get_session(session_id)
            .await?
            .ok_or_else(|| talos_error::TalosError::ResourceNotFound(session_id.to_string()))
    }

    async fn clear_session(&self, session_id: &str) -> TalosResult<talos_types::TalosSession> {
        let mut session = self
            .get_session(session_id)
            .await?
            .ok_or_else(|| talos_error::TalosError::ResourceNotFound(session_id.to_string()))?;
        session.state.message_count = 0;
        session.state.usage = talos_types::talos::TalosUsage::default();
        Ok(session)
    }

    async fn close_session(&self, _session_id: &str) -> TalosResult<()> {
        Ok(())
    }

    async fn delete_session(&self, _session_id: &str) -> TalosResult<talos_types::SessionPatchAck> {
        Ok(talos_types::SessionPatchAck { success: true })
    }

    async fn cancel_active_turn(&self, _session_id: &str) -> TalosResult<()> {
        Ok(())
    }

    async fn submit_tool_approval(
        &self,
        _session_id: &str,
        _call_id: &str,
        _tool_name: &str,
        _approved: bool,
        _reason: Option<String>,
    ) -> TalosResult<()> {
        Ok(())
    }

    async fn submit_dynamic_tool_result(
        &self,
        _session_id: &str,
        _call_id: &str,
        _tool_name: &str,
        _output: String,
        _success: bool,
    ) -> TalosResult<()> {
        Ok(())
    }

    async fn get_session_turns(
        &self,
        _session_id: &str,
    ) -> TalosResult<Vec<talos_types::talos::TalosMessageResponse>> {
        Ok(vec![])
    }

    async fn list_projects(&self) -> TalosResult<Vec<talos_types::ProjectRecord>> {
        Ok(Vec::new())
    }

    async fn get_project(
        &self,
        _project_id: &str,
    ) -> TalosResult<Option<talos_types::ProjectRecord>> {
        Ok(None)
    }

    async fn create_project(
        &self,
        name: String,
        directory: std::path::PathBuf,
        _user_id: String,
    ) -> TalosResult<talos_types::ProjectRecord> {
        Ok(talos_types::ProjectRecord {
            project_id: "project-mock".to_string(),
            name,
            user_id: "static-token".to_string(),
            directory,
            created_at: 0,
            updated_at: 0,
            session_count: 0,
        })
    }

    async fn patch_project(
        &self,
        _project_id: &str,
        _patch: talos_types::ProjectPatch,
    ) -> TalosResult<Option<talos_types::ProjectRecord>> {
        Ok(None)
    }

    async fn delete_project(&self, _project_id: &str) -> TalosResult<()> {
        Ok(())
    }
}

fn test_auth() -> RequestIdentity {
    RequestIdentity::mock_admin("static-token")
}

fn test_context() -> TalosContext {
    TalosContext::new(
        TalosSourceType::Http,
        TalosProvider::Http,
        "static-token",
        "msg-1",
        "trace-1",
    )
    .with_channel_id("test-channel")
}

#[tokio::test]
async fn test_health() {
    let state = test_state(false);
    let app = Router::new()
        .route("/health", get(health_handler))
        .with_state(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("body should be readable");
    let payload: Value = serde_json::from_slice(&body).expect("payload should be valid json");
    assert_eq!(payload["auth_enabled"], true);
}

#[tokio::test]
async fn test_health_reports_auth_disabled() {
    let state = test_state(true);
    let app = Router::new()
        .route("/health", get(health_handler))
        .with_state(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("body should be readable");
    let payload: Value = serde_json::from_slice(&body).expect("payload should be valid json");
    assert_eq!(payload["auth_enabled"], false);
}

#[tokio::test]
async fn test_models() {
    let state = test_state(false);
    let app = Router::new()
        .route("/v1/models", get(list_models_handler))
        .with_state(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("body should be readable");
    let payload: Value = serde_json::from_slice(&body).expect("payload should be valid json");
    assert_eq!(payload["data"][0]["id"], "qwen-plus");
    assert_eq!(payload["data"][0]["provider"], "bailian");
    assert_eq!(payload["data"][0]["context_window"], 32_768);
    assert_eq!(payload["data"][0]["reasoning"], true);
}

#[tokio::test]
async fn test_create_session_handler_returns_session_payload() {
    let state = test_state(false);

    let response = create_session_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Json(
            serde_json::from_value(serde_json::json!({
                "context": test_context(),
                "model": "qwen-plus"
            }))
            .expect("request should deserialize"),
        ),
    )
    .await
    .expect("create session should succeed");

    assert_eq!(response.0["context"]["session_id"], "mock-session");
    assert!(response.0["project_id"].is_null());
    assert!(response.0["current_directory"].is_null());
    assert_eq!(response.0["config"]["llm"]["model"], "qwen-plus");
}

#[tokio::test]
async fn test_list_sessions_handler_returns_data_array() {
    let state = test_state(false);

    let response = list_sessions_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Query(SessionQuery::default()),
    )
    .await
    .expect("list sessions should succeed");

    assert!(response.0["data"].is_array());
}

#[tokio::test]
async fn test_get_session_handler_returns_not_found_for_unknown_session() {
    let state = test_state(false);

    let err = get_session_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Path("unknown-session".to_string()),
    )
    .await
    .expect_err("unknown session should return error");

    assert_eq!(err.into_response().status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_patch_session_handler_returns_success_ack() {
    let state = test_state(false);

    let response = patch_session_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Path("mock-session".to_string()),
        Json(
            serde_json::from_value(serde_json::json!({
                "conf": {"agent.model": "qwen-plus"}
            }))
            .expect("patch request should deserialize"),
        ),
    )
    .await
    .expect("patch session should succeed");

    assert_eq!(response.0["success"], serde_json::Value::Bool(true));
}

#[tokio::test]
async fn test_create_user_token_handler_returns_raw_token() {
    let state = test_state(false);

    let response = create_user_token_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Json(CreateUserTokenRequest {
            name: "cli".to_string(),
        }),
    )
    .await
    .expect("create token should succeed");

    assert_eq!(response.0["prefix"].as_str().unwrap_or_default().len(), 12);
    assert!(response.0["raw_token"]
        .as_str()
        .unwrap_or_default()
        .starts_with("tls_"));
}

#[tokio::test]
async fn test_list_user_tokens_handler_returns_data_array() {
    let state = test_state(false);

    let _ = create_user_token_handler(
        State(state.clone()),
        axum::extract::Extension(test_auth()),
        Json(CreateUserTokenRequest {
            name: "cli".to_string(),
        }),
    )
    .await
    .expect("create token should succeed");

    let response = list_user_tokens_handler(State(state), axum::extract::Extension(test_auth()))
        .await
        .expect("list tokens should succeed");

    assert_eq!(response.0["data"].as_array().map(Vec::len), Some(1));
}

#[tokio::test]
async fn test_close_session_handler_returns_success() {
    let state = test_state(false);

    let response = close_session_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Path("mock-session".to_string()),
    )
    .await
    .expect("close session should succeed");

    assert_eq!(response.0["success"], serde_json::Value::Bool(true));
}

#[tokio::test]
async fn test_delete_session_handler_returns_success() {
    let state = test_state(false);

    let response = delete_session_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Path("mock-session".to_string()),
    )
    .await
    .expect("delete session should succeed");

    assert_eq!(response.0["success"], serde_json::Value::Bool(true));
}

#[tokio::test]
async fn test_delete_admin_session_handler_returns_success() {
    let state = test_state(false);

    let response = delete_admin_session_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Path("mock-session".to_string()),
    )
    .await
    .expect("delete admin session should succeed");

    assert_eq!(response.0["success"], serde_json::Value::Bool(true));
}

#[tokio::test]
async fn test_list_admin_sessions_handler_returns_pagination_payload() {
    let state = test_state(false);

    let response = list_admin_sessions_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Query(SessionQuery {
            user_id: None,
            channel_id: None,
            provider: None,
            project_id: None,
            limit: Some(25),
            offset: Some(50),
        }),
    )
    .await
    .expect("list admin sessions should succeed");

    assert_eq!(response.0["data"], serde_json::Value::Array(Vec::new()));
    assert_eq!(response.0["pagination"]["total"], 0);
    assert_eq!(response.0["pagination"]["limit"], 25);
    assert_eq!(response.0["pagination"]["offset"], 50);
}

#[tokio::test]
async fn test_compact_session_handler_returns_session_payload() {
    let state = test_state(false);

    let response = compact_session_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Path("mock-session".to_string()),
    )
    .await
    .expect("compact session should succeed");

    assert_eq!(response.0["context"]["session_id"], "mock-session");
    assert_eq!(response.0["state"]["message_count"], 1);
}

#[tokio::test]
async fn test_clear_session_handler_returns_reset_session_payload() {
    let state = test_state(false);

    let response = clear_session_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Path("mock-session".to_string()),
    )
    .await
    .expect("clear session should succeed");

    assert_eq!(response.0["context"]["session_id"], "mock-session");
    assert_eq!(response.0["state"]["message_count"], 0);
    assert_eq!(response.0["state"]["usage"]["total_tokens"], 0);
}

#[tokio::test]
async fn test_get_workspace_doc_handler_returns_exists_false_for_missing_file() {
    let (config, workspace_dir) = isolated_test_config();
    let state = test_state_with_config(config, false);

    let response = get_workspace_doc_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Path("soul".to_string()),
    )
    .await
    .expect("get workspace doc should succeed");

    assert_eq!(response.0["kind"], "soul");
    assert_eq!(response.0["exists"], serde_json::Value::Bool(false));
    assert_eq!(response.0["content"], "");
    let _ = std::fs::remove_dir_all(workspace_dir);
}

#[tokio::test]
async fn test_put_and_get_memory_file_handlers_roundtrip() {
    let (config, workspace_dir) = isolated_test_config();
    let state = test_state_with_config(config, false);

    let put_response = put_memory_file_handler(
        State(state.clone()),
        axum::extract::Extension(test_auth()),
        Path("2026-04-04".to_string()),
        Json(talos_types::DocUpdateRequest {
            content: "## [10:00] Example\nsession: session-1\n\n**Facts:**\n- stable fact\n"
                .to_string(),
            base_version: talos_types::ExpectedFileVersion::MustNotExist,
        }),
    )
    .await
    .expect("put memory file should succeed");

    assert_eq!(put_response.0["date"], "2026-04-04");

    let get_response = get_memory_file_handler(
        State(state.clone()),
        axum::extract::Extension(test_auth()),
        Path("2026-04-04".to_string()),
    )
    .await
    .expect("get memory file should succeed");

    assert!(get_response.0["content"]
        .as_str()
        .unwrap()
        .contains("stable fact"));

    let list_response = list_memory_files_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Query(serde_json::from_value(serde_json::json!({ "limit": 10 })).unwrap()),
    )
    .await
    .expect("list memory files should succeed");

    assert_eq!(list_response.0["data"][0]["date"], "2026-04-04");
    let _ = std::fs::remove_dir_all(workspace_dir);
}

#[tokio::test]
async fn test_search_memory_files_handler_returns_matching_data() {
    let (config, workspace_dir) = isolated_test_config();
    let memory_dir = workspace_dir.join("memory");
    std::fs::create_dir_all(&memory_dir).expect("memory dir should exist");
    std::fs::write(
        memory_dir.join("2026-04-04.md"),
        "## [10:00] Example\nsession: session-1\n\n**Facts:**\n- stable fact\n",
    )
    .expect("memory file should write");
    let state = test_state_with_config(config, false);

    let response = search_memory_files_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Query(
            serde_json::from_value(serde_json::json!({
                "q": "stable",
                "limit": 5,
                "date_from": "2026-04-01",
                "date_to": "2026-04-30"
            }))
            .unwrap(),
        ),
    )
    .await
    .expect("search memory should succeed");

    assert_eq!(response.0["data"][0]["path"], "memory/2026-04-04.md");
    let _ = std::fs::remove_dir_all(workspace_dir);
}

#[test]
fn test_http_server_listen_addr_uses_configured_host() {
    let (auth_service, user_service) = test_auth_services();
    let llm_config_service: Arc<dyn talos_config::LlmConfigService> =
        Arc::new(talos_config::LlmConfigManager::new(Arc::new(
            talos_config::InMemoryLlmConfigStore::new(Default::default()),
        )));
    let server = HttpServer::new(
        Arc::new(MockGateway),
        Arc::new(MockSessionService),
        test_event_service(),
        Arc::new(RecordingEventStore::default()),
        auth_service,
        user_service,
        test_config(),
        Arc::new(MockLlmProvider),
        llm_config_service,
        "127.0.0.1",
        8080,
        "qwen-plus",
        vec![test_model_info()],
    );

    assert_eq!(server.listen_addr(), "127.0.0.1:8080");
}

#[tokio::test]
async fn test_models_require_bearer_when_auth_enabled() {
    let state = test_state(false);
    let app = Router::new()
        .route("/v1/models", get(list_models_handler))
        .with_state(state.clone())
        .layer(middleware::from_fn_with_state(state, auth_middleware));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_models_allow_no_token_when_auth_disabled() {
    let state = test_state(true);
    let app = Router::new()
        .route("/v1/models", get(list_models_handler))
        .with_state(state.clone())
        .layer(middleware::from_fn_with_state(state, auth_middleware));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_list_sessions_allows_anonymous_route_when_auth_disabled() {
    let state = test_state(true);
    let app = Router::new()
        .route("/v1/sessions", get(list_sessions_handler))
        .with_state(state.clone())
        .layer(middleware::from_fn_with_state(state, auth_middleware));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/sessions")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("body should be readable");
    let payload: Value = serde_json::from_slice(&body).expect("payload should be valid json");
    assert!(payload["data"].is_array());
}

#[tokio::test]
async fn test_list_admin_users_allows_auth_disabled_actor() {
    let state = test_state(true);
    let response = list_admin_users_handler(
        State(state),
        axum::extract::Extension(RequestIdentity::auth_disabled()),
    )
    .await
    .expect("auth disabled actor should access admin users");
    let body = response.0;
    let data = body
        .get("data")
        .and_then(Value::as_array)
        .expect("data array should exist");
    assert!(data.is_empty());
}

#[tokio::test]
async fn test_get_admin_llm_allows_auth_disabled_actor() {
    let state = test_state(true);
    let response = get_admin_llm_handler(
        State(state),
        axum::extract::Extension(RequestIdentity::auth_disabled()),
    )
    .await
    .expect("auth disabled actor should access admin llm");
    assert_eq!(response.0.provider, "anthropic");
}

#[tokio::test]
async fn test_lifecycle_gate_rejects_new_requests_when_quiescing() {
    let lifecycle = Arc::new(ServerLifecycle::new());
    lifecycle.store(ServerLifecycleState::Quiescing);
    let (auth_service, user_service) = test_auth_services();
    let llm_svc: Arc<dyn talos_config::LlmConfigService> =
        Arc::new(talos_config::LlmConfigManager::new(Arc::new(
            talos_config::InMemoryLlmConfigStore::new(Default::default()),
        )));
    let state = Arc::new(HttpApiState::new_with_lifecycle(
        Arc::new(MockGateway),
        Arc::new(MockSessionService),
        test_event_service(),
        Arc::new(RecordingEventStore::default()),
        test_config(),
        Arc::new(MockLlmProvider),
        auth_service,
        user_service,
        "test-secret".to_string(),
        true,
        vec![test_model_info()],
        llm_svc,
        lifecycle,
    ));
    let app = Router::new()
        .route("/v1/models", get(list_models_handler))
        .with_state(state.clone())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            lifecycle_gate_middleware,
        ))
        .layer(middleware::from_fn_with_state(state, auth_middleware));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn test_lifecycle_gate_allows_health_when_quiescing() {
    let lifecycle = Arc::new(ServerLifecycle::new());
    lifecycle.store(ServerLifecycleState::Quiescing);
    let (auth_service, user_service) = test_auth_services();
    let llm_svc: Arc<dyn talos_config::LlmConfigService> =
        Arc::new(talos_config::LlmConfigManager::new(Arc::new(
            talos_config::InMemoryLlmConfigStore::new(Default::default()),
        )));
    let state = Arc::new(HttpApiState::new_with_lifecycle(
        Arc::new(MockGateway),
        Arc::new(MockSessionService),
        test_event_service(),
        Arc::new(RecordingEventStore::default()),
        test_config(),
        Arc::new(MockLlmProvider),
        auth_service,
        user_service,
        "test-secret".to_string(),
        true,
        vec![test_model_info()],
        llm_svc,
        lifecycle,
    ));
    let app = Router::new()
        .route("/health", get(health_handler))
        .with_state(state.clone())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            lifecycle_gate_middleware,
        ))
        .layer(middleware::from_fn_with_state(state, auth_middleware));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_submit_message_command_returns_async_ack() {
    let event_store = Arc::new(RecordingEventStore::default());
    let state = test_state_with_event_store(false, event_store.clone());

    let request = TalosMessageRequest {
        input: vec![TalosInputItem::Text {
            text: "hello".to_string(),
        }],
        workspace_id: None,
    };

    let result = submit_message_command(
        state,
        test_auth(),
        HeaderMap::new(),
        "mock-session".to_string(),
        request,
    )
    .await
    .expect("message should be processed");

    assert_eq!(result.session_id, "mock-session");
    assert_eq!(result.message_id.as_deref(), Some("msg_mock"));
    assert_eq!(result.status, IdempotencyStatus::Accepted);
    assert!(result.turn_id.starts_with("turn_"));

    let recorded = event_store.events.lock().unwrap().clone();
    assert!(!recorded.is_empty());
    assert_eq!(recorded[0].event_type, TalosEventType::MessageStarted);
    assert_eq!(recorded[0].message_id, "msg_mock");
}

#[tokio::test]
async fn test_session_message_handler_reuses_idempotent_ack() {
    let event_store = Arc::new(RecordingEventStore::default());
    let state = test_state_with_event_store(false, event_store.clone());
    let app = Router::new()
        .route(
            "/v1/sessions/:session_id/messages",
            post(session_message_handler),
        )
        .layer(axum::Extension(test_auth()))
        .with_state(state);
    let body = serde_json::json!({
        "input": [{ "type": "text", "text": "hello" }]
    })
    .to_string();

    let first = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/sessions/mock-session/messages")
                .header("content-type", "application/json")
                .header("idempotency-key", "idem-1")
                .body(Body::from(body.clone()))
                .expect("request should build"),
        )
        .await
        .expect("first request should succeed");
    assert_eq!(first.status(), StatusCode::ACCEPTED);
    let first_body = axum::body::to_bytes(first.into_body(), usize::MAX)
        .await
        .expect("first body should be readable");
    let first_payload: Value =
        serde_json::from_slice(&first_body).expect("first payload should be json");

    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    let recorded_before = event_store.events.lock().unwrap().len();

    let second = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/sessions/mock-session/messages")
                .header("content-type", "application/json")
                .header("idempotency-key", "idem-1")
                .body(Body::from(body))
                .expect("request should build"),
        )
        .await
        .expect("second request should succeed");
    assert_eq!(second.status(), StatusCode::ACCEPTED);
    let second_body = axum::body::to_bytes(second.into_body(), usize::MAX)
        .await
        .expect("second body should be readable");
    let second_payload: Value =
        serde_json::from_slice(&second_body).expect("second payload should be json");

    assert_eq!(first_payload["turn_id"], second_payload["turn_id"]);
    assert_eq!(first_payload["message_id"], second_payload["message_id"]);
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    assert_eq!(event_store.events.lock().unwrap().len(), recorded_before);
}

#[tokio::test]
async fn test_session_message_handler_rejects_idempotency_hash_conflict() {
    let event_store = Arc::new(RecordingEventStore::default());
    let state = test_state_with_event_store(false, event_store);
    let app = Router::new()
        .route(
            "/v1/sessions/:session_id/messages",
            post(session_message_handler),
        )
        .layer(axum::Extension(test_auth()))
        .with_state(state);

    let first_body = serde_json::json!({
        "input": [{ "type": "text", "text": "hello" }]
    })
    .to_string();
    let second_body = serde_json::json!({
        "input": [{ "type": "text", "text": "different" }]
    })
    .to_string();

    let first = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/sessions/mock-session/messages")
                .header("content-type", "application/json")
                .header("idempotency-key", "idem-conflict")
                .body(Body::from(first_body))
                .expect("request should build"),
        )
        .await
        .expect("first request should succeed");
    assert_eq!(first.status(), StatusCode::ACCEPTED);

    let second = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/sessions/mock-session/messages")
                .header("content-type", "application/json")
                .header("idempotency-key", "idem-conflict")
                .body(Body::from(second_body))
                .expect("request should build"),
        )
        .await
        .expect("second request should succeed");

    assert_eq!(second.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn test_submit_message_command_timeout_stores_failed_idempotency() {
    let event_store = Arc::new(RecordingEventStore::default());
    let mut config = test_config();
    config.agent.kernel.session_events.first_event_timeout_ms = 1;
    let state = test_state_with_gateway_event_store_config(
        Arc::new(SilentGateway),
        false,
        event_store,
        config,
    );
    let request = TalosMessageRequest {
        input: vec![TalosInputItem::Text {
            text: "hello".to_string(),
        }],
        workspace_id: None,
    };
    let mut headers = HeaderMap::new();
    headers.insert("idempotency-key", "timeout-key".parse().unwrap());

    let err = submit_message_command(
        state.clone(),
        test_auth(),
        headers.clone(),
        "mock-session".to_string(),
        request.clone(),
    )
    .await
    .expect_err("first request should timeout");
    assert_eq!(err.into_response().status(), StatusCode::GATEWAY_TIMEOUT);

    let retry = submit_message_command(
        state,
        test_auth(),
        headers,
        "mock-session".to_string(),
        request,
    )
    .await
    .expect("retry should return failed terminal ack");
    assert_eq!(retry.status, IdempotencyStatus::FailedNoFirstEvent);
    assert!(retry.message_id.is_some());
}

#[tokio::test]
async fn test_retry_message_attempt_rejects_running_active_turn() {
    let event_store = Arc::new(RecordingEventStore::default());
    event_store.active_turns.lock().unwrap().insert(
        "mock-session".to_string(),
        ActiveTurnRecord {
            session_id: "mock-session".to_string(),
            turn_id: "turn-running".to_string(),
            message_id: Some("msg-running".to_string()),
            status: ActiveTurnStatus::Running,
            last_event_id: Some(SessionEventId::from_seq(3)),
            pending_user_action: None,
            updated_at: 123,
        },
    );
    let state = test_state_with_event_store(false, event_store);
    let app = Router::new()
        .route(
            "/v1/sessions/:session_id/message-attempts",
            post(retry_message_attempt_handler),
        )
        .layer(axum::Extension(test_auth()))
        .with_state(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/sessions/mock-session/message-attempts")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "client_message_id": "client-1",
                        "retry_of_message_id": "msg-running",
                        "input": [{"type": "text", "text": "retry"}],
                        "workspace_id": null
                    })
                    .to_string(),
                ))
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("error body should be readable");
    let payload: Value = serde_json::from_slice(&body).expect("payload should be valid json");
    assert_eq!(payload["code"], "retry_active_turn_running");
}

#[tokio::test]
async fn test_retry_message_attempt_rejects_completed_active_turn() {
    let event_store = Arc::new(RecordingEventStore::default());
    event_store.active_turns.lock().unwrap().insert(
        "mock-session".to_string(),
        ActiveTurnRecord {
            session_id: "mock-session".to_string(),
            turn_id: "turn-completed".to_string(),
            message_id: Some("msg-completed".to_string()),
            status: ActiveTurnStatus::Completed,
            last_event_id: Some(SessionEventId::from_seq(4)),
            pending_user_action: None,
            updated_at: 124,
        },
    );
    let state = test_state_with_event_store(false, event_store);
    let app = Router::new()
        .route(
            "/v1/sessions/:session_id/message-attempts",
            post(retry_message_attempt_handler),
        )
        .layer(axum::Extension(test_auth()))
        .with_state(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/sessions/mock-session/message-attempts")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "client_message_id": "client-1",
                        "retry_of_message_id": "msg-completed",
                        "input": [{"type": "text", "text": "retry"}],
                        "workspace_id": null
                    })
                    .to_string(),
                ))
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("error body should be readable");
    let payload: Value = serde_json::from_slice(&body).expect("payload should be valid json");
    assert_eq!(payload["code"], "retry_source_completed");
}

#[tokio::test]
async fn test_retry_message_attempt_allows_failed_active_turn() {
    let event_store = Arc::new(RecordingEventStore::default());
    event_store.active_turns.lock().unwrap().insert(
        "mock-session".to_string(),
        ActiveTurnRecord {
            session_id: "mock-session".to_string(),
            turn_id: "turn-failed".to_string(),
            message_id: Some("msg-failed".to_string()),
            status: ActiveTurnStatus::Failed,
            last_event_id: Some(SessionEventId::from_seq(5)),
            pending_user_action: None,
            updated_at: 125,
        },
    );
    let state = test_state_with_event_store(false, event_store);
    let app = Router::new()
        .route(
            "/v1/sessions/:session_id/message-attempts",
            post(retry_message_attempt_handler),
        )
        .layer(axum::Extension(test_auth()))
        .with_state(state);

    let request_body = serde_json::json!({
        "client_message_id": "client-1",
        "retry_of_message_id": "msg-failed",
        "input": [{"type": "text", "text": "retry"}],
        "workspace_id": null
    })
    .to_string();

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/sessions/mock-session/message-attempts")
                .header("content-type", "application/json")
                .header("idempotency-key", "retry-attempt-1")
                .body(Body::from(request_body.clone()))
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(resp.status(), StatusCode::ACCEPTED);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("ack body should be readable");
    let payload: Value = serde_json::from_slice(&body).expect("payload should be valid json");
    assert_eq!(payload["message_id"], "msg_mock");
    assert_eq!(payload["status"], "accepted");

    let retry_resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/sessions/mock-session/message-attempts")
                .header("content-type", "application/json")
                .header("idempotency-key", "ignored-client-key")
                .body(Body::from(request_body))
                .expect("request should build"),
        )
        .await
        .expect("retry request should succeed");

    assert_eq!(retry_resp.status(), StatusCode::ACCEPTED);
    let retry_body = axum::body::to_bytes(retry_resp.into_body(), usize::MAX)
        .await
        .expect("retry ack body should be readable");
    let retry_payload: Value =
        serde_json::from_slice(&retry_body).expect("retry payload should be valid json");
    assert_eq!(retry_payload["turn_id"], payload["turn_id"]);
    assert_eq!(retry_payload["message_id"], payload["message_id"]);
}

#[tokio::test]
async fn test_subscribe_session_events_replays_after_cursor() {
    let state = test_state(false);
    let first = state
        .event_service
        .record_talos_event(
            Some("turn-1".to_string()),
            TalosEvent {
                event_type: TalosEventType::MessageStarted,
                message_id: "msg-1".to_string(),
                session_id: "mock-session".to_string(),
                sequence: 0,
                payload: TalosEventPayload::MessageStarted(TalosMessageStartedEvent {
                    request_id: "req-1".to_string(),
                    model: "qwen-plus".to_string(),
                    usage: TalosUsage::default(),
                    iteration: 0,
                    step_id: "start".to_string(),
                }),
            },
        )
        .await
        .expect("first event should record");
    let second = state
        .event_service
        .record_talos_event(
            Some("turn-1".to_string()),
            TalosEvent {
                event_type: TalosEventType::MessageOutputDelta,
                message_id: "msg-1".to_string(),
                session_id: "mock-session".to_string(),
                sequence: 1,
                payload: TalosEventPayload::MessageOutputDelta(TalosMessageOutputDeltaEvent {
                    request_id: "req-1".to_string(),
                    delta: "delta after reconnect".to_string(),
                    item_index: 0,
                    iteration: 1,
                    step_id: "step-1".to_string(),
                }),
            },
        )
        .await
        .expect("second event should record");
    let app = Router::new()
        .route(
            "/v1/sessions/:session_id/events",
            get(subscribe_session_events_handler),
        )
        .layer(axum::Extension(test_auth()))
        .with_state(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/v1/sessions/mock-session/events?after_event_id={}",
                    first.event_id
                ))
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("event body should be readable");
    let body_text = String::from_utf8_lossy(&body);
    assert!(body_text.contains("event: message.output.delta"));
    assert!(body_text.contains(second.event_id.as_str()));
    assert!(body_text.contains("delta after reconnect"));
    assert!(!body_text.contains("event: message.started"));
}

#[tokio::test]
async fn test_subscribe_session_events_replay_continues_after_historical_terminal() {
    let state = test_state(false);
    state
        .event_service
        .record_talos_event(
            Some("turn-1".to_string()),
            TalosEvent {
                event_type: TalosEventType::MessageCompleted,
                message_id: "msg-1".to_string(),
                session_id: "mock-session".to_string(),
                sequence: 0,
                payload: TalosEventPayload::MessageCompleted(TalosMessageCompletedEvent {
                    request_id: "req-1".to_string(),
                    stop_reason: StopReason::EndTurn,
                    usage: TalosUsage::default(),
                    total_output_items: 1,
                    iteration: 0,
                    step_id: "done".to_string(),
                    task_count: 0,
                }),
            },
        )
        .await
        .expect("historical terminal event should record");
    let second = state
        .event_service
        .record_talos_event(
            Some("turn-2".to_string()),
            TalosEvent {
                event_type: TalosEventType::MessageOutputDelta,
                message_id: "msg-2".to_string(),
                session_id: "mock-session".to_string(),
                sequence: 0,
                payload: TalosEventPayload::MessageOutputDelta(TalosMessageOutputDeltaEvent {
                    request_id: "req-2".to_string(),
                    delta: "second turn output".to_string(),
                    item_index: 0,
                    iteration: 0,
                    step_id: "step-1".to_string(),
                }),
            },
        )
        .await
        .expect("second turn event should record");
    state
        .event_service
        .record_talos_event(
            Some("turn-2".to_string()),
            TalosEvent {
                event_type: TalosEventType::MessageCompleted,
                message_id: "msg-2".to_string(),
                session_id: "mock-session".to_string(),
                sequence: 1,
                payload: TalosEventPayload::MessageCompleted(TalosMessageCompletedEvent {
                    request_id: "req-2".to_string(),
                    stop_reason: StopReason::EndTurn,
                    usage: TalosUsage::default(),
                    total_output_items: 1,
                    iteration: 1,
                    step_id: "done".to_string(),
                    task_count: 0,
                }),
            },
        )
        .await
        .expect("second terminal event should record");

    let app = Router::new()
        .route(
            "/v1/sessions/:session_id/events",
            get(subscribe_session_events_handler),
        )
        .layer(axum::Extension(test_auth()))
        .with_state(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/sessions/mock-session/events")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("event body should be readable");
    let body_text = String::from_utf8_lossy(&body);
    assert!(body_text.contains("event: message.completed"));
    assert!(body_text.contains(second.event_id.as_str()));
    assert!(body_text.contains("second turn output"));
}

#[tokio::test]
async fn test_get_active_turn_handler_returns_active_state() {
    let event_store = Arc::new(RecordingEventStore::default());
    event_store.active_turns.lock().unwrap().insert(
        "mock-session".to_string(),
        ActiveTurnRecord {
            session_id: "mock-session".to_string(),
            turn_id: "turn-1".to_string(),
            message_id: Some("msg-1".to_string()),
            status: ActiveTurnStatus::Running,
            last_event_id: Some(SessionEventId::from_seq(7)),
            pending_user_action: None,
            updated_at: 123,
        },
    );
    let state = test_state_with_event_store(false, event_store);
    let app = Router::new()
        .route(
            "/v1/sessions/:session_id/active-turn",
            get(get_active_turn_handler),
        )
        .layer(axum::Extension(test_auth()))
        .with_state(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/sessions/mock-session/active-turn")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("active turn body should be readable");
    let payload: Value = serde_json::from_slice(&body).expect("payload should be valid json");
    assert_eq!(payload["status"], "running");
    assert_eq!(payload["turn_id"], "turn-1");
    assert_eq!(payload["last_event_id"], "ev_00000000000000000007");
}

#[tokio::test]
async fn test_get_active_turn_handler_returns_pending_approval_action() {
    let event_store = Arc::new(RecordingEventStore::default());
    event_store.active_turns.lock().unwrap().insert(
        "mock-session".to_string(),
        ActiveTurnRecord {
            session_id: "mock-session".to_string(),
            turn_id: "turn-approval".to_string(),
            message_id: Some("msg-approval".to_string()),
            status: ActiveTurnStatus::WaitingUserAction,
            last_event_id: Some(SessionEventId::from_seq(8)),
            pending_user_action: Some(PendingUserAction::ToolApproval(PendingToolApproval {
                call_id: "call-approval".to_string(),
                tool_name: "Bash".to_string(),
                prompt: "Approve command?".to_string(),
                arguments: serde_json::json!({"cmd": "date"}),
            })),
            updated_at: 124,
        },
    );
    let state = test_state_with_event_store(false, event_store);
    let app = Router::new()
        .route(
            "/v1/sessions/:session_id/active-turn",
            get(get_active_turn_handler),
        )
        .layer(axum::Extension(test_auth()))
        .with_state(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/sessions/mock-session/active-turn")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("active turn body should be readable");
    let payload: Value = serde_json::from_slice(&body).expect("payload should be valid json");
    assert_eq!(payload["status"], "waiting_user_action");
    assert_eq!(payload["pending_user_action"]["type"], "tool_approval");
    assert_eq!(payload["pending_user_action"]["call_id"], "call-approval");
    assert_eq!(payload["pending_user_action"]["tool_name"], "Bash");
    assert_eq!(payload["pending_user_action"]["prompt"], "Approve command?");
}

#[tokio::test]
async fn test_get_active_turn_handler_returns_pending_dynamic_action() {
    let event_store = Arc::new(RecordingEventStore::default());
    event_store.active_turns.lock().unwrap().insert(
        "mock-session".to_string(),
        ActiveTurnRecord {
            session_id: "mock-session".to_string(),
            turn_id: "turn-dynamic".to_string(),
            message_id: Some("msg-dynamic".to_string()),
            status: ActiveTurnStatus::WaitingUserAction,
            last_event_id: Some(SessionEventId::from_seq(9)),
            pending_user_action: Some(PendingUserAction::DynamicToolResult(
                PendingDynamicToolResult {
                    call_id: "call-dynamic".to_string(),
                    tool_name: "ExternalTool".to_string(),
                },
            )),
            updated_at: 125,
        },
    );
    let state = test_state_with_event_store(false, event_store);
    let app = Router::new()
        .route(
            "/v1/sessions/:session_id/active-turn",
            get(get_active_turn_handler),
        )
        .layer(axum::Extension(test_auth()))
        .with_state(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/sessions/mock-session/active-turn")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("active turn body should be readable");
    let payload: Value = serde_json::from_slice(&body).expect("payload should be valid json");
    assert_eq!(payload["status"], "waiting_user_action");
    assert_eq!(
        payload["pending_user_action"]["type"],
        "dynamic_tool_result"
    );
    assert_eq!(payload["pending_user_action"]["call_id"], "call-dynamic");
    assert_eq!(payload["pending_user_action"]["tool_name"], "ExternalTool");
}

#[tokio::test]
async fn test_cancel_active_turn_handler_returns_success() {
    let state = test_state(false);

    let result = cancel_active_turn_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Path("mock-session".to_string()),
    )
    .await
    .expect("cancel should succeed");

    assert_eq!(result.0["success"], serde_json::Value::Bool(true));
}

#[tokio::test]
async fn test_submit_tool_approval_handler_returns_no_content() {
    let state = test_state(false);
    let app = Router::new()
        .route(
            "/v1/sessions/:session_id/tool-approvals",
            post(submit_tool_approval_handler),
        )
        .layer(axum::Extension(test_auth()))
        .with_state(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/sessions/mock-session/tool-approvals")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "call_id": "call-1",
                        "tool_name": "Bash",
                        "approved": false,
                        "reason": "deny"
                    })
                    .to_string(),
                ))
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}
