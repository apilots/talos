use std::sync::Arc;

use talos_auth::{AuthService, UserService};
use talos_config::{Config, ConfigStore};
use talos_coordinator::{AgentSessionCompactionIdentity, SessionRuntime};
use talos_error::TalosError;
use talos_store::profile::ProfileStore;
use talos_store::project::ProjectStore;
use talos_store::session::{
    ActiveTurnStatus, IdempotencyStatus, MessageCommandResult, MessageIdempotencyStore,
    SessionArtifactStore, SessionPatchAck, SessionStore, TimelineEventId,
};
use talos_types::{
    AgentTreeSnapshot, ConfigValue, FollowUpSubagentRequest, InterruptSubagentRequest,
    ListSubagentsRequest, MessageQueue, MessageQueueAddRequest, MessageQueueDeleteRequest,
    MessageQueueListRequest, MessageQueueReorderRequest, MessageQueueStartRequest,
    MessageQueueSteerRequest, MessageQueueUpdateRequest, ModelInfo, OptionalCurrentDirectoryPatch,
    RecoverTimelineRequest, RetryTurnRequest as SharedRetryTurnRequest, RetryTurnResponse,
    SendMessageResult, SendSubagentMessageRequest, SessionConfigOverridePolicy, SessionListRequest,
    SessionPatch, SpawnSubagentRequest, StartTurnRequest, StartTurnResponse, SubagentSnapshot,
    SubagentSpawnResult, SubmitDynamicToolResultRequest, SubmitDynamicToolResultResponse,
    SubscribeTimelineRequest, TalosConfig, TalosSession, TimelineActiveTurnSnapshot,
    WaitSubagentRequest,
};

use super::mapping::{map_timeline_cursor_status, turn_command_status};
use super::types::ServiceResult;
use crate::access::{CreateSessionRequest, ResourceAccessService};
use crate::file::FileService;
use crate::fs::FsService;
use crate::project::ProjectService;
use crate::subagent::SubagentService;
use crate::{
    AccountService, Actor, ConfigService, MemoryService, PluginManagerService, ProfileService,
    RuntimeConfigReloadHook, RuntimeConfigReloadTrigger, SessionHistoryService,
    SessionMessageQueueService, SessionService, SkillsService, TimelineEventService,
    TimelineSubscribeCommand, TimelineSubscriptionService, TimelineSubscriptionSource,
    TurnExecutor, TurnRetryParams, TurnService, WorkspaceDocService,
};

#[path = "interrupt.rs"]
mod interrupt;
#[path = "message_queue.rs"]
mod message_queue;
#[path = "orphan_turn_recovery.rs"]
mod orphan_turn_recovery;
#[path = "project_fs.rs"]
mod project_fs;

#[derive(Clone)]
pub struct TalosService {
    access: ResourceAccessService,
    pub account: AccountService,
    pub profile: ProfileService,
    turns: TurnService,
    event_log: Arc<TimelineEventService>,
    pub events: TimelineSubscriptionService,
    history: SessionHistoryService,
    message_queue: SessionMessageQueueService,
    subagents: SubagentService,
    pub config: ConfigService,
    pub files: Arc<FileService>,
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
        session_artifact_store: Arc<dyn SessionArtifactStore>,
        project_store: Arc<dyn ProjectStore>,
        profile_store: Arc<dyn ProfileStore>,
        files: Arc<FileService>,
        event_service: Arc<TimelineEventService>,
        idempotency_store: Arc<dyn MessageIdempotencyStore>,
        base_config: TalosConfig,
        memory: MemoryService,
        config_store: Arc<dyn ConfigStore>,
        models: Vec<ModelInfo>,
        auth_service: Arc<dyn AuthService>,
        user_service: Arc<dyn UserService>,
    ) -> Self {
        Self::new_with_reload_hook(
            turn_executor,
            session_runtime,
            session_store,
            session_artifact_store,
            project_store,
            profile_store,
            files,
            event_service,
            idempotency_store,
            base_config,
            memory,
            config_store,
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
        session_artifact_store: Arc<dyn SessionArtifactStore>,
        project_store: Arc<dyn ProjectStore>,
        profile_store: Arc<dyn ProfileStore>,
        files: Arc<FileService>,
        event_service: Arc<TimelineEventService>,
        idempotency_store: Arc<dyn MessageIdempotencyStore>,
        base_config: TalosConfig,
        memory: MemoryService,
        config_store: Arc<dyn ConfigStore>,
        models: Vec<ModelInfo>,
        auth_service: Arc<dyn AuthService>,
        user_service: Arc<dyn UserService>,
        reload_hook: Option<RuntimeConfigReloadHook>,
    ) -> Self {
        let mut sessions = SessionService::new(
            session_store.clone(),
            project_store.clone(),
            base_config.clone(),
        );
        sessions = sessions
            .with_default_workspace_id(base_config.agent.kernel.default_workspace_id.clone());
        let sessions = Arc::new(sessions);
        let projects = Arc::new(ProjectService::new(project_store.clone()));
        let reload_hook: Option<RuntimeConfigReloadHook> = {
            let sessions = sessions.clone();
            let base_config_state = Arc::new(std::sync::RwLock::new(base_config.clone()));
            Some(Arc::new(move |trigger: RuntimeConfigReloadTrigger| {
                let reloaded_config = if let Some(hook) = &reload_hook {
                    hook(trigger.clone())?
                } else {
                    match &trigger {
                        RuntimeConfigReloadTrigger::Updated(value) => {
                            let mut base_config = base_config_state
                                .write()
                                .expect("service base config write lock");
                            apply_config_value_to_config(&mut base_config, value);
                            base_config.clone()
                        }
                        RuntimeConfigReloadTrigger::Explicit => base_config_state
                            .read()
                            .expect("service base config read lock")
                            .clone(),
                    }
                };
                sessions.update_base_config(reloaded_config.clone());
                Ok(reloaded_config)
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
        let message_queue = SessionMessageQueueService::new(
            base_config.agent.message_queue.max_messages_per_session,
        )
        .expect("message queue config must be validated before service assembly");
        let turns = TurnService::new(
            turn_executor,
            event_service.clone(),
            idempotency_store,
            profile_store.clone(),
            sessions.clone(),
            session_runtime.clone(),
            message_queue.clone(),
            base_config.clone(),
        );
        let history = SessionHistoryService::new(session_artifact_store, event_service.clone());
        let subagents = SubagentService::new(
            sessions.clone(),
            Arc::new(turns.clone()),
            event_service.clone(),
            history.clone(),
        );

        Self {
            access: ResourceAccessService::with_services(
                session_runtime.clone(),
                sessions.clone(),
                projects,
                profile_store.clone(),
            ),
            account: AccountService::new(
                auth_service,
                user_service,
                base_config.cloud.api_base_url.clone(),
            ),
            turns,
            event_log: event_service.clone(),
            profile: ProfileService::new(
                session_store,
                project_store,
                profile_store,
                base_config.agent.kernel.timezone.clone(),
            ),
            events: TimelineSubscriptionService::new(event_service.clone()),
            history,
            message_queue,
            subagents,
            config: ConfigService::new_with_reload_hook(config_store, models, reload_hook),
            files,
            fs: FsService::new(base_config.fs.directory_picker.clone()),
            memory,
            plugins,
            skills,
            workspace_docs: WorkspaceDocService::new(base_config),
        }
    }

    pub fn new_with_session_store(
        turn_executor: Arc<dyn TurnExecutor>,
        session_runtime: Arc<dyn SessionRuntime>,
        session_store: Arc<dyn SessionStore>,
        session_artifact_store: Arc<dyn SessionArtifactStore>,
        project_store: Arc<dyn ProjectStore>,
        profile_store: Arc<dyn ProfileStore>,
        files: Arc<FileService>,
        event_service: Arc<TimelineEventService>,
        idempotency_store: Arc<dyn MessageIdempotencyStore>,
        base_config: TalosConfig,
        memory: MemoryService,
        config_store: Arc<dyn ConfigStore>,
        models: Vec<ModelInfo>,
        auth_service: Arc<dyn AuthService>,
        user_service: Arc<dyn UserService>,
    ) -> Self {
        Self::new_with_reload_hook(
            turn_executor,
            session_runtime,
            session_store,
            session_artifact_store,
            project_store,
            profile_store,
            files,
            event_service,
            idempotency_store,
            base_config,
            memory,
            config_store,
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
        if let Some(selection) = patch.model_selection()? {
            self.config.validate_model_selection_supported(&selection)?;
        }
        let _session = self.access.get_session(actor, session_id).await?;
        self.ensure_session_write_allowed(session_id).await?;
        self.access.patch_session(actor, session_id, patch).await
    }

    pub async fn list_sessions(
        &self,
        actor: &Actor,
        request: SessionListRequest,
    ) -> ServiceResult<Vec<TalosSession>> {
        self.access.list_sessions(actor, request).await
    }

    /// Reads the authenticated user's Profile usage view.
    pub async fn read_profile(
        &self,
        actor: &Actor,
    ) -> ServiceResult<talos_types::ProfileReadResponse> {
        self.profile.read_profile(actor).await
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
        params: CreateSessionRequest,
    ) -> ServiceResult<TalosSession> {
        if let Some(selection) = params.model_selection.as_ref() {
            self.config.validate_model_selection_supported(selection)?;
        }
        self.access.get_or_create_session(actor, params).await
    }

    pub async fn get_session_history(
        &self,
        actor: &Actor,
        session_id: &str,
    ) -> ServiceResult<talos_types::talos::SessionHistorySnapshot> {
        let session = self.access.get_session(actor, session_id).await?;
        self.history.snapshot(&session).await
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
        if let Some(selection) = request.model_selection.as_ref() {
            self.config.validate_model_selection_supported(selection)?;
        }
        let session_id = request.session_id.clone();
        let (session, created_session) = match self.access.get_session(actor, &session_id).await {
            Ok(session) => {
                let session = self
                    .apply_existing_turn_metadata(actor, session, &request)
                    .await?;
                (session, false)
            }
            Err(error) if error.is_code(talos_error::ErrorCode::ResourceNotFound) => {
                let context = request.context.clone().ok_or_else(|| {
                    talos_error::TalosError::public(
                        talos_error::ErrorCode::InvalidRequest,
                        "message context is required to create session".to_string(),
                    )
                })?;
                let session = self
                    .access
                    .get_or_create_session(
                        actor,
                        CreateSessionRequest {
                            session_id: session_id.clone(),
                            context,
                            model_selection: request.model_selection.clone(),
                            user_permission_mode: request.user_permission_mode,
                            workspace_id: request.workspace_id.clone(),
                            project_id: request.project_id.clone(),
                            current_directory: request.current_directory.clone(),
                        },
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

    async fn apply_existing_turn_metadata(
        &self,
        actor: &Actor,
        session: TalosSession,
        request: &StartTurnRequest,
    ) -> ServiceResult<TalosSession> {
        let mut patch = SessionPatch::default();
        if let Some(selection) = request.model_selection.as_ref() {
            patch.conf = Some(SessionConfigOverridePolicy::single_model_selection_conf(
                selection,
            ));
        }
        if let Some(current_directory) = request.current_directory.clone() {
            match session.current_directory.as_ref() {
                Some(bound) if bound != &current_directory => {
                    return Err(talos_error::TalosError::public(
                        talos_error::ErrorCode::InvalidRequest,
                        format!(
                            "session {} is bound to current_directory {}, but request used {}",
                            session.context.session_id,
                            bound.display(),
                            current_directory.display()
                        ),
                    ));
                }
                None => {
                    patch.current_directory = Some(OptionalCurrentDirectoryPatch::Set {
                        path: current_directory,
                    });
                }
                Some(_) => {}
            }
        }
        if patch.conf.is_none() && patch.current_directory.is_none() {
            return Ok(session);
        }
        self.access
            .patch_session(actor, &session.context.session_id, patch)
            .await?;
        self.access
            .get_session(actor, &session.context.session_id)
            .await
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

    /// Starts one child Session owned by the caller's active parent turn.
    pub async fn spawn_subagent(
        &self,
        request: SpawnSubagentRequest,
    ) -> ServiceResult<SubagentSpawnResult> {
        self.subagents.spawn(request).await
    }

    /// Sends a message to one authorized running child Turn.
    pub async fn send_subagent_message(
        &self,
        request: SendSubagentMessageRequest,
    ) -> ServiceResult<SendMessageResult> {
        self.subagents.send_message(request).await
    }

    /// Waits until one selected child reaches a terminal state or the timeout expires.
    pub async fn wait_subagent(
        &self,
        request: WaitSubagentRequest,
    ) -> ServiceResult<SubagentSnapshot> {
        self.subagents.wait(request).await
    }

    /// Requests cancellation of one authorized descendant Session.
    pub async fn interrupt_subagent(
        &self,
        request: InterruptSubagentRequest,
    ) -> ServiceResult<SubagentSnapshot> {
        self.subagents.interrupt(request).await
    }

    /// Starts another turn in an idle authorized descendant Session.
    pub async fn follow_up_subagent(
        &self,
        request: FollowUpSubagentRequest,
    ) -> ServiceResult<SubagentSnapshot> {
        self.subagents.follow_up(request).await
    }

    /// Lists the complete Agent tree rooted at the caller's collaboration root.
    pub async fn list_subagents(
        &self,
        request: ListSubagentsRequest,
    ) -> ServiceResult<AgentTreeSnapshot> {
        self.subagents.list(request).await
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
        let _session = self.access.get_session(actor, session_id).await?;
        self.ensure_session_write_allowed(session_id).await?;
        let operation_id = uuid::Uuid::new_v4().simple().to_string();
        let identity = AgentSessionCompactionIdentity {
            request_id: format!("req_{operation_id}"),
            turn_id: format!("turn_{operation_id}"),
            message_id: format!("msg_{operation_id}"),
        };
        let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(16);
        let event_log = self.event_log.clone();
        let event_task = tokio::spawn(async move {
            let mut first_error = None;
            while let Some(event) = event_rx.recv().await {
                if let Err(error) = event_log.record_talos_event(event).await {
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
            }
            if let Some(error) = first_error {
                return Err(error);
            }
            Ok::<(), TalosError>(())
        });
        let result = self
            .access
            .compact_session(actor, session_id, identity, event_tx)
            .await;
        let event_result = event_task.await.map_err(|error| {
            TalosError::internal(format!("session compaction event task failed: {error}"))
        })?;
        event_result?;
        result
    }

    pub async fn clear_session(
        &self,
        actor: &Actor,
        session_id: &str,
    ) -> ServiceResult<TalosSession> {
        let _session = self.access.get_session(actor, session_id).await?;
        self.ensure_session_write_allowed(session_id).await?;
        self.access.clear_session(actor, session_id).await
    }

    pub async fn close_session(&self, actor: &Actor, session_id: &str) -> ServiceResult<()> {
        let _session = self.access.get_session(actor, session_id).await?;
        self.ensure_session_write_allowed(session_id).await?;
        self.access.close_session(actor, session_id).await
    }

    pub async fn delete_session(
        &self,
        actor: &Actor,
        session_id: &str,
    ) -> ServiceResult<SessionPatchAck> {
        let _session = self.access.get_session(actor, session_id).await?;
        self.ensure_session_write_allowed(session_id).await?;
        self.subagents.ensure_tree_inactive(session_id).await?;
        let ack = self.access.delete_session(actor, session_id).await?;
        if ack.success {
            self.message_queue.clear_session(session_id)?;
        }
        Ok(ack)
    }

    async fn ensure_session_write_allowed(&self, session_id: &str) -> ServiceResult<()> {
        if matches!(
            self.events
                .get_active_turn(session_id)
                .await?
                .map(|turn| turn.status),
            Some(
                ActiveTurnStatus::Running
                    | ActiveTurnStatus::WaitingUserAction
                    | ActiveTurnStatus::Cancelling
            )
        ) {
            return Err(TalosError::public(
                talos_error::ErrorCode::ActiveTurnRunning,
                format!("session `{session_id}` already has an active turn"),
            ));
        }
        Ok(())
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

    pub async fn submit_user_action(
        &self,
        actor: &Actor,
        request: talos_types::SubmitUserActionRequest,
    ) -> ServiceResult<talos_types::SubmitUserActionResponse> {
        let session = self.access.get_session(actor, &request.session_id).await?;
        let state = self
            .turns
            .submit_user_action(
                session,
                &request.turn_id,
                &request.action_id,
                request.response,
            )
            .await?;
        Ok(talos_types::SubmitUserActionResponse {
            action_id: request.action_id,
            state,
        })
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
        let session = self.access.get_session(actor, &request.session_id).await?;
        if !self.access.has_active_runtime_turn(&session).await? {
            if let Some(active) = self.events.get_active_turn(&request.session_id).await? {
                self.turns.recover_user_action(session, active).await?;
            }
        }
        let session_id = request.session_id.clone();
        let after_event_id = parse_timeline_event_id(request.since_event_id)?;
        let command = TimelineSubscribeCommand {
            session_id: request.session_id,
            turn_id: request.turn_id.clone(),
            after_event_id,
            replay_limit: request.replay_limit.unwrap_or(1).max(1),
            close_when_idle: request.close_when_idle.unwrap_or(false),
            close_on_terminal: request.close_on_terminal.unwrap_or(false),
        };
        let subscription = if request.turn_id.is_none() {
            self.events
                .subscribe_with_queue_changes(command, self.message_queue.subscribe_changed())
                .await?
        } else {
            self.events.subscribe(command).await?
        };
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
        let session = self.access.get_session(actor, &request.session_id).await?;
        if !self.access.has_active_runtime_turn(&session).await? {
            if let Some(active) = self.events.get_active_turn(&request.session_id).await? {
                self.turns.recover_user_action(session, active).await?;
            }
        }
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
}

fn apply_config_value_to_config(config: &mut Config, value: &ConfigValue) {
    match value {
        ConfigValue::Llm(llm) => {
            config.llm = (**llm).clone();
        }
        ConfigValue::WebSearch(web_search) => {
            config.agent.kernel.tools.web_search = (**web_search).clone();
        }
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
