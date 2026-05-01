pub mod access;
pub mod account;
pub mod admin;
pub mod config;
pub mod event;
pub mod file;
pub mod memory;
pub mod session;
pub mod turn;
pub mod workspace_docs;

pub use access::{Actor, ActorAccess, ResourceAccessService};
pub use account::AccountService;
pub use admin::{AdminSessionPage, DEFAULT_ADMIN_SESSION_PAGE_SIZE, MAX_ADMIN_SESSION_PAGE_SIZE};
pub use config::ConfigService;
pub use event::{ApplicationEvent, ApplicationEventStream, EventReplay, EventService};
pub use file::{
    content_hash, modified_at_ms, read_optional, validate_expected_version, with_file_lock,
    workspace_root, write_atomic, FileLockRegistry,
};
pub use memory::{MemoryDate, MemoryListParams, MemorySearchParams, MemoryService};
pub use session::SessionService;
pub use turn::{message_request_hash, TurnRetryRequest, TurnService};
pub use workspace_docs::WorkspaceDocService;

pub type ApplicationResult<T> = talos_error::TalosResult<T>;

use std::sync::Arc;

use talos_auth::{AuthService, UserService};
use talos_config::LlmConfigService;
use talos_gateway::Gateway;
use talos_llm_provider::LlmProvider;
use talos_memory::MemoryStore;
use talos_session_store::{
    MessageCommandAck, MessageIdempotencyStore, SessionEventService, SessionStore,
};
use talos_types::talos::{TalosContext, TalosMessageRequest, TalosMessageResponse};
use talos_types::{
    ModelInfo, ProjectPatch, ProjectRecord, SessionConf, SessionPatch, SessionPatchAck,
    SessionQuery, SessionRuntime, TalosConfig, TalosSession,
};

#[derive(Clone)]
pub struct TalosApplicationService {
    pub access: ResourceAccessService,
    pub account: AccountService,
    pub turns: TurnService,
    pub events: EventService,
    pub config: ConfigService,
    pub memory: MemoryService,
    pub workspace_docs: WorkspaceDocService,
}

impl TalosApplicationService {
    pub fn new(
        gateway: Arc<dyn Gateway>,
        session_runtime: Arc<dyn SessionRuntime>,
        session_store: Arc<dyn SessionStore>,
        event_service: Arc<SessionEventService>,
        idempotency_store: Arc<dyn MessageIdempotencyStore>,
        base_config: TalosConfig,
        llm_provider: Arc<dyn LlmProvider>,
        llm_config_service: Arc<dyn LlmConfigService>,
        models: Vec<ModelInfo>,
        auth_service: Arc<dyn AuthService>,
        user_service: Arc<dyn UserService>,
    ) -> Self {
        let mut session_records = SessionService::new(session_store, base_config.clone());
        session_records = session_records
            .with_default_workspace_id(base_config.agent.kernel.default_workspace_id.clone());
        session_records = session_records.with_channel_dm_scope(
            "telegram",
            base_config.channels.telegram.dm_scope.to_string(),
        );
        let file_locks = FileLockRegistry::default();
        Self {
            access: ResourceAccessService::with_records(session_runtime, Arc::new(session_records)),
            account: AccountService::new(auth_service, user_service),
            turns: TurnService::new(
                gateway,
                event_service.clone(),
                idempotency_store,
                base_config.clone(),
            ),
            events: EventService::new(event_service),
            config: ConfigService::new(llm_config_service, models),
            memory: MemoryService::new(base_config.clone(), llm_provider, file_locks.clone()),
            workspace_docs: WorkspaceDocService::new(base_config, file_locks),
        }
    }

    pub fn new_with_session_store(
        gateway: Arc<dyn Gateway>,
        session_runtime: Arc<dyn SessionRuntime>,
        session_store: Arc<dyn SessionStore>,
        event_service: Arc<SessionEventService>,
        idempotency_store: Arc<dyn MessageIdempotencyStore>,
        base_config: TalosConfig,
        llm_provider: Arc<dyn LlmProvider>,
        llm_config_service: Arc<dyn LlmConfigService>,
        models: Vec<ModelInfo>,
        auth_service: Arc<dyn AuthService>,
        user_service: Arc<dyn UserService>,
    ) -> Self {
        Self::new(
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
        )
    }

    pub async fn create_session(
        &self,
        actor: &Actor,
        context: TalosContext,
        model: Option<String>,
        conf: Option<SessionConf>,
        project_id: Option<String>,
        current_directory: Option<std::path::PathBuf>,
    ) -> ApplicationResult<TalosSession> {
        if let Some(model) = model.as_deref() {
            self.config.validate_model_supported(model)?;
        }
        self.access
            .create_session(actor, context, model, conf, project_id, current_directory)
            .await
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

    pub async fn get_session_turns(
        &self,
        actor: &Actor,
        session_id: &str,
    ) -> ApplicationResult<Vec<TalosMessageResponse>> {
        let _session = self.access.get_session(actor, session_id).await?;
        let memory_store = MemoryStore::new_default();
        let turns = memory_store.load_session_turns(session_id).await?;

        Ok(turns
            .into_iter()
            .map(|turn| TalosMessageResponse {
                request_id: turn.turn_id.clone(),
                message_id: turn.turn_id.clone(),
                session_id: turn.session_id.clone(),
                model: String::new(),
                reasoning: Vec::new(),
                output: turn.items,
                stop_reason: turn.stop_reason,
                usage: turn.usage,
            })
            .collect())
    }

    pub async fn submit_message(
        &self,
        actor: &Actor,
        session_id: &str,
        request: TalosMessageRequest,
        idempotency_key: Option<String>,
    ) -> ApplicationResult<MessageCommandAck> {
        let session = self.access.get_session(actor, session_id).await?;
        self.turns
            .submit_message(session, request, idempotency_key)
            .await
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

    pub async fn submit_tool_approval(
        &self,
        actor: &Actor,
        session_id: &str,
        call_id: &str,
        tool_name: &str,
        approved: bool,
        reason: Option<String>,
    ) -> ApplicationResult<()> {
        let session = self.access.get_session(actor, session_id).await?;
        self.turns
            .submit_tool_approval(session, call_id, tool_name, approved, reason)
            .await
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
        self.access.list_projects(actor).await
    }

    pub async fn get_project(
        &self,
        actor: &Actor,
        project_id: &str,
    ) -> ApplicationResult<ProjectRecord> {
        self.access.get_project(actor, project_id).await
    }

    pub async fn create_project(
        &self,
        actor: &Actor,
        name: String,
        directory: std::path::PathBuf,
    ) -> ApplicationResult<ProjectRecord> {
        self.access.create_project(actor, name, directory).await
    }

    pub async fn patch_project(
        &self,
        actor: &Actor,
        project_id: &str,
        patch: ProjectPatch,
    ) -> ApplicationResult<ProjectRecord> {
        self.access.patch_project(actor, project_id, patch).await
    }

    pub async fn delete_project(&self, actor: &Actor, project_id: &str) -> ApplicationResult<()> {
        self.access.delete_project(actor, project_id).await
    }

    pub async fn list_admin_sessions(
        &self,
        actor: &Actor,
        query: SessionQuery,
    ) -> ApplicationResult<Vec<TalosSession>> {
        self.access.list_admin_sessions(actor, query).await
    }

    pub async fn count_admin_sessions(
        &self,
        actor: &Actor,
        query: SessionQuery,
    ) -> ApplicationResult<usize> {
        self.access.count_admin_sessions(actor, query).await
    }

    pub async fn delete_admin_session(
        &self,
        actor: &Actor,
        session_id: &str,
    ) -> ApplicationResult<SessionPatchAck> {
        self.access.delete_admin_session(actor, session_id).await
    }

    pub async fn list_admin_projects(
        &self,
        actor: &Actor,
    ) -> ApplicationResult<Vec<ProjectRecord>> {
        self.access.list_admin_projects(actor).await
    }
}
