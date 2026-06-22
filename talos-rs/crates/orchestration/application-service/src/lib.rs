pub mod access;
pub mod account;
pub mod config;
pub mod event;
pub mod file;
pub mod fs;
pub mod history;
pub mod memory;
pub mod model;
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
    TimelineRecoveryState, TimelineSubscribeCommand, TimelineSubscription,
    TimelineSubscriptionCloseReason, TimelineSubscriptionClosed, TimelineSubscriptionMessage,
    TimelineSubscriptionSource,
};
pub use file::{
    content_hash, modified_at_ms, read_optional, validate_expected_version, with_file_lock,
    workspace_root, write_atomic, FileLockRegistry,
};
pub use fs::FsService;
pub use history::SessionHistoryService;
pub use memory::{MemoryDate, MemoryListCommand, MemorySearchCommand, MemoryService};
pub use plugin::PluginManagerService;
pub use session::SessionService;
pub use skills::SkillsService;
pub use turn::{
    turn_submission_hash, AgentDispatchTurnExecutor, TurnExecutor, TurnRetryRequest, TurnService,
};
pub use workspace::{AgentWorkspaceInfo, EffectiveAgentWorkspace, EffectiveWorkspaceResolver};
pub use workspace_docs::WorkspaceDocService;

pub type ApplicationResult<T> = talos_error::TalosResult<T>;

use std::sync::Arc;

use talos_auth::{AuthService, UserService};
use talos_config::LlmConfigService;
use talos_coordinator::SessionRuntime;
use talos_llm_provider::LlmProvider;
use talos_project_store::{ProjectRecord, ProjectStore};
use talos_session_store::{
    IdempotencyStatus, MessageCommandResult, MessageIdempotencyStore, SessionPatchAck,
    SessionStore, TimelineEventId, TimelineEventService,
};
use talos_types::{
    InterruptTurnRequest, InterruptTurnResponse, ModelInfo, ProjectInfo, ProjectPatch,
    RecoverTimelineRequest, RetryTurnRequest as SharedRetryTurnRequest, RetryTurnResponse,
    SessionListRequest, SessionPatch, StartTurnRequest, StartTurnResponse, SubmitToolResultRequest,
    SubmitToolResultResponse, SubscribeTimelineRequest, TalosConfig, TalosSession,
    TimelineActiveTurnSnapshot, TurnCommandStatus,
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
        turn_executor: Arc<dyn TurnExecutor>,
        session_runtime: Arc<dyn SessionRuntime>,
        session_store: Arc<dyn SessionStore>,
        project_store: Arc<dyn ProjectStore>,
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
            turn_executor,
            session_runtime,
            session_store,
            project_store,
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
        turn_executor: Arc<dyn TurnExecutor>,
        session_runtime: Arc<dyn SessionRuntime>,
        session_store: Arc<dyn SessionStore>,
        project_store: Arc<dyn ProjectStore>,
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
        let mut session_records = SessionService::new(
            session_store.clone(),
            project_store.clone(),
            base_config.clone(),
        );
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
            access: ResourceAccessService::with_records(session_runtime.clone(), session_records),
            account: AccountService::new(
                auth_service,
                user_service,
                base_config.cloud.api_base_url.clone(),
            ),
            turns: TurnService::new(
                turn_executor,
                event_service.clone(),
                idempotency_store,
                session_store,
                session_runtime,
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
        turn_executor: Arc<dyn TurnExecutor>,
        session_runtime: Arc<dyn SessionRuntime>,
        session_store: Arc<dyn SessionStore>,
        project_store: Arc<dyn ProjectStore>,
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
            turn_executor,
            session_runtime,
            session_store,
            project_store,
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
        request: SessionListRequest,
    ) -> ApplicationResult<Vec<TalosSession>> {
        self.access.list_sessions(actor, request).await
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

    pub async fn get_active_turn(
        &self,
        actor: &Actor,
        session_id: &str,
    ) -> ApplicationResult<TimelineActiveTurnSnapshot> {
        let _session = self.access.get_session(actor, session_id).await?;
        let active_turn = self.events.get_active_turn(session_id).await?;
        Ok(event::map_active_turn_snapshot(session_id, active_turn))
    }

    async fn submit_start_turn(
        &self,
        actor: &Actor,
        request: StartTurnRequest,
        idempotency_key: Option<String>,
    ) -> ApplicationResult<MessageCommandResult> {
        TurnService::validate_start_turn_request(&request)?;
        if let Some(model) = request.model.as_deref() {
            self.config.validate_model_supported(model)?;
        }
        let session_id = request.session_id.clone();
        let (session, created_session) = match self.access.get_session(actor, &session_id).await {
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
                        &session_id,
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
            .submit_start_turn(session, request, idempotency_key)
            .await;
        if created_session
            && !matches!(&result, Ok(result) if result.status == IdempotencyStatus::Accepted)
        {
            if let Err(error) = self.access.discard_session_record(actor, &session_id).await {
                tracing::error!(
                    session_id = %session_id,
                    error = %error,
                    "failed to discard newly created session after failed turn"
                );
            }
        }
        result
    }

    pub async fn start_turn(
        &self,
        actor: &Actor,
        request: StartTurnRequest,
        idempotency_key: Option<String>,
    ) -> ApplicationResult<StartTurnResponse> {
        let result = self
            .submit_start_turn(actor, request, idempotency_key)
            .await?;
        Ok(StartTurnResponse {
            request_id: result.request_id,
            message_id: result.message_id,
            session_id: result.session_id,
            turn_id: result.turn_id,
            status: turn_command_status(result.status),
        })
    }

    pub async fn submit_retry(
        &self,
        actor: &Actor,
        session_id: &str,
        request: TurnRetryRequest,
    ) -> ApplicationResult<MessageCommandResult> {
        let session = self.access.get_session(actor, session_id).await?;
        self.turns.submit_retry(session, request).await
    }

    pub async fn retry_turn(
        &self,
        actor: &Actor,
        request: SharedRetryTurnRequest,
    ) -> ApplicationResult<RetryTurnResponse> {
        let session_id = request.session_id.clone();
        let result = self
            .submit_retry(
                actor,
                &session_id,
                TurnRetryRequest {
                    input: request.input,
                    workspace_id: request.workspace_id,
                    client_message_id: request.client_message_id,
                    retry_of_message_id: request.retry_of_message_id,
                },
            )
            .await?;
        Ok(RetryTurnResponse {
            request_id: result.request_id,
            message_id: result.message_id,
            session_id: result.session_id,
            turn_id: result.turn_id,
            status: turn_command_status(result.status),
        })
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

    pub async fn interrupt_turn(
        &self,
        actor: &Actor,
        request: InterruptTurnRequest,
    ) -> ApplicationResult<InterruptTurnResponse> {
        self.cancel_active_turn(actor, &request.session_id).await?;
        Ok(InterruptTurnResponse { success: true })
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

    pub async fn submit_tool_result(
        &self,
        actor: &Actor,
        request: SubmitToolResultRequest,
    ) -> ApplicationResult<SubmitToolResultResponse> {
        self.submit_dynamic_tool_result(
            actor,
            &request.session_id,
            &request.call_id,
            &request.tool_name,
            request.output,
            request.success,
        )
        .await?;
        Ok(SubmitToolResultResponse { success: true })
    }

    pub async fn subscribe_timeline(
        &self,
        actor: &Actor,
        request: SubscribeTimelineRequest,
    ) -> ApplicationResult<talos_types::SubscribeTimelineResponse> {
        let source = self.open_timeline_subscription(actor, request).await?;
        Ok(source.response)
    }

    pub async fn open_timeline_subscription(
        &self,
        actor: &Actor,
        request: SubscribeTimelineRequest,
    ) -> ApplicationResult<TimelineSubscriptionSource> {
        let _session = self.access.get_session(actor, &request.session_id).await?;
        let session_id = request.session_id.clone();
        let after_event_id = parse_timeline_event_id(request.since_event_id)?;
        let subscription = self
            .events
            .subscribe(TimelineSubscribeCommand {
                session_id: request.session_id,
                after_event_id,
                replay_limit: request.replay_limit.unwrap_or(1).max(1),
                close_when_idle: request.close_when_idle.unwrap_or(false),
                close_on_terminal: request.close_on_terminal.unwrap_or(false),
            })
            .await?;
        let response = talos_types::SubscribeTimelineResponse {
            subscription_id: format!("sub_{}", uuid::Uuid::new_v4().simple()),
            session_id,
            high_water_mark_event_id: subscription
                .high_water_mark_event_id
                .as_ref()
                .map(|value| value.to_string()),
        };
        Ok(TimelineSubscriptionSource {
            response,
            subscription,
        })
    }

    pub async fn recover_timeline(
        &self,
        actor: &Actor,
        request: RecoverTimelineRequest,
    ) -> ApplicationResult<talos_types::TimelineRecoveryResponse> {
        let _session = self.access.get_session(actor, &request.session_id).await?;
        let after_event_id = parse_timeline_event_id(request.after_event_id)?;
        let recovery = self
            .events
            .recover_session(
                &request.session_id,
                after_event_id.as_ref(),
                request.replay_limit.unwrap_or(1).max(1),
            )
            .await?;
        Ok(talos_types::TimelineRecoveryResponse {
            snapshot: talos_types::TimelineRecoverySnapshot {
                replay: talos_types::TimelineReplay {
                    events: recovery.replay.events,
                    high_water_mark_event_id: recovery
                        .replay
                        .high_water_mark_event_id
                        .map(|value| value.to_string()),
                    last_replay_seq: recovery.replay.last_replay_seq,
                    cursor_status: map_timeline_cursor_status(recovery.replay.cursor_status),
                },
                active_turn: recovery.active_turn,
            },
        })
    }

    pub async fn list_projects(&self, actor: &Actor) -> ApplicationResult<Vec<ProjectInfo>> {
        let projects = self.access.list_projects(actor).await?;
        Ok(projects
            .into_iter()
            .map(|project| self.fs.project_status(project))
            .map(project_info_from_record)
            .collect())
    }

    pub async fn get_project(
        &self,
        actor: &Actor,
        project_id: &str,
    ) -> ApplicationResult<ProjectInfo> {
        let project = self.access.get_project(actor, project_id).await?;
        Ok(project_info_from_record(self.fs.project_status(project)))
    }

    pub async fn create_project(
        &self,
        actor: &Actor,
        name: String,
        directory: std::path::PathBuf,
    ) -> ApplicationResult<ProjectInfo> {
        let project = self.access.create_project(actor, name, directory).await?;
        Ok(project_info_from_record(self.fs.project_status(project)))
    }

    pub async fn patch_project(
        &self,
        actor: &Actor,
        project_id: &str,
        patch: ProjectPatch,
    ) -> ApplicationResult<ProjectInfo> {
        let project = self.access.patch_project(actor, project_id, patch).await?;
        Ok(project_info_from_record(self.fs.project_status(project)))
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

fn project_info_from_record(record: ProjectRecord) -> ProjectInfo {
    ProjectInfo {
        project_id: record.project_id,
        name: record.name,
        directory: record.directory,
        status: record.status,
        created_at: record.created_at,
        updated_at: record.updated_at,
        session_count: record.session_count,
    }
}

fn turn_command_status(status: IdempotencyStatus) -> TurnCommandStatus {
    match status {
        IdempotencyStatus::Pending => TurnCommandStatus::Pending,
        IdempotencyStatus::Accepted => TurnCommandStatus::Accepted,
        IdempotencyStatus::Failed => TurnCommandStatus::Failed,
        IdempotencyStatus::FailedNoFirstEvent => TurnCommandStatus::FailedNoFirstEvent,
    }
}

fn parse_timeline_event_id(value: Option<String>) -> ApplicationResult<Option<TimelineEventId>> {
    value
        .map(|raw| {
            let event_id = TimelineEventId::from(raw);
            event_id.event_seq()?;
            Ok(event_id)
        })
        .transpose()
}

fn map_timeline_cursor_status(status: TimelineCursorStatus) -> talos_types::TimelineCursorStatus {
    match status {
        TimelineCursorStatus::Current => talos_types::TimelineCursorStatus::Current,
        TimelineCursorStatus::FromBeginning => talos_types::TimelineCursorStatus::FromBeginning,
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
    use talos_llm_provider::{ChatRequest, ChatResponse, ChatStream, LlmProvider, Usage};
    use talos_project_store::StorageProjectStore;
    use talos_session_store::{
        ActiveTurnStatus, ActiveTurnStore, ActiveTurnUpdate, DatabaseSessionStore,
        IdempotencyReservation, MessageIdempotencyStore, SessionStore,
        TimelineEventRetentionConfig, TimelineEventStore,
    };
    use talos_storage::SqliteStorage;
    use talos_types::agent_dispatch::AgentDispatchRequest;
    use talos_types::talos::{
        TalosContext, TalosInputItem, TalosProvider, TalosSourceType, TalosTimelineEvent,
    };

    enum TestTurnExecutorMode {
        Timeout,
        Closed,
    }

    struct TestTurnExecutor {
        mode: TestTurnExecutorMode,
    }

    struct TestSessionRuntime;
    struct TestLlmProvider;
    struct TestAuthService;

    #[async_trait]
    impl TurnExecutor for TestTurnExecutor {
        async fn start_turn_stream(
            &self,
            _request: AgentDispatchRequest,
        ) -> TalosResult<tokio::sync::mpsc::Receiver<TalosTimelineEvent>> {
            let (tx, rx) = tokio::sync::mpsc::channel(1);
            match self.mode {
                TestTurnExecutorMode::Timeout => {
                    tokio::spawn(async move {
                        tokio::time::sleep(Duration::from_secs(60)).await;
                        drop(tx);
                    });
                }
                TestTurnExecutorMode::Closed => drop(tx),
            }
            Ok(rx)
        }

        async fn cancel_active_turn(&self, _session: TalosSession) -> TalosResult<()> {
            Ok(())
        }

        async fn submit_dynamic_tool_result(
            &self,
            _session: TalosSession,
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

        async fn compact_session(
            &self,
            _session: talos_types::TalosSession,
        ) -> TalosResult<talos_types::TalosUsage> {
            unreachable!("not used in this test")
        }

        async fn clear_session(&self, _session: talos_types::TalosSession) -> TalosResult<()> {
            unreachable!("not used in this test")
        }

        async fn close_session(&self, _session: talos_types::TalosSession) -> TalosResult<()> {
            unreachable!("not used in this test")
        }

        async fn prepare_delete_session(
            &self,
            _session: talos_types::TalosSession,
        ) -> TalosResult<()> {
            Ok(())
        }

        async fn cancel_active_turn(&self, _session: talos_types::TalosSession) -> TalosResult<()> {
            Ok(())
        }

        async fn submit_dynamic_tool_result(
            &self,
            _session: talos_types::TalosSession,
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
        turn_executor: Arc<dyn TurnExecutor>,
        config: TalosConfig,
    ) -> (TalosApplicationService, Arc<DatabaseSessionStore>) {
        let storage = Arc::new(SqliteStorage::new(":memory:").await.unwrap());
        let store = Arc::new(DatabaseSessionStore::new(storage.clone()).await.unwrap());
        let project_store = Arc::new(StorageProjectStore::new(storage).await.unwrap());
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
            turn_executor,
            Arc::new(TestSessionRuntime),
            store.clone(),
            project_store,
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

    fn start_turn_request(session_id: &str, text: &str) -> StartTurnRequest {
        StartTurnRequest {
            session_id: session_id.to_string(),
            input: vec![TalosInputItem::Text {
                text: text.to_string(),
            }],
            workspace_id: None,
            context: Some(request_context()),
            model: None,
            project_id: None,
            current_directory: None,
            client_request_id: None,
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
        assert_eq!(
            recovery.active_turn.status,
            talos_types::TimelineActiveTurnStatus::Completed
        );
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
            Arc::new(TestTurnExecutor {
                mode: TestTurnExecutorMode::Timeout,
            }),
            config,
        )
        .await;

        let result = service
            .start_turn(
                &Actor::regular("member-user"),
                start_turn_request("session-timeout", "hello"),
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
            Arc::new(TestTurnExecutor {
                mode: TestTurnExecutorMode::Closed,
            }),
            TalosConfig::default(),
        )
        .await;

        let result = service
            .start_turn(
                &Actor::regular("member-user"),
                start_turn_request("session-closed", "hello"),
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
            Arc::new(TestTurnExecutor {
                mode: TestTurnExecutorMode::Closed,
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
            .start_turn(
                &Actor::regular("member-user"),
                start_turn_request("session-idempotency-conflict", "hello"),
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
            Arc::new(TestTurnExecutor {
                mode: TestTurnExecutorMode::Closed,
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
            .start_turn(
                &Actor::regular("member-user"),
                start_turn_request("session-active-conflict", "hello"),
                None,
            )
            .await;

        assert!(result.is_err());
        assert_session_creation_rolled_back(&service, &store, "session-active-conflict", None)
            .await;
    }
}
