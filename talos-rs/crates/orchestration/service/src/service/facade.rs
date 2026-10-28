use std::sync::Arc;

use crate::InnerModelClient;
use talos_agent_api::{SubmitTurnInputRequest, SubmitTurnInputResponse};
use talos_auth::{AuthService, UserService};
use talos_coordinator::AgentCoordinator;
use talos_error::TalosError;
use talos_store::profile::ProfileStore;
use talos_store::project::ProjectStore;
use talos_store::session::{AgentExecutionStore, SessionRecord, SessionStore};
use talos_types::{
    AgentTreeSnapshot, FollowUpSubagentRequest, InterruptSubagentRequest, ListSubagentsRequest,
    MessageQueue, MessageQueueAddRequest, MessageQueueDeleteRequest, MessageQueueListRequest,
    MessageQueueReorderRequest, MessageQueueStartRequest, MessageQueueUpdateRequest,
    OptionalCurrentDirectoryPatch, RecoverTimelineRequest,
    RetryTurnRequest as SharedRetryTurnRequest, SendMessageResult, SendSubagentMessageRequest,
    SessionConfigOverridePolicy, SessionInfo, SessionListRequest, SessionPatch,
    SpawnSubagentRequest, StartTurnRequest, StartTurnResponse, SubagentSnapshot,
    SubagentSpawnResult, SubscribeTimelineRequest, TalosConfig, WaitSubagentRequest,
};

use super::types::ServiceResult;
use crate::access::{CreateSessionRequest, ResourceAccessService};
use crate::file::FileService;
use crate::project::ProjectService;
use crate::session::SessionService;
use crate::subagent::SubagentService;
use crate::timeline::TimelineEventId;
use crate::turn::{StartTurnReservation, TurnRetryParams, TurnService};
use crate::{
    AccountService, Actor, ProfileService, SessionHistoryService, SessionMessageQueueService,
    TimelineEventService, TimelineSubscribeCommand, TimelineSubscription,
    TimelineSubscriptionService,
};

#[path = "sync_management.rs"]
mod sync_management;
pub use sync_management::ManagementSync;

#[path = "active_commands.rs"]
mod active_commands;
#[path = "config.rs"]
mod config;
#[path = "desktop.rs"]
mod desktop;
#[path = "environment.rs"]
mod environment;
#[path = "interrupt.rs"]
mod interrupt;
#[path = "mcp_oauth.rs"]
mod mcp_oauth;
#[path = "message_queue.rs"]
mod message_queue;
#[path = "plugins.rs"]
mod plugins;
#[path = "project_fs.rs"]
mod project_fs;
#[path = "session_deletion.rs"]
mod session_deletion;

#[path = "file_preview.rs"]
mod file_preview;

#[derive(Clone)]
pub struct TalosService {
    file_preview_base_url: Option<String>,
    file_previews:
        Arc<std::sync::Mutex<std::collections::HashMap<String, Arc<file_preview::PreviewGrant>>>>,
    desktop_ingress: Arc<std::sync::RwLock<Option<agent_client::AgentNodeClient>>>,
    desktop_routes:
        Arc<std::sync::Mutex<std::collections::HashMap<[u8; 32], desktop::ViewerRoute>>>,
    sync_management: Arc<std::sync::RwLock<Option<std::sync::Weak<dyn ManagementSync>>>>,
    oauth_relay_origin: String,
    oauth_routes:
        Arc<std::sync::Mutex<std::collections::HashMap<[u8; 32], Arc<mcp_oauth::McpRelayRoute>>>>,
    oauth_capacity: Arc<tokio::sync::Semaphore>,
    access: ResourceAccessService,
    sessions: Arc<SessionService>,
    pub account: AccountService,
    pub nodes: crate::AgentNodeService,
    pub profile: ProfileService,
    turns: TurnService,
    pub events: TimelineSubscriptionService,
    history: SessionHistoryService,
    message_queue: SessionMessageQueueService,
    subagents: SubagentService,
    pub files: Arc<FileService>,
    execution_gate: Arc<crate::turn::NodeAdmissionGates>,
    active_commands: Arc<std::sync::RwLock<Option<Arc<dyn agent_protocol::ActiveSessionCommands>>>>,
}

impl TalosService {
    /// Host composition after authenticating an account. Sync owns transport;
    /// original Services retain admission, persistence and notification policy.
    pub fn install_sync_handle(&self, account_id: String, handle: Arc<talos_sync::SyncHandle>) {
        self.sessions.install_sync_handle(account_id, handle);
    }

    pub fn remove_sync_handle(&self, account_id: &str) {
        self.sessions.remove_sync_handle(account_id);
    }

    pub async fn retry_shared_session_heads(&self, account_id: &str) -> ServiceResult<()> {
        self.sessions.retry_shared_history_cursors(account_id).await
    }

    pub fn set_coordinator_id(&self, peer_id: String) {
        self.sessions.set_coordinator_id(peer_id);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        coordinator: Arc<dyn AgentCoordinator>,
        nodes: crate::AgentNodeService,
        session_store: Arc<dyn SessionStore>,
        agent_execution_store: Arc<dyn AgentExecutionStore>,
        project_store: Arc<dyn ProjectStore>,
        profile_store: Arc<dyn ProfileStore>,
        files: Arc<FileService>,
        event_service: Arc<TimelineEventService>,
        base_config: TalosConfig,
        auth_service: Arc<dyn AuthService>,
        user_service: Arc<dyn UserService>,
    ) -> Self {
        Self::assemble(
            coordinator,
            nodes,
            session_store,
            agent_execution_store,
            project_store,
            profile_store,
            files,
            event_service,
            base_config,
            auth_service,
            user_service,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn assemble(
        coordinator: Arc<dyn AgentCoordinator>,
        nodes: crate::AgentNodeService,
        session_store: Arc<dyn SessionStore>,
        agent_execution_store: Arc<dyn AgentExecutionStore>,
        project_store: Arc<dyn ProjectStore>,
        profile_store: Arc<dyn ProfileStore>,
        files: Arc<FileService>,
        event_service: Arc<TimelineEventService>,
        base_config: TalosConfig,
        auth_service: Arc<dyn AuthService>,
        user_service: Arc<dyn UserService>,
    ) -> Self {
        let sessions = SessionService::new(
            session_store.clone(),
            project_store.clone(),
            base_config.clone(),
            coordinator.clone(),
        );
        let sessions = Arc::new(sessions);
        let execution_gate = Arc::new(crate::turn::NodeAdmissionGates::default());
        let projects = Arc::new(ProjectService::new(project_store.clone()));
        let message_queue = SessionMessageQueueService::new(
            base_config.agent.message_queue.max_messages_per_session,
        )
        .expect("message queue config must be validated before service assembly");
        let turns = TurnService::new(
            crate::turn::AgentExecutionDriver::new(
                coordinator.clone(),
                agent_execution_store.clone(),
                event_service.clone(),
                sessions.clone(),
            ),
            profile_store.clone(),
            message_queue.clone(),
            base_config.clone(),
            execution_gate.clone(),
        );
        let history = SessionHistoryService::new(event_service.clone());
        let subagents = SubagentService::new(
            sessions.clone(),
            Arc::new(turns.clone()),
            event_service.clone(),
        );

        Self {
            file_preview_base_url: base_config.storage.files.preview_base_url.clone(),
            file_previews: Default::default(),
            sync_management: Default::default(),
            active_commands: Default::default(),
            oauth_relay_origin: base_config.cloud.agent_base_url.clone(),
            oauth_routes: Arc::default(),
            oauth_capacity: Arc::new(tokio::sync::Semaphore::new(32)),
            desktop_ingress: Default::default(),
            desktop_routes: Default::default(),
            nodes,
            access: ResourceAccessService::with_services(
                coordinator.clone(),
                sessions.clone(),
                projects,
            ),
            sessions,
            account: AccountService::new(
                auth_service,
                user_service,
                base_config.cloud.api_base_url.clone(),
            ),
            turns,
            profile: ProfileService::new(
                session_store.clone(),
                project_store,
                profile_store,
                base_config.agent.kernel.timezone.clone(),
            ),
            events: TimelineSubscriptionService::new(event_service.clone()),
            history,
            message_queue,
            subagents,
            files,
            execution_gate,
        }
    }

    /// Installs the control-plane model used by the existing reflection policy.
    #[must_use]
    pub fn with_user_workspace_memory_reflection(
        mut self,
        inner_model: Arc<InnerModelClient>,
    ) -> Self {
        self.turns = self
            .turns
            .with_user_workspace_memory_reflection(inner_model);
        self
    }

    pub async fn patch_session(
        &self,
        actor: &Actor,
        session_id: &str,
        patch: SessionPatch,
    ) -> ServiceResult<SessionInfo> {
        let session = self.access.load_session(actor, session_id).await?;
        let _execution_guard = self
            .execution_gate
            .for_node(&session.execution_node_id)
            .write_owned()
            .await;
        let updated = self.access.patch_session(session, patch).await?;
        self.session_for_client(actor, self.sessions.session_info(&updated).await?)
            .await
    }

    pub async fn list_sessions(
        &self,
        actor: &Actor,
        mut request: SessionListRequest,
    ) -> ServiceResult<Vec<SessionInfo>> {
        if let Some(node) = request.node_id.take() {
            request.node_id = Some(self.resolve_computer_metadata_node(actor, &node).await?);
        }
        let mut result = Vec::new();
        for info in self.access.list_sessions(actor, request).await? {
            result.push(self.session_for_client(actor, info).await?);
        }
        Ok(result)
    }

    /// Reads the authenticated user's Profile usage view.
    pub async fn read_profile(
        &self,
        actor: &Actor,
    ) -> ServiceResult<talos_types::ProfileReadResponse> {
        self.profile.read_profile(actor).await
    }

    pub async fn get_session(&self, actor: &Actor, session_id: &str) -> ServiceResult<SessionInfo> {
        let record = self.access.load_session(actor, session_id).await?;
        self.session_for_client(actor, self.sessions.session_info(&record).await?)
            .await
    }

    pub async fn get_session_history(
        &self,
        actor: &Actor,
        request: talos_types::SessionHistoryRequest,
    ) -> ServiceResult<talos_agent_api::SessionHistoryResponse> {
        let _session = self.access.load_session(actor, &request.session_id).await?;
        self.history
            .read(
                &request.session_id,
                request.cursor.as_deref(),
                request.limit,
            )
            .await
    }

    pub async fn get_active_turn(
        &self,
        actor: &Actor,
        session_id: &str,
    ) -> ServiceResult<talos_agent_api::SessionActiveTurnResponse> {
        let _session = self.access.load_session(actor, session_id).await?;
        let active_turn = self.events.display_active_turn(session_id).await?;
        let latest_event_id = self
            .events
            .latest_event_id(session_id)
            .await?
            .map(|event_id| event_id.to_string());
        Ok(talos_agent_api::SessionActiveTurnResponse {
            active_turn,
            latest_event_id,
        })
    }

    async fn submit_start_turn(
        &self,
        actor: &Actor,
        source: talos_types::talos::SessionSource,
        mut request: StartTurnRequest,
        idempotency_key: Option<String>,
    ) -> ServiceResult<StartTurnResponse> {
        TurnService::validate_start_turn_request(&request)?;
        // Idempotency describes the public command, before host-local routing aliases.
        let command_request = request.clone();
        if let Some(node) = request.node_id.take() {
            request.node_id = Some(self.connect_selected_computer(actor, &node).await?);
        }
        let session_id = request.session_id.clone();
        let existing_session = match self.access.load_session(actor, &session_id).await {
            Ok(session) => Some(session),
            Err(error) if error.is_code(talos_error::ErrorCode::ResourceNotFound) => None,
            Err(error) => return Err(error),
        };
        let computer = existing_session
            .as_ref()
            .map(|session| session.execution_node_id.as_str())
            .or(request.node_id.as_deref())
            .unwrap_or("embedded");
        self.connect_selected_computer(actor, computer).await?;
        let prepared = match self
            .turns
            .reserve_start_turn(&command_request, idempotency_key.as_deref())
            .await?
        {
            StartTurnReservation::Ready(prepared) => prepared,
            StartTurnReservation::Completed(response) => return Ok(response),
        };
        let created_session = existing_session.is_none();
        let session_result = match existing_session {
            Some(session) => self.apply_existing_turn_metadata(session, &request).await,
            None => {
                self.access
                    .create_session(
                        actor,
                        CreateSessionRequest {
                            node_id: request.node_id.clone(),
                            session_id: session_id.clone(),
                            source,
                            model_selection: request.model_selection.clone(),
                            user_permission_mode: request.user_permission_mode,
                            project_id: request.project_id.clone(),
                            current_directory: request.current_directory.clone(),
                        },
                    )
                    .await
            }
        };
        let session = match session_result {
            Ok(session) => session,
            Err(error) => {
                self.turns.release_start_turn(prepared).await;
                return Err(error);
            }
        };
        let rollback_session = created_session.then(|| session.clone());
        let result = self
            .turns
            .submit_start_turn(session, request, prepared)
            .await;
        if let (Some(session), Err(_)) = (rollback_session, &result) {
            if let Err(error) = self.sessions.delete_session(session).await {
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
        session: SessionRecord,
        request: &StartTurnRequest,
    ) -> ServiceResult<SessionRecord> {
        if request
            .node_id
            .as_ref()
            .is_some_and(|id| id != &session.execution_node_id)
        {
            return Err(talos_error::TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "Session Worker binding is immutable",
            ));
        }
        if request
            .project_id
            .as_deref()
            .is_some_and(|project_id| session.project_id.as_deref() != Some(project_id))
        {
            return Err(talos_error::TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                format!("session '{}' project binding is immutable", session.id),
            ));
        }
        let mut patch = SessionPatch::default();
        if let Some(selection) = request.model_selection.as_ref() {
            patch.conf = Some(SessionConfigOverridePolicy::single_model_selection_conf(
                selection,
            ));
        }
        if let Some(current_directory) = request.current_directory.clone() {
            match session.current_directory.as_ref() {
                Some(bound) if bound != &current_directory => {
                    return Err(talos_error::TalosError::new(
                        talos_error::ErrorCode::InvalidRequest,
                        format!(
                            "session {} is bound to current_directory {}, but request used {}",
                            session.id,
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
        self.access.patch_session(session, patch).await
    }

    pub async fn start_turn(
        &self,
        actor: &Actor,
        source: talos_types::talos::SessionSource,
        request: StartTurnRequest,
        idempotency_key: Option<String>,
    ) -> ServiceResult<StartTurnResponse> {
        self.submit_start_turn(actor, source, request, idempotency_key)
            .await
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

    async fn submit_retry(
        &self,
        actor: &Actor,
        session_id: &str,
        request: TurnRetryParams,
    ) -> ServiceResult<StartTurnResponse> {
        let session = self.access.load_session(actor, session_id).await?;
        self.turns.submit_retry(session, request).await
    }

    pub async fn retry_turn(
        &self,
        actor: &Actor,
        request: SharedRetryTurnRequest,
    ) -> ServiceResult<StartTurnResponse> {
        let session_id = request.session_id.clone();
        self.submit_retry(
            actor,
            &session_id,
            TurnRetryParams {
                input: request.input,
                client_message_id: request.client_message_id,
                retry_of_message_id: request.retry_of_message_id,
            },
        )
        .await
    }

    pub async fn compact_session(
        &self,
        actor: &Actor,
        session_id: &str,
    ) -> ServiceResult<SessionInfo> {
        let session = self.access.load_session(actor, session_id).await?;
        self.ensure_session_write_allowed(session_id).await?;
        let operation_id = uuid::Uuid::new_v4().simple().to_string();
        self.turns
            .compact_session(
                session,
                format!("req_{operation_id}"),
                talos_agent_api::AgentTurnRef::new(session_id, format!("turn_{operation_id}")),
            )
            .await
    }

    pub async fn clear_session(
        &self,
        actor: &Actor,
        session_id: &str,
    ) -> ServiceResult<SessionInfo> {
        let session = self.access.load_session(actor, session_id).await?;
        let _execution_guard = self
            .execution_gate
            .for_node(&session.execution_node_id)
            .write_owned()
            .await;
        self.ensure_session_write_allowed(session_id).await?;
        self.access.clear_session(session).await
    }

    pub async fn close_session(&self, actor: &Actor, session_id: &str) -> ServiceResult<()> {
        let session = self.access.load_session(actor, session_id).await?;
        let _execution_guard = self
            .execution_gate
            .for_node(&session.execution_node_id)
            .write_owned()
            .await;
        self.ensure_session_write_allowed(session_id).await?;
        self.access.close_session(session).await
    }

    async fn ensure_session_write_allowed(&self, session_id: &str) -> ServiceResult<()> {
        if self
            .turns
            .canonical_active_turn(session_id)
            .await?
            .is_some()
        {
            return Err(TalosError::new(
                talos_error::ErrorCode::ActiveTurnRunning,
                format!("session `{session_id}` already has an active turn"),
            ));
        }
        Ok(())
    }

    /// Run one process-local maintenance operation only while the Session has no Turn owner.
    ///
    /// The reservation blocks new Turn admission for this Session for the operation duration.
    /// It does not persist maintenance state or expose the execution gate to callers.
    pub async fn run_idle_session_maintenance<T, F, Fut>(
        &self,
        session_id: &str,
        operation: F,
    ) -> ServiceResult<Option<T>>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = ServiceResult<T>>,
    {
        self.turns
            .run_when_session_idle(session_id, operation)
            .await
    }

    pub async fn submit_turn_input(
        &self,
        actor: &Actor,
        request: SubmitTurnInputRequest,
    ) -> ServiceResult<SubmitTurnInputResponse> {
        let identity = request.identity.clone();
        let session = self
            .access
            .load_session(actor, &identity.session_id)
            .await?;
        if let Some(reply) = self
            .forward_active_command(
                actor,
                &session,
                agent_protocol::ActiveSessionCommand::Input(request.clone()),
            )
            .await?
        {
            let agent_protocol::ActiveSessionReply::Input(reply) = reply else {
                return Err(TalosError::new(
                    talos_error::ErrorCode::InvalidRequest,
                    "Current coordinator returned an invalid command result.",
                ));
            };
            return Ok(reply);
        }
        self.turns.submit_turn_input(session, request).await
    }

    pub async fn subscribe_timeline(
        &self,
        actor: &Actor,
        request: SubscribeTimelineRequest,
    ) -> ServiceResult<talos_types::SubscribeTimelineResponse> {
        let subscription = self.open_timeline_subscription(actor, request).await?;
        Ok(subscription.response())
    }

    pub async fn open_timeline_subscription(
        &self,
        actor: &Actor,
        request: SubscribeTimelineRequest,
    ) -> ServiceResult<TimelineSubscription> {
        let session = self.access.load_session(actor, &request.session_id).await?;
        if let Some(active) = self
            .turns
            .canonical_active_turn(&request.session_id)
            .await?
        {
            if !self.turns.is_turn_execution_active(&active.identity) {
                self.turns.recover_active_turn(session, active).await?;
            }
        }
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
        Ok(subscription)
    }

    pub async fn recover_timeline(
        &self,
        actor: &Actor,
        request: RecoverTimelineRequest,
    ) -> ServiceResult<talos_agent_api::RecoverTimelineResponse> {
        let session = self.access.load_session(actor, &request.session_id).await?;
        if let Some(active) = self
            .turns
            .canonical_active_turn(&request.session_id)
            .await?
        {
            if !self.turns.is_turn_execution_active(&active.identity) {
                self.turns.recover_active_turn(session, active).await?;
            }
        }
        let after_event_id = parse_timeline_event_id(request.after_event_id)?;
        self.events
            .recover_timeline(
                &request.session_id,
                after_event_id.as_ref(),
                request.replay_limit.unwrap_or(1).max(1),
            )
            .await
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
