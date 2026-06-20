pub mod access;
pub mod account;
pub mod config;
pub mod event;
pub mod file;
pub mod fs;
pub mod history;
pub mod memory;
pub mod plugin;
pub mod session;
pub mod skills;
pub mod turn;
pub mod workspace;
pub mod workspace_docs;

pub use access::{Actor, ResourceAccessService};
pub use account::AccountService;
pub use config::{ConfigService, RuntimeConfigReloadHook};
pub use event::{
    ApplicationEvent, ApplicationEventStream, EventReplay, EventService, TimelineCursorStatus,
    TimelineRecoverySnapshot, TimelineSubscribeParams, TimelineSubscription,
    TimelineSubscriptionCloseReason, TimelineSubscriptionClosed, TimelineSubscriptionMessage,
};
pub use file::{
    content_hash, modified_at_ms, read_optional, validate_expected_version, with_file_lock,
    workspace_root, write_atomic, FileLockRegistry,
};
pub use fs::FsService;
pub use history::SessionHistoryService;
pub use memory::{MemoryDate, MemoryListParams, MemorySearchParams, MemoryService};
pub use plugin::PluginManagerService;
pub use session::SessionService;
pub use skills::SkillsService;
pub use turn::{message_request_hash, TurnRetryRequest, TurnService};
pub use workspace::{AgentWorkspaceInfo, EffectiveAgentWorkspace, EffectiveWorkspaceResolver};
pub use workspace_docs::WorkspaceDocService;

pub type ApplicationResult<T> = talos_error::TalosResult<T>;

use std::sync::Arc;

use talos_auth::{AuthService, UserService};
use talos_config::LlmConfigService;
use talos_gateway::Gateway;
use talos_llm_provider::LlmProvider;
use talos_session_store::{
    IdempotencyStatus, MessageCommandAck, MessageIdempotencyStore, SessionStore,
    TimelineEventService,
};
use talos_types::talos::TalosMessageRequest;
use talos_types::{
    ModelInfo, ProjectPatch, ProjectRecord, SessionPatch, SessionPatchAck, SessionQuery,
    SessionRuntime, TalosConfig, TalosSession,
};

#[derive(Clone)]
pub struct TalosApplicationService {
    pub access: ResourceAccessService,
    pub account: AccountService,
    pub turns: TurnService,
    pub events: EventService,
    pub history: SessionHistoryService,
    pub config: ConfigService,
    pub fs: FsService,
    pub memory: MemoryService,
    pub plugins: PluginManagerService,
    pub skills: SkillsService,
    pub workspace_docs: WorkspaceDocService,
}

impl TalosApplicationService {
    pub fn new(
        gateway: Arc<dyn Gateway>,
        session_runtime: Arc<dyn SessionRuntime>,
        session_store: Arc<dyn SessionStore>,
        event_service: Arc<TimelineEventService>,
        idempotency_store: Arc<dyn MessageIdempotencyStore>,
        base_config: TalosConfig,
        llm_provider: Arc<dyn LlmProvider>,
        llm_config_service: Arc<dyn LlmConfigService>,
        models: Vec<ModelInfo>,
        auth_service: Arc<dyn AuthService>,
        user_service: Arc<dyn UserService>,
    ) -> Self {
        Self::new_with_reload_hook(
            gateway,
            session_runtime,
            session_store,
            event_service,
            idempotency_store,
            base_config,
            llm_provider,
            llm_config_service,
            models,
            auth_service,
            user_service,
            None,
        )
    }

    pub fn new_with_reload_hook(
        gateway: Arc<dyn Gateway>,
        session_runtime: Arc<dyn SessionRuntime>,
        session_store: Arc<dyn SessionStore>,
        event_service: Arc<TimelineEventService>,
        idempotency_store: Arc<dyn MessageIdempotencyStore>,
        base_config: TalosConfig,
        llm_provider: Arc<dyn LlmProvider>,
        llm_config_service: Arc<dyn LlmConfigService>,
        models: Vec<ModelInfo>,
        auth_service: Arc<dyn AuthService>,
        user_service: Arc<dyn UserService>,
        reload_hook: Option<RuntimeConfigReloadHook>,
    ) -> Self {
        let mut session_records = SessionService::new(session_store, base_config.clone());
        session_records = session_records
            .with_default_workspace_id(base_config.agent.kernel.default_workspace_id.clone());
        session_records = session_records.with_channel_dm_scope(
            "telegram",
            base_config.channels.telegram.dm_scope.to_string(),
        );
        let session_records = Arc::new(session_records);
        let reload_hook: Option<RuntimeConfigReloadHook> = {
            let session_records = session_records.clone();
            Some(Arc::new(move |llm: &talos_config::LlmConfig| {
                session_records.update_base_llm(llm);
                if let Some(hook) = &reload_hook {
                    hook(llm)?;
                }
                Ok(())
            }))
        };
        let file_locks = FileLockRegistry::default();
        let plugins = PluginManagerService::new(
            base_config.agent.kernel.plugins.clone(),
            base_config.agent.kernel.skills.clone(),
        );
        let skills = SkillsService::new(
            base_config.agent.kernel.skills.clone(),
            base_config.agent.kernel.plugins.clone(),
        )
        .with_plugin_manager(plugins.clone());

        Self {
            access: ResourceAccessService::with_records(session_runtime, session_records),
            account: AccountService::new(auth_service, user_service),
            turns: TurnService::new(
                gateway,
                event_service.clone(),
                idempotency_store,
                base_config.clone(),
            ),
            events: EventService::new(event_service.clone()),
            history: SessionHistoryService::new(base_config.clone(), event_service.clone()),
            config: ConfigService::new_with_reload_hook(llm_config_service, models, reload_hook),
            fs: FsService::new(base_config.fs.directory_picker.clone()),
            memory: MemoryService::new(base_config.clone(), llm_provider, file_locks.clone()),
            plugins,
            skills,
            workspace_docs: WorkspaceDocService::new(base_config, file_locks),
        }
    }

    pub fn new_with_session_store(
        gateway: Arc<dyn Gateway>,
        session_runtime: Arc<dyn SessionRuntime>,
        session_store: Arc<dyn SessionStore>,
        event_service: Arc<TimelineEventService>,
        idempotency_store: Arc<dyn MessageIdempotencyStore>,
        base_config: TalosConfig,
        llm_provider: Arc<dyn LlmProvider>,
        llm_config_service: Arc<dyn LlmConfigService>,
        models: Vec<ModelInfo>,
        auth_service: Arc<dyn AuthService>,
        user_service: Arc<dyn UserService>,
    ) -> Self {
        Self::new_with_reload_hook(
            gateway,
            session_runtime,
            session_store,
            event_service,
            idempotency_store,
            base_config,
            llm_provider,
            llm_config_service,
            models,
            auth_service,
            user_service,
            None,
        )
    }

    pub async fn patch_session(
        &self,
        actor: &Actor,
        session_id: &str,
        patch: SessionPatch,
    ) -> ApplicationResult<SessionPatchAck> {
        if let Some(model) = patch.agent_model()? {
            self.config.validate_model_supported(&model)?;
        }
        self.access.patch_session(actor, session_id, patch).await
    }

    pub async fn list_sessions(
        &self,
        actor: &Actor,
        query: SessionQuery,
    ) -> ApplicationResult<Vec<TalosSession>> {
        self.access.list_sessions(actor, query).await
    }

    pub async fn get_session(
        &self,
        actor: &Actor,
        session_id: &str,
    ) -> ApplicationResult<TalosSession> {
        self.access.get_session(actor, session_id).await
    }

    pub async fn get_session_history(
        &self,
        actor: &Actor,
        session_id: &str,
    ) -> ApplicationResult<talos_types::talos::SessionHistorySnapshot> {
        let _session = self.access.get_session(actor, session_id).await?;
        self.history.snapshot(session_id).await
    }

    pub async fn submit_message(
        &self,
        actor: &Actor,
        session_id: &str,
        request: TalosMessageRequest,
        idempotency_key: Option<String>,
    ) -> ApplicationResult<MessageCommandAck> {
        TurnService::validate_message_request(&request)?;
        if let Some(model) = request.model.as_deref() {
            self.config.validate_model_supported(model)?;
        }
        let (session, created_session) = match self.access.get_session(actor, session_id).await {
            Ok(session) => (session, false),
            Err(talos_error::TalosError::ResourceNotFound(_)) => {
                let context = request.context.clone().ok_or_else(|| {
                    talos_error::TalosError::ValidationError(
                        "message context is required to create session".to_string(),
                    )
                })?;
                let session = self
                    .access
                    .get_or_create_session(
                        actor,
                        session_id,
                        context,
                        request.model.clone(),
                        request.project_id.clone(),
                        request.current_directory.clone(),
                    )
                    .await?;
                (session, true)
            }
            Err(error) => return Err(error),
        };
        let result = self
            .turns
            .submit_message(session, request, idempotency_key)
            .await;
        if created_session
            && !matches!(&result, Ok(ack) if ack.status == IdempotencyStatus::Accepted)
        {
            if let Err(error) = self.access.discard_session_record(actor, session_id).await {
                tracing::error!(
                    session_id = %session_id,
                    error = %error,
                    "failed to discard newly created session after failed turn"
                );
            }
        }
        result
    }

    pub async fn submit_retry(
        &self,
        actor: &Actor,
        session_id: &str,
        request: TurnRetryRequest,
    ) -> ApplicationResult<MessageCommandAck> {
        let session = self.access.get_session(actor, session_id).await?;
        self.turns.submit_retry(session, request).await
    }

    pub async fn compact_session(
        &self,
        actor: &Actor,
        session_id: &str,
    ) -> ApplicationResult<TalosSession> {
        self.access.compact_session(actor, session_id).await
    }

    pub async fn clear_session(
        &self,
        actor: &Actor,
        session_id: &str,
    ) -> ApplicationResult<TalosSession> {
        self.access.clear_session(actor, session_id).await
    }

    pub async fn close_session(&self, actor: &Actor, session_id: &str) -> ApplicationResult<()> {
        self.access.close_session(actor, session_id).await
    }

    pub async fn delete_session(
        &self,
        actor: &Actor,
        session_id: &str,
    ) -> ApplicationResult<SessionPatchAck> {
        self.access.delete_session(actor, session_id).await
    }

    pub async fn cancel_active_turn(
        &self,
        actor: &Actor,
        session_id: &str,
    ) -> ApplicationResult<()> {
        self.access.cancel_active_turn(actor, session_id).await
    }

    pub async fn submit_dynamic_tool_result(
        &self,
        actor: &Actor,
        session_id: &str,
        call_id: &str,
        tool_name: &str,
        output: String,
        success: bool,
    ) -> ApplicationResult<()> {
        let session = self.access.get_session(actor, session_id).await?;
        self.turns
            .submit_dynamic_tool_result(session, call_id, tool_name, output, success)
            .await
    }

    pub async fn list_projects(&self, actor: &Actor) -> ApplicationResult<Vec<ProjectRecord>> {
        let projects = self.access.list_projects(actor).await?;
        Ok(projects
            .into_iter()
            .map(|project| self.fs.project_status(project))
            .collect())
    }

    pub async fn get_project(
        &self,
        actor: &Actor,
        project_id: &str,
    ) -> ApplicationResult<ProjectRecord> {
        let project = self.access.get_project(actor, project_id).await?;
        Ok(self.fs.project_status(project))
    }

    pub async fn create_project(
        &self,
        actor: &Actor,
        name: String,
        directory: std::path::PathBuf,
    ) -> ApplicationResult<ProjectRecord> {
        let project = self.access.create_project(actor, name, directory).await?;
        Ok(self.fs.project_status(project))
    }

    pub async fn patch_project(
        &self,
        actor: &Actor,
        project_id: &str,
        patch: ProjectPatch,
    ) -> ApplicationResult<ProjectRecord> {
        let project = self.access.patch_project(actor, project_id, patch).await?;
        Ok(self.fs.project_status(project))
    }

    pub async fn delete_project(&self, actor: &Actor, project_id: &str) -> ApplicationResult<()> {
        self.access.delete_project(actor, project_id).await
    }

    pub async fn read_fs_directory(
        &self,
        actor: &Actor,
        path: Option<std::path::PathBuf>,
    ) -> ApplicationResult<talos_types::FsDirectorySnapshot> {
        self.fs.read_directory(actor, path).await
    }

    pub async fn create_fs_directory(
        &self,
        actor: &Actor,
        parent: std::path::PathBuf,
        name: String,
    ) -> ApplicationResult<talos_types::FsDirectoryCreateResponse> {
        self.fs.create_directory(actor, parent, name).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Arc;
    use std::time::Duration;
    use talos_auth::{
        AuthResponse, AuthService, LoginRequest, RegisterRequest, RequestIdentity, UserDto,
        UserService,
    };
    use talos_config::{InMemoryLlmConfigStore, LlmConfigManager, LlmConfigService};
    use talos_error::{TalosError, TalosResult};
    use talos_gateway::{GatewayRequest, GatewayResponse};
    use talos_llm_provider::{ChatRequest, ChatResponse, ChatStream, LlmProvider, Usage};
    use talos_session_store::{
        ActiveTurnStatus, ActiveTurnStore, ActiveTurnUpdate, DatabaseSessionStore,
        IdempotencyReservation, MessageIdempotencyStore, SessionStore,
        TimelineEventRetentionConfig, TimelineEventStore,
    };
    use talos_storage::SqliteStorage;
    use talos_types::talos::{
        StopReason, TalosContext, TalosInputItem, TalosOutputItem, TalosProvider, TalosReasoning,
        TalosSourceType, TalosTimelineEvent, TalosUsage,
    };

    enum TestGatewayMode {
        Timeout,
        Closed,
    }

    struct TestGateway {
        mode: TestGatewayMode,
    }

    struct TestSessionRuntime;
    struct TestLlmProvider;
    struct TestAuthService;

    #[async_trait]
    impl Gateway for TestGateway {
        async fn handle_request(&self, _request: GatewayRequest) -> TalosResult<GatewayResponse> {
            Ok(GatewayResponse {
                content: "ok".to_string(),
                output_items: vec![TalosOutputItem::OutputText {
                    text: "ok".to_string(),
                }],
                reasoning: vec![TalosReasoning {
                    text: String::new(),
                }],
                stop_reason: StopReason::EndTurn,
                usage: TalosUsage::default(),
                success: true,
                model: "default".to_string(),
                session_id: "session-1".to_string(),
            })
        }

        async fn handle_request_stream(
            &self,
            _request: GatewayRequest,
        ) -> TalosResult<tokio::sync::mpsc::Receiver<TalosTimelineEvent>> {
            let (tx, rx) = tokio::sync::mpsc::channel(1);
            match self.mode {
                TestGatewayMode::Timeout => {
                    tokio::spawn(async move {
                        tokio::time::sleep(Duration::from_secs(60)).await;
                        drop(tx);
                    });
                }
                TestGatewayMode::Closed => drop(tx),
            }
            Ok(rx)
        }

        async fn start(&self) -> TalosResult<()> {
            Ok(())
        }

        async fn cancel_active_turn(&self, _session_id: &str) -> TalosResult<()> {
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
    }

    #[async_trait]
    impl SessionRuntime for TestSessionRuntime {
        async fn resolve_target_agent(&self, _context: &TalosContext) -> TalosResult<String> {
            Ok("default".to_string())
        }

        async fn compact_session(&self, _session_id: &str) -> TalosResult<TalosSession> {
            unreachable!("not used in this test")
        }

        async fn clear_session(&self, _session_id: &str) -> TalosResult<TalosSession> {
            unreachable!("not used in this test")
        }

        async fn close_session(&self, _session_id: &str) -> TalosResult<()> {
            unreachable!("not used in this test")
        }

        async fn delete_session(&self, _session_id: &str) -> TalosResult<SessionPatchAck> {
            unreachable!("rollback deletes the just-created record directly")
        }

        async fn cancel_active_turn(&self, _session_id: &str) -> TalosResult<()> {
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
            unreachable!("not used in this test")
        }
    }

    #[async_trait]
    impl LlmProvider for TestLlmProvider {
        fn name(&self) -> &str {
            "test"
        }

        async fn chat(&self, _request: ChatRequest) -> TalosResult<ChatResponse> {
            Ok(ChatResponse {
                content: String::new(),
                reasoning: Vec::new(),
                model: "default".to_string(),
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

        fn default_model(&self) -> String {
            "default".to_string()
        }
    }

    #[async_trait]
    impl AuthService for TestAuthService {
        async fn register(&self, _input: RegisterRequest) -> TalosResult<AuthResponse> {
            Err(TalosError::AuthError("not available".to_string()))
        }

        async fn login(&self, _input: LoginRequest) -> TalosResult<AuthResponse> {
            Err(TalosError::AuthError("not available".to_string()))
        }

        async fn logout(&self, _actor: &RequestIdentity) -> TalosResult<()> {
            Ok(())
        }

        async fn verify_access_token(&self, token: &str) -> TalosResult<RequestIdentity> {
            Ok(RequestIdentity::mock_admin(token).with_access_token(token))
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
                disabled: false,
                created_at: 0,
                updated_at: 0,
            })
        }
    }

    async fn test_application(
        gateway: Arc<dyn Gateway>,
        config: TalosConfig,
    ) -> (TalosApplicationService, Arc<DatabaseSessionStore>) {
        let storage = Arc::new(SqliteStorage::new(":memory:").await.unwrap());
        let store = Arc::new(DatabaseSessionStore::new(storage).await.unwrap());
        let event_store: Arc<dyn TimelineEventStore> = store.clone();
        let active_turn_store: Arc<dyn ActiveTurnStore> = store.clone();
        let event_service = Arc::new(TimelineEventService::new(
            event_store,
            active_turn_store,
            TimelineEventRetentionConfig::default(),
        ));
        let idempotency_store: Arc<dyn MessageIdempotencyStore> = store.clone();
        let llm_config_service: Arc<dyn LlmConfigService> = Arc::new(LlmConfigManager::new(
            Arc::new(InMemoryLlmConfigStore::new(config.llm.clone())),
        ));
        let auth = Arc::new(TestAuthService);
        let auth_service: Arc<dyn AuthService> = auth.clone();
        let user_service: Arc<dyn UserService> = auth;
        let service = TalosApplicationService::new_with_session_store(
            gateway,
            Arc::new(TestSessionRuntime),
            store.clone(),
            event_service,
            idempotency_store,
            config,
            Arc::new(TestLlmProvider),
            llm_config_service,
            Vec::new(),
            auth_service,
            user_service,
        );
        (service, store)
    }

    fn request_context() -> TalosContext {
        TalosContext {
            source_type: TalosSourceType::Http,
            provider: TalosProvider::Http,
            user_id: "client-user".to_string(),
            message_id: "client-message".to_string(),
            trace_id: "trace-1".to_string(),
            channel_id: None,
            thread_id: None,
            project_root: None,
            environment: None,
        }
    }

    fn message_request(text: &str) -> TalosMessageRequest {
        TalosMessageRequest {
            input: vec![TalosInputItem::Text {
                text: text.to_string(),
            }],
            workspace_id: None,
            context: Some(request_context()),
            model: None,
            project_id: None,
            current_directory: None,
        }
    }

    async fn assert_session_absent(store: &DatabaseSessionStore, session_id: &str) {
        assert!(store
            .get_session_record(session_id)
            .await
            .expect("session read should succeed")
            .is_none());
    }

    async fn assert_session_creation_rolled_back(
        service: &TalosApplicationService,
        store: &DatabaseSessionStore,
        session_id: &str,
        idempotency_key: Option<&str>,
    ) {
        assert_session_absent(store, session_id).await;
        assert!(service
            .history
            .snapshot(session_id)
            .await
            .expect("history snapshot should succeed")
            .messages
            .is_empty());
        let recovery = service
            .events
            .recover_session(session_id, None, 32)
            .await
            .expect("recovery should succeed");
        assert!(recovery.replay.events.is_empty());
        assert!(recovery.active_turn.is_none());
        assert!(service
            .events
            .get_active_turn(session_id)
            .await
            .expect("active turn lookup should succeed")
            .is_none());
        if let Some(key) = idempotency_key {
            assert!(store
                .get(session_id, key)
                .await
                .expect("idempotency lookup should succeed")
                .is_none());
            let reservation = store
                .reserve(
                    session_id,
                    key,
                    "new-request-hash",
                    "turn-new",
                    chrono::Utc::now().timestamp_millis(),
                    None,
                )
                .await
                .expect("idempotency key should be reusable after rollback");
            assert!(matches!(reservation, IdempotencyReservation::Reserved(_)));
        }
    }

    #[tokio::test]
    async fn first_turn_timeout_does_not_leave_session_record() {
        let mut config = TalosConfig::default();
        config.agent.kernel.timeline_events.first_event_timeout_ms = 1;
        let (service, store) = test_application(
            Arc::new(TestGateway {
                mode: TestGatewayMode::Timeout,
            }),
            config,
        )
        .await;

        let result = service
            .submit_message(
                &Actor::regular("member-user"),
                "session-timeout",
                message_request("hello"),
                Some("timeout-key".to_string()),
            )
            .await;

        assert!(result.is_err());
        assert_session_creation_rolled_back(
            &service,
            &store,
            "session-timeout",
            Some("timeout-key"),
        )
        .await;
    }

    #[tokio::test]
    async fn first_turn_closed_stream_does_not_leave_session_record() {
        let (service, store) = test_application(
            Arc::new(TestGateway {
                mode: TestGatewayMode::Closed,
            }),
            TalosConfig::default(),
        )
        .await;

        let result = service
            .submit_message(
                &Actor::regular("member-user"),
                "session-closed",
                message_request("hello"),
                Some("closed-key".to_string()),
            )
            .await;

        assert!(result.is_err());
        assert_session_creation_rolled_back(&service, &store, "session-closed", Some("closed-key"))
            .await;
    }

    #[tokio::test]
    async fn first_turn_idempotency_conflict_does_not_leave_session_record() {
        let (service, store) = test_application(
            Arc::new(TestGateway {
                mode: TestGatewayMode::Closed,
            }),
            TalosConfig::default(),
        )
        .await;
        let reservation = store
            .reserve(
                "session-idempotency-conflict",
                "conflict-key",
                "different-request-hash",
                "turn-existing",
                1,
                None,
            )
            .await
            .expect("idempotency reserve should succeed");
        assert!(matches!(reservation, IdempotencyReservation::Reserved(_)));

        let result = service
            .submit_message(
                &Actor::regular("member-user"),
                "session-idempotency-conflict",
                message_request("hello"),
                Some("conflict-key".to_string()),
            )
            .await;

        assert!(matches!(result, Err(TalosError::IdempotencyConflict(_))));
        assert_session_creation_rolled_back(
            &service,
            &store,
            "session-idempotency-conflict",
            Some("conflict-key"),
        )
        .await;
    }

    #[tokio::test]
    async fn first_turn_active_turn_conflict_does_not_leave_session_record() {
        let (service, store) = test_application(
            Arc::new(TestGateway {
                mode: TestGatewayMode::Closed,
            }),
            TalosConfig::default(),
        )
        .await;
        store
            .upsert_active_turn(ActiveTurnUpdate {
                session_id: "session-active-conflict".to_string(),
                turn_id: "turn-existing".to_string(),
                message_id: None,
                status: ActiveTurnStatus::Running,
                last_event_id: None,
                pending_user_action: None,
                updated_at: 1,
            })
            .await
            .expect("active turn insert should succeed");

        let result = service
            .submit_message(
                &Actor::regular("member-user"),
                "session-active-conflict",
                message_request("hello"),
                None,
            )
            .await;

        assert!(result.is_err());
        assert_session_creation_rolled_back(&service, &store, "session-active-conflict", None)
            .await;
    }
}
