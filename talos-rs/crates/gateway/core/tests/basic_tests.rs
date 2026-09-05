use async_trait::async_trait;
use opendal::{services::Memory, Operator};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use talos_agent::{
    AgentAdmission, AgentAdmissionPermit, AgentContext, AgentEventPipeline, AgentInputParams,
    AgentInterruptParams, AgentProgressEventReceiver, AgentRunExecution, AgentRunParams,
    AgentRunSettlement, AgentRunSettlementReceiver, AgentSessionContext,
};
use talos_agent_api::{
    AgentEventPayload, AgentInterruptOutcome, AgentRunMetrics, AgentTurnCompleted, AgentTurnStarted,
};
use talos_auth::{AuthService, RequestIdentity, UserDto, UserRole, UserService};
use talos_config::{ConfigStore, InMemoryLlmConfigStore, ProviderModelConfig};
use talos_coordinator::{AgentCoordinator, AgentSelectionParams};
use talos_error::TalosResult;
use talos_gateway::{Gateway, ServiceGateway};
use talos_service::TimelineEventService;
use talos_service::{
    Actor, ConfigReloadHook, ConfigReloadPlan, ConfigReloadTrigger, FileService, MemoryService,
    TalosService,
};
use talos_storage::SqliteStorage;
use talos_store::file::DatabaseFileStore;
use talos_store::memory::MemoryStore;
use talos_store::migration::run_migrations;
use talos_store::profile::{DatabaseProfileStore, ProfileStore};
use talos_store::project::{ProjectStore, StorageProjectStore};
use talos_store::session::{AgentExecutionStore, DatabaseSessionStore, SessionStore};
use talos_types::talos::SessionSource;
use talos_types::{AgentUserContent, AgentUserInput};
use talos_types::{
    ConfigGetRequest, ConfigPath, ConfigUpdateRequest, ConfigValue, SessionPatchRequest,
    StartTurnRequest, TalosConfig, TurnCommandStatus,
};

struct RecordingHandler;
struct TestAuthService;

fn test_config_reload_hook(config: TalosConfig) -> ConfigReloadHook {
    let active = Arc::new(std::sync::RwLock::new(config));
    Arc::new(move |trigger| {
        let mut next = active
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        if let ConfigReloadTrigger::Updated(value) = trigger {
            match value {
                ConfigValue::Llm(llm) => next.llm = *llm,
                ConfigValue::WebSearch(web_search) => {
                    next.agent.kernel.tools.web_search = *web_search;
                }
            }
        }
        let active = active.clone();
        Ok(ConfigReloadPlan::new(next.clone(), move || {
            *active
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = next;
        }))
    })
}

#[async_trait]
impl AgentCoordinator for RecordingHandler {
    async fn resolve_agent(&self, _params: AgentSelectionParams) -> TalosResult<String> {
        Ok("default".to_string())
    }

    async fn ensure_agent(&self, _agent_id: &str) -> TalosResult<()> {
        Ok(())
    }

    async fn start(&self, params: AgentRunParams) -> TalosResult<AgentRunExecution> {
        let identity = params.context.identity();
        let (pipeline, _sink, progress_rx) = AgentEventPipeline::fresh(identity);
        let mut admission_events = vec![pipeline
            .record(AgentEventPayload::TurnStarted(AgentTurnStarted {}))
            .await
            .map_err(talos_agent::AgentError::into_talos_error)?];
        admission_events.extend(
            pipeline
                .record_user_message(params.input)
                .await
                .map_err(talos_agent::AgentError::into_talos_error)?,
        );
        let terminal = pipeline
            .record(AgentEventPayload::TurnCompleted(AgentTurnCompleted {
                stop_reason: talos_agent_api::AgentStopReason::EndTurn,
            }))
            .await
            .map_err(talos_agent::AgentError::into_talos_error)?;
        let (settlement_tx, settlement_rx) = tokio::sync::oneshot::channel();
        Ok(AgentRunExecution {
            admission: AgentAdmission {
                events: admission_events,
                permit: AgentAdmissionPermit::new(async move {
                    settlement_tx
                        .send(Ok(AgentRunSettlement::Finished {
                            event: terminal,
                            messages: Vec::new(),
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

    async fn submit_input(&self, _params: AgentInputParams) -> TalosResult<AgentAdmission> {
        unreachable!("gateway test does not submit live input")
    }

    async fn interrupt(&self, _params: AgentInterruptParams) -> TalosResult<AgentInterruptOutcome> {
        Ok(AgentInterruptOutcome::Cancelled)
    }

    async fn compact_session(&self, _params: AgentContext) -> TalosResult<AgentRunExecution> {
        unreachable!("gateway test does not compact Sessions")
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

async fn test_gateway() -> ServiceGateway {
    let config = test_config();
    let (storage, store) = test_session_storage().await;
    let handler = Arc::new(RecordingHandler);
    test_gateway_with_coordinator(config, storage, store, handler).await
}

async fn test_session_storage() -> (Arc<SqliteStorage>, Arc<DatabaseSessionStore>) {
    let storage = Arc::new(SqliteStorage::new(":memory:").await.unwrap());
    let migration_dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../conf/migrations/sqlite");
    run_migrations(storage.pool(), &migration_dir)
        .await
        .unwrap();
    let store = Arc::new(DatabaseSessionStore::new(storage.clone()).await.unwrap());
    (storage, store)
}

async fn test_gateway_with_coordinator(
    config: TalosConfig,
    storage: Arc<SqliteStorage>,
    store: Arc<DatabaseSessionStore>,
    coordinator: Arc<dyn AgentCoordinator>,
) -> ServiceGateway {
    let profile_store: Arc<dyn ProfileStore> =
        Arc::new(DatabaseProfileStore::new(storage.pool().clone()));
    let file_store = Arc::new(DatabaseFileStore::new(storage.pool().clone()));
    let project_store_impl = Arc::new(StorageProjectStore::new(storage).await.unwrap());
    let session_store: Arc<dyn SessionStore> = store.clone();
    let project_store: Arc<dyn ProjectStore> = project_store_impl.clone();
    let files = Arc::new(
        FileService::new(
            file_store,
            project_store.clone(),
            BTreeMap::from([(
                "local".to_string(),
                Operator::new(Memory::default()).unwrap().finish(),
            )]),
            talos_config::FileStorageConfig::default(),
        )
        .unwrap(),
    );
    let event_service = Arc::new(TimelineEventService::new(store.clone()));
    let config_store: Arc<dyn ConfigStore> =
        Arc::new(InMemoryLlmConfigStore::new(config.llm.clone()));
    let auth = Arc::new(TestAuthService);
    let auth_service: Arc<dyn AuthService> = auth.clone();
    let user_service: Arc<dyn UserService> = auth;
    let memory = MemoryService::new(
        config.clone(),
        Arc::new(MemoryStore::in_memory().await.unwrap()),
    )
    .unwrap();
    let reload_hook = test_config_reload_hook(config.clone());
    let service = Arc::new(TalosService::new(
        coordinator,
        session_store,
        store.clone() as Arc<dyn AgentExecutionStore>,
        project_store,
        profile_store,
        files,
        event_service,
        config,
        memory,
        config_store,
        Vec::new(),
        auth_service,
        user_service,
        reload_hook,
    ));
    ServiceGateway::new(service)
}

#[path = "basic_tests/context_usage_tests.rs"]
mod context_usage_tests;

fn test_config() -> TalosConfig {
    let mut config = TalosConfig::default();
    config.llm.provider = "anthropic".to_string();
    config.llm.model = "claude-3-5-sonnet".to_string();
    config.llm.anthropic.models = vec![ProviderModelConfig {
        id: config.llm.model.clone(),
        name: "Claude 3.5 Sonnet".to_string(),
        context_window: 32_768,
        reasoning: false,
        reasoning_effort: Default::default(),
        speed: Default::default(),
        input: Vec::new(),
        cost: Default::default(),
    }];
    config
}

fn request_source() -> SessionSource {
    SessionSource::http()
}

#[tokio::test]
async fn gateway_start_turn_delegates_to_service() {
    let gateway = test_gateway().await;
    let response = gateway
        .start_turn(
            &Actor::regular("user-1"),
            request_source(),
            StartTurnRequest {
                session_id: "session-1".to_string(),
                input: AgentUserInput {
                    content: vec![AgentUserContent::Text {
                        text: "hello".to_string(),
                    }],
                },
                model_selection: None,
                user_permission_mode: None,
                project_id: None,
                current_directory: None,
                client_request_id: Some("client-1".to_string()),
            },
            Some("client-1".to_string()),
        )
        .await
        .expect("start turn should delegate to application service");

    assert_eq!(response.session_id, "session-1");
    assert_eq!(response.status, TurnCommandStatus::Accepted);
}

#[tokio::test]
async fn gateway_start_turn_rejects_invalid_request() {
    let gateway = test_gateway().await;
    let result = gateway
        .start_turn(
            &Actor::regular("user-1"),
            request_source(),
            StartTurnRequest {
                session_id: "session-1".to_string(),
                input: AgentUserInput {
                    content: Vec::new(),
                },
                model_selection: None,
                user_permission_mode: None,
                project_id: None,
                current_directory: None,
                client_request_id: None,
            },
            None,
        )
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn gateway_config_requires_admin_for_regular_auth() {
    let gateway = test_gateway().await;

    let read_error = gateway
        .get_config(
            &Actor::regular("user-1"),
            ConfigGetRequest {
                path: ConfigPath::WebSearch,
            },
        )
        .await
        .expect_err("regular users should not read global config");
    assert!(read_error.is_code(talos_error::ErrorCode::PermissionDenied));

    let update_error = gateway
        .update_config(
            &Actor::regular("user-1"),
            ConfigUpdateRequest {
                value: ConfigValue::WebSearch(Box::default()),
            },
        )
        .await
        .expect_err("regular users should not update global config");
    assert!(update_error.is_code(talos_error::ErrorCode::PermissionDenied));
}

#[tokio::test]
async fn gateway_config_allows_admin_and_auth_disabled() {
    let gateway = test_gateway().await;

    gateway
        .get_config(
            &Actor::regular("admin-1").with_role(UserRole::Admin),
            ConfigGetRequest {
                path: ConfigPath::WebSearch,
            },
        )
        .await
        .expect("admin should read global config");

    gateway
        .update_config(
            &Actor::auth_disabled(),
            ConfigUpdateRequest {
                value: ConfigValue::WebSearch(Box::default()),
            },
        )
        .await
        .expect("auth-disabled mode should update global config");
}

#[tokio::test]
async fn gateway_patch_session_rejects_missing_session_id() {
    let gateway = test_gateway().await;
    let result = gateway
        .patch_session(
            &Actor::regular("user-1"),
            SessionPatchRequest {
                session_id: String::new(),
                conf: None,
                current_directory: None,
            },
        )
        .await;

    assert!(result
        .as_ref()
        .is_err_and(|error| error.is_code(talos_error::ErrorCode::InvalidRequest)));
}

#[tokio::test]
async fn gateway_start_is_available() {
    let gateway = test_gateway().await;
    gateway.start().await.expect("gateway should start");
}
