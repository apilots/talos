use std::sync::Arc;

use talos_auth::{AuthService, UserService};
use talos_config::LlmConfigService;
use talos_coordinator::SessionRuntime;
use talos_llm_provider::LlmProvider;
use talos_store::project::ProjectStore;
use talos_store::session::{
    ActiveTurnStatus, IdempotencyStatus, MessageCommandResult, MessageIdempotencyStore,
    SessionPatchAck, SessionStore, TimelineEventId,
};
use talos_types::{
    InterruptTurnRequest, InterruptTurnResponse, ModelInfo, ProjectInfo, ProjectPatch,
    RecoverTimelineRequest, RetryTurnRequest as SharedRetryTurnRequest, RetryTurnResponse,
    SessionListRequest, SessionPatch, StartTurnRequest, StartTurnResponse,
    SubmitDynamicToolResultRequest, SubmitDynamicToolResultResponse, SubscribeTimelineRequest,
    TalosConfig, TalosSession, TimelineActiveTurnSnapshot,
};

use super::mapping::{map_timeline_cursor_status, project_info_from_record, turn_command_status};
use super::types::ServiceResult;
use crate::access::ResourceAccessService;
use crate::fs::FsService;
use crate::project::ProjectService;
use crate::{
    AccountService, Actor, ConfigService, MemoryService, PluginManagerService,
    RuntimeConfigReloadHook, SessionHistoryService, SessionService, SkillsService,
    TimelineEventService, TimelineSubscribeCommand, TimelineSubscriptionService,
    TimelineSubscriptionSource, TurnExecutor, TurnRetryParams, TurnService, WorkspaceDocService,
};

#[derive(Clone)]
pub struct TalosService {
    access: ResourceAccessService,
    pub account: AccountService,
    turns: TurnService,
    pub events: TimelineSubscriptionService,
    history: SessionHistoryService,
    pub config: ConfigService,
    fs: FsService,
    pub memory: MemoryService,
    pub plugins: PluginManagerService,
    pub skills: SkillsService,
    pub workspace_docs: WorkspaceDocService,
}

impl TalosService {
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
        let projects = Arc::new(ProjectService::new(project_store));
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
            access: ResourceAccessService::with_records(
                session_runtime.clone(),
                session_records,
                projects,
            ),
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
            events: TimelineSubscriptionService::new(event_service.clone()),
            history: SessionHistoryService::new(base_config.clone(), event_service.clone()),
            config: ConfigService::new_with_reload_hook(llm_config_service, models, reload_hook),
            fs: FsService::new(base_config.fs.directory_picker.clone()),
            memory: MemoryService::new(base_config.clone(), llm_provider),
            plugins,
            skills,
            workspace_docs: WorkspaceDocService::new(base_config),
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
    ) -> ServiceResult<SessionPatchAck> {
        if let Some(model) = patch.agent_model()? {
            self.config.validate_model_supported(&model)?;
        }
        self.access.patch_session(actor, session_id, patch).await
    }

    pub async fn list_sessions(
        &self,
        actor: &Actor,
        request: SessionListRequest,
    ) -> ServiceResult<Vec<TalosSession>> {
        self.access.list_sessions(actor, request).await
    }

    pub async fn get_session(
        &self,
        actor: &Actor,
        session_id: &str,
    ) -> ServiceResult<TalosSession> {
        self.access.get_session(actor, session_id).await
    }

    pub async fn create_session(
        &self,
        actor: &Actor,
        session_id: &str,
        context: talos_types::talos::TalosContext,
        model: Option<String>,
        workspace_id: Option<String>,
        project_id: Option<String>,
        current_directory: Option<std::path::PathBuf>,
    ) -> ServiceResult<TalosSession> {
        if let Some(model) = model.as_deref() {
            self.config.validate_model_supported(model)?;
        }
        self.access
            .get_or_create_session(
                actor,
                session_id,
                context,
                model,
                workspace_id,
                project_id,
                current_directory,
            )
            .await
    }

    pub async fn get_session_history(
        &self,
        actor: &Actor,
        session_id: &str,
    ) -> ServiceResult<talos_types::talos::SessionHistorySnapshot> {
        let _session = self.access.get_session(actor, session_id).await?;
        self.history.snapshot(session_id).await
    }

    pub async fn get_active_turn(
        &self,
        actor: &Actor,
        session_id: &str,
    ) -> ServiceResult<TimelineActiveTurnSnapshot> {
        let _session = self.access.get_session(actor, session_id).await?;
        self.events.get_active_turn_snapshot(session_id).await
    }

    async fn submit_start_turn(
        &self,
        actor: &Actor,
        request: StartTurnRequest,
        idempotency_key: Option<String>,
    ) -> ServiceResult<MessageCommandResult> {
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
                        request.workspace_id.clone(),
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
    ) -> ServiceResult<StartTurnResponse> {
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
        request: TurnRetryParams,
    ) -> ServiceResult<MessageCommandResult> {
        let session = self.access.get_session(actor, session_id).await?;
        self.turns.submit_retry(session, request).await
    }

    pub async fn retry_turn(
        &self,
        actor: &Actor,
        request: SharedRetryTurnRequest,
    ) -> ServiceResult<RetryTurnResponse> {
        let session_id = request.session_id.clone();
        let result = self
            .submit_retry(
                actor,
                &session_id,
                TurnRetryParams {
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
    ) -> ServiceResult<TalosSession> {
        self.access.compact_session(actor, session_id).await
    }

    pub async fn clear_session(
        &self,
        actor: &Actor,
        session_id: &str,
    ) -> ServiceResult<TalosSession> {
        self.access.clear_session(actor, session_id).await
    }

    pub async fn close_session(&self, actor: &Actor, session_id: &str) -> ServiceResult<()> {
        self.access.close_session(actor, session_id).await
    }

    pub async fn delete_session(
        &self,
        actor: &Actor,
        session_id: &str,
    ) -> ServiceResult<SessionPatchAck> {
        self.access.delete_session(actor, session_id).await
    }

    pub async fn cancel_active_turn(&self, actor: &Actor, session_id: &str) -> ServiceResult<()> {
        let _session = self.access.get_session(actor, session_id).await?;
        let Some(active_turn) = self.events.get_active_turn(session_id).await? else {
            return Ok(());
        };
        match active_turn.status {
            ActiveTurnStatus::Running
            | ActiveTurnStatus::WaitingUserAction
            | ActiveTurnStatus::Cancelling => {}
            ActiveTurnStatus::Completed | ActiveTurnStatus::Failed => return Ok(()),
        }
        self.access
            .cancel_active_turn(actor, session_id, active_turn.turn_id)
            .await
    }

    pub async fn interrupt_turn(
        &self,
        actor: &Actor,
        request: InterruptTurnRequest,
    ) -> ServiceResult<InterruptTurnResponse> {
        self.cancel_active_turn(actor, &request.session_id).await?;
        Ok(InterruptTurnResponse { success: true })
    }

    pub async fn submit_dynamic_tool_result_for_session_fields(
        &self,
        actor: &Actor,
        session_id: &str,
        call_id: &str,
        tool_name: &str,
        output: String,
        success: bool,
    ) -> ServiceResult<()> {
        let session = self.access.get_session(actor, session_id).await?;
        self.turns
            .submit_dynamic_tool_result(session, call_id, tool_name, output, success)
            .await
    }

    pub async fn submit_dynamic_tool_result(
        &self,
        actor: &Actor,
        request: SubmitDynamicToolResultRequest,
    ) -> ServiceResult<SubmitDynamicToolResultResponse> {
        self.submit_dynamic_tool_result_for_session_fields(
            actor,
            &request.session_id,
            &request.call_id,
            &request.tool_name,
            request.output,
            request.success,
        )
        .await?;
        Ok(SubmitDynamicToolResultResponse { success: true })
    }

    pub async fn subscribe_timeline(
        &self,
        actor: &Actor,
        request: SubscribeTimelineRequest,
    ) -> ServiceResult<talos_types::SubscribeTimelineResponse> {
        let source = self.timeline_subscription_source(actor, request).await?;
        Ok(source.response)
    }

    pub async fn timeline_subscription_source(
        &self,
        actor: &Actor,
        request: SubscribeTimelineRequest,
    ) -> ServiceResult<TimelineSubscriptionSource> {
        let _session = self.access.get_session(actor, &request.session_id).await?;
        let session_id = request.session_id.clone();
        let after_event_id = parse_timeline_event_id(request.since_event_id)?;
        let subscription = self
            .events
            .subscribe(TimelineSubscribeCommand {
                session_id: request.session_id,
                turn_id: request.turn_id,
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
    ) -> ServiceResult<talos_types::TimelineRecoveryResponse> {
        let _session = self.access.get_session(actor, &request.session_id).await?;
        let after_event_id = parse_timeline_event_id(request.after_event_id)?;
        let recovery = self
            .events
            .recover_timeline(
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

    pub async fn list_projects(&self, actor: &Actor) -> ServiceResult<Vec<ProjectInfo>> {
        let projects = self.access.list_projects(actor).await?;
        Ok(projects
            .into_iter()
            .map(|project| self.project_info_with_status(project))
            .collect())
    }

    pub async fn get_project(&self, actor: &Actor, project_id: &str) -> ServiceResult<ProjectInfo> {
        let project = self.access.get_project(actor, project_id).await?;
        Ok(self.project_info_with_status(project))
    }

    pub async fn create_project(
        &self,
        actor: &Actor,
        name: String,
        directory: std::path::PathBuf,
    ) -> ServiceResult<ProjectInfo> {
        let project = self.access.create_project(actor, name, directory).await?;
        Ok(self.project_info_with_status(project))
    }

    pub async fn patch_project(
        &self,
        actor: &Actor,
        project_id: &str,
        patch: ProjectPatch,
    ) -> ServiceResult<ProjectInfo> {
        let project = self.access.patch_project(actor, project_id, patch).await?;
        Ok(self.project_info_with_status(project))
    }

    pub async fn delete_project(&self, actor: &Actor, project_id: &str) -> ServiceResult<()> {
        self.access.delete_project(actor, project_id).await
    }

    fn project_info_with_status(
        &self,
        mut project: talos_store::project::ProjectRecord,
    ) -> ProjectInfo {
        project.status = self.fs.project_status_for_directory(&project.directory);
        project_info_from_record(project)
    }

    pub async fn read_fs_directory(
        &self,
        actor: &Actor,
        path: Option<std::path::PathBuf>,
    ) -> ServiceResult<talos_types::FsDirectorySnapshot> {
        self.fs.read_directory(actor, path).await
    }

    pub async fn create_fs_directory(
        &self,
        actor: &Actor,
        parent: std::path::PathBuf,
        name: String,
    ) -> ServiceResult<talos_types::FsDirectoryCreateResponse> {
        self.fs.create_directory(actor, parent, name).await
    }
}

fn parse_timeline_event_id(value: Option<String>) -> ServiceResult<Option<TimelineEventId>> {
    value
        .map(|raw| {
            let event_id = TimelineEventId::from(raw);
            event_id.event_seq()?;
            Ok(event_id)
        })
        .transpose()
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
