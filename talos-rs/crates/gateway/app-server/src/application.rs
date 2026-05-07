use async_trait::async_trait;
use serde::Serialize;
use serde_json::{json, Value};
use talos_app_protocol::errors::AppServerErrorBody;
use talos_app_protocol::requests::{
    AdminProjectListResponse, AdminSessionListParams, AdminSessionListResponse,
    AdminSessionPagination, AdminUserRole, ClientRequest, ClientRequestParams,
    ConfigBatchWriteParams, ConfigReadParams, ConfigValueWriteParams, EventSubscribeParams,
    MemoryFileReadParams, MemoryFileWriteParams, MessageCommandStatus, OptionalPathPatch,
    OptionalStringPatch, ProjectCreateParams, ProjectPatchParams, ProtocolInputItem,
    ProtocolTalosContext, SessionCreateParams, SessionListParams, SessionPatchParams,
    TurnStartParams, TurnStartResponse,
};
use talos_application_service::{
    Actor, AdminSessionPage, ApplicationEventStream, EventReplay, MemoryDate, MemoryListParams,
    MemorySearchParams, TalosApplicationService, TurnRetryRequest, WorkspaceDocService,
};
use talos_auth::{
    AdminCreateUserRequest, AdminPatchUserRequest, AuthScope, RequestIdentity, UserRole,
};
use talos_error::TalosError;
use talos_session_store::{ActiveTurnRecord, IdempotencyStatus, SessionEventId};
use talos_tools::ToolRegistry;
use talos_types::talos::{
    TalosContext, TalosInputItem, TalosMessageRequest, TalosProvider, TalosSourceType,
};
use talos_types::{
    DocUpdateRequest, ExpectedFileVersion, OptionalCurrentDirectoryPatch, OptionalProjectIdPatch,
    ProjectPatch, SessionConf, SessionConfigOverridePolicy, SessionPatch, SessionQuery,
};

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use talos_error::TalosResult;

/// Abstraction for draining background tasks (e.g. memory flush) during shutdown.
#[async_trait::async_trait]
pub trait BackgroundTaskDrain: Send + Sync {
    async fn drain(&self, timeout: Duration) -> TalosResult<()>;
}

/// Blanket impl for closure-based drain providers.
#[async_trait::async_trait]
impl<F, Fut> BackgroundTaskDrain for F
where
    F: Fn(Duration) -> Fut + Send + Sync,
    Fut: std::future::Future<Output = TalosResult<()>> + Send,
{
    async fn drain(&self, timeout: Duration) -> TalosResult<()> {
        self(timeout).await
    }
}

pub type ProtocolResult<T> = Result<T, AppServerErrorBody>;

#[async_trait]
pub trait AppServerApplication: Send + Sync {
    async fn dispatch(&self, actor: &Actor, request: ClientRequest) -> ProtocolResult<Value>;

    async fn subscribe_session_events(
        &self,
        actor: &Actor,
        params: EventSubscribeParams,
    ) -> ProtocolResult<SubscriptionSource>;

    async fn cancel_active_turn(&self, actor: &Actor, session_id: &str) -> ProtocolResult<()>;

    async fn drain_background_tasks(&self, timeout: Duration) -> TalosResult<()>;
}

pub struct SubscriptionSource {
    pub session_id: String,
    pub high_water_mark_event_id: Option<SessionEventId>,
    pub replay: EventReplay,
    pub live_events: ApplicationEventStream,
    pub has_live_turn: bool,
}

#[derive(Clone)]
pub struct TalosApplicationAdapter {
    app: std::sync::Arc<TalosApplicationService>,
    replay_limit: usize,
    drain_provider: Arc<Option<Box<dyn BackgroundTaskDrain>>>,
    tool_registry: Option<Arc<ToolRegistry>>,
}

impl TalosApplicationAdapter {
    pub fn new(app: std::sync::Arc<TalosApplicationService>, replay_limit: usize) -> Self {
        Self {
            app,
            replay_limit: replay_limit.max(1),
            drain_provider: Arc::new(None),
            tool_registry: None,
        }
    }

    pub fn with_drain_provider(mut self, drain: Box<dyn BackgroundTaskDrain>) -> Self {
        self.drain_provider = Arc::new(Some(drain));
        self
    }

    pub fn with_tool_registry(mut self, registry: Arc<ToolRegistry>) -> Self {
        self.tool_registry = Some(registry);
        self
    }
}

#[async_trait]
impl AppServerApplication for TalosApplicationAdapter {
    async fn dispatch(&self, actor: &Actor, request: ClientRequest) -> ProtocolResult<Value> {
        match request.params {
            ClientRequestParams::Initialize(_) => Err(AppServerErrorBody::invalid_request(
                "initialize is handled by the app-server runtime",
            )),
            ClientRequestParams::SessionCreate(params) => {
                to_value(self.create_session(actor, params).await?)
            }
            ClientRequestParams::SessionList(params) => {
                to_value(json!({ "data": self.list_sessions(actor, params).await? }))
            }
            ClientRequestParams::SessionRead(params) => to_value(
                self.app
                    .get_session(actor, &params.session_id)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?,
            ),
            ClientRequestParams::SessionPatch(params) => {
                self.patch_session(actor, params).await?;
                to_value(json!({ "success": true }))
            }
            ClientRequestParams::SessionDelete(params) => {
                let ack = self
                    .app
                    .delete_session(actor, &params.session_id)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?;
                to_value(json!({ "success": ack.success }))
            }
            ClientRequestParams::SessionClose(params) => {
                self.app
                    .close_session(actor, &params.session_id)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?;
                to_value(json!({ "success": true }))
            }
            ClientRequestParams::SessionCompact(params) => to_value(
                self.app
                    .compact_session(actor, &params.session_id)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?,
            ),
            ClientRequestParams::SessionClear(params) => to_value(
                self.app
                    .clear_session(actor, &params.session_id)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?,
            ),
            ClientRequestParams::SessionActiveTurn(params) => {
                self.app
                    .get_session(actor, &params.session_id)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?;
                let active_turn = self
                    .app
                    .events
                    .get_active_turn(&params.session_id)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?;
                to_value(render_active_turn(&params.session_id, active_turn))
            }
            ClientRequestParams::SessionTurns(params) => {
                let turns = self
                    .app
                    .get_session_turns(actor, &params.session_id)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?;
                to_value(json!({ "data": turns }))
            }
            ClientRequestParams::ProjectCreate(params) => {
                to_value(self.create_project(actor, params).await?)
            }
            ClientRequestParams::ProjectList(_) => {
                let projects = self
                    .app
                    .list_projects(actor)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?;
                to_value(json!({ "data": projects }))
            }
            ClientRequestParams::ProjectRead(params) => to_value(
                self.app
                    .get_project(actor, &params.project_id)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?,
            ),
            ClientRequestParams::ProjectPatch(params) => {
                to_value(self.patch_project(actor, params).await?)
            }
            ClientRequestParams::ProjectDelete(params) => {
                self.app
                    .delete_project(actor, &params.project_id)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?;
                to_value(json!({ "success": true }))
            }
            ClientRequestParams::TurnStart(params) => {
                to_value(self.start_turn(actor, params).await?)
            }
            ClientRequestParams::TurnInterrupt(params) => {
                self.cancel_active_turn(actor, &params.session_id).await?;
                to_value(json!({ "success": true }))
            }
            ClientRequestParams::TurnRetry(params) => {
                let ack = self
                    .app
                    .submit_retry(
                        actor,
                        &params.session_id,
                        TurnRetryRequest {
                            input: params.input.into_iter().map(map_input_item).collect(),
                            workspace_id: params.workspace_id,
                            client_message_id: params.client_message_id,
                            retry_of_message_id: params.retry_of_message_id,
                        },
                    )
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?;
                to_value(ack)
            }
            ClientRequestParams::EventSubscribe(_) => Err(AppServerErrorBody::invalid_request(
                "event.subscribe is handled by the app-server runtime",
            )),
            ClientRequestParams::EventUnsubscribe(_) => Err(AppServerErrorBody::invalid_request(
                "event.unsubscribe is handled by the app-server runtime",
            )),
            ClientRequestParams::ToolApprovalSubmit(params) => {
                self.app
                    .submit_tool_approval(
                        actor,
                        &params.session_id,
                        &params.call_id,
                        &params.tool_name,
                        params.approved,
                        params.reason,
                    )
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?;
                to_value(json!({ "success": true }))
            }
            ClientRequestParams::ToolDynamicResultSubmit(params) => {
                self.app
                    .submit_dynamic_tool_result(
                        actor,
                        &params.session_id,
                        &params.call_id,
                        &params.tool_name,
                        params.output,
                        params.success,
                    )
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?;
                to_value(json!({ "success": true }))
            }
            ClientRequestParams::AdminSessionList(params) => {
                to_value(self.list_admin_sessions(actor, params).await?)
            }
            ClientRequestParams::AdminProjectList(_) => {
                let projects = self
                    .app
                    .list_admin_projects(actor)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?;
                to_value(AdminProjectListResponse { data: projects })
            }
            ClientRequestParams::AdminUserList(_) => {
                let users = self
                    .app
                    .account
                    .list_users(&request_identity(actor))
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?;
                to_value(json!({ "data": users }))
            }
            ClientRequestParams::AdminUserCreate(params) => {
                let user = self
                    .app
                    .account
                    .create_user(
                        &request_identity(actor),
                        AdminCreateUserRequest {
                            username: params.username,
                            password: params.password,
                            email: params.email,
                            role: map_user_role(params.role),
                            disabled: params.disabled,
                        },
                    )
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?;
                to_value(user)
            }
            ClientRequestParams::AdminUserPatch(params) => {
                let user = self
                    .app
                    .account
                    .patch_user(
                        &request_identity(actor),
                        &params.user_id,
                        AdminPatchUserRequest {
                            username: params.username,
                            email: params.email,
                            password: params.password,
                            role: params.role.map(map_user_role),
                            disabled: params.disabled,
                        },
                    )
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?;
                to_value(user)
            }
            ClientRequestParams::AdminUserDelete(params) => {
                self.app
                    .account
                    .delete_user(&request_identity(actor), &params.user_id)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?;
                to_value(json!({ "success": true }))
            }
            ClientRequestParams::ModelList(_) => {
                let models = self.app.config.list_models().await;
                to_value(json!({ "data": models }))
            }
            ClientRequestParams::ConfigRead(ConfigReadParams {}) => {
                let llm = self
                    .app
                    .config
                    .get_llm()
                    .map_err(AppServerErrorBody::from_talos_error)?;
                to_value(llm)
            }
            ClientRequestParams::MemoryFileList(params) => {
                let files = self
                    .app
                    .memory
                    .list_files(MemoryListParams {
                        limit: params.limit,
                        before_date: None,
                        date_from: None,
                        date_to: None,
                    })
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?;
                to_value(json!({ "data": files }))
            }
            ClientRequestParams::MemoryFileRead(params) => {
                to_value(self.read_memory_file(params).await?)
            }
            ClientRequestParams::MemoryFileWrite(params) => {
                to_value(self.write_memory_file(actor, params).await?)
            }
            ClientRequestParams::WorkspaceDocList(_) => {
                let docs = self
                    .app
                    .workspace_docs
                    .list_docs()
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?;
                to_value(json!({ "data": docs }))
            }
            ClientRequestParams::WorkspaceDocRead(params) => {
                let kind = WorkspaceDocService::parse_kind(&params.kind)
                    .map_err(AppServerErrorBody::from_talos_error)?;
                to_value(
                    self.app
                        .workspace_docs
                        .get_doc(kind)
                        .await
                        .map_err(AppServerErrorBody::from_talos_error)?,
                )
            }
            ClientRequestParams::WorkspaceDocWrite(params) => {
                let kind = WorkspaceDocService::parse_kind(&params.kind)
                    .map_err(AppServerErrorBody::from_talos_error)?;
                to_value(
                    self.app
                        .workspace_docs
                        .put_doc(
                            actor,
                            kind,
                            DocUpdateRequest {
                                content: params.content,
                                base_version: expected_version(params.expected_version),
                            },
                        )
                        .await
                        .map_err(AppServerErrorBody::from_talos_error)?,
                )
            }
            ClientRequestParams::ConfigValueWrite(params) => {
                to_value(self.write_config_value(actor, params).await?)
            }
            ClientRequestParams::ConfigBatchWrite(params) => {
                to_value(self.write_config_batch(actor, params).await?)
            }
            ClientRequestParams::ConfigReload(_) => to_value(json!({ "success": true })),
            ClientRequestParams::SkillList(_) => to_value(json!({ "data": self.list_skills() })),
            ClientRequestParams::AppList(_) => to_value(json!({ "data": self.list_apps() })),
            ClientRequestParams::ToolList(_) => to_value(json!({ "data": self.list_tools() })),
            ClientRequestParams::AccountGet(_) => to_value(json!({
                "user_id": actor.user_id,
                "username": actor.user_id,
                "email": null,
                "role": if actor.has_full_access() { "admin" } else { "user" },
                "disabled": false,
                "created_at": 0,
                "updated_at": 0
            })),
            ClientRequestParams::AuthRegister(params) => {
                let response = self
                    .app
                    .account
                    .register(params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?;
                to_value(response)
            }
            ClientRequestParams::AuthLogin(params) => {
                let response = self
                    .app
                    .account
                    .login(params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?;
                to_value(response)
            }
            ClientRequestParams::AuthLogout(_) => {
                self.app
                    .account
                    .logout(&request_identity(actor))
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?;
                to_value(json!({ "success": true }))
            }
            ClientRequestParams::UserCurrent(_) => {
                let user = self
                    .app
                    .account
                    .current_user(&request_identity(actor))
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?;
                to_value(user)
            }
            ClientRequestParams::UserUpdate(params) => {
                let user = self
                    .app
                    .account
                    .patch_current_user(&request_identity(actor), params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?;
                to_value(user)
            }
            ClientRequestParams::UserTokenList(_) => {
                let tokens = self
                    .app
                    .account
                    .list_tokens(&request_identity(actor))
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?;
                to_value(json!({ "data": tokens }))
            }
            ClientRequestParams::UserTokenCreate(params) => {
                let token = self
                    .app
                    .account
                    .create_token(&request_identity(actor), &params.name)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?;
                to_value(token)
            }
            ClientRequestParams::UserTokenDelete(params) => {
                self.app
                    .account
                    .revoke_token(&request_identity(actor), &params.token_id)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?;
                to_value(json!({ "success": true }))
            }
            ClientRequestParams::MemorySearch(_) => self.dispatch_secondary(actor, request).await,
        }
    }

    async fn subscribe_session_events(
        &self,
        actor: &Actor,
        params: EventSubscribeParams,
    ) -> ProtocolResult<SubscriptionSource> {
        self.app
            .get_session(actor, &params.session_id)
            .await
            .map_err(AppServerErrorBody::from_talos_error)?;
        let after_event_id = parse_event_id(params.since_event_id)?;
        let limit = params.replay_limit.unwrap_or(self.replay_limit).max(1);
        let live_events = self
            .app
            .events
            .subscribe_live_events()
            .map_err(AppServerErrorBody::from_talos_error)?;
        let high_water_mark_event_id = self
            .app
            .events
            .latest_event_id(&params.session_id)
            .await
            .map_err(AppServerErrorBody::from_talos_error)?;
        let replay = self
            .app
            .events
            .replay_after(&params.session_id, after_event_id.as_ref(), limit)
            .await
            .map_err(AppServerErrorBody::from_talos_error)?;
        let has_live_turn = self
            .app
            .events
            .has_live_turn(&params.session_id)
            .await
            .map_err(AppServerErrorBody::from_talos_error)?;
        Ok(SubscriptionSource {
            session_id: params.session_id,
            high_water_mark_event_id,
            replay,
            live_events,
            has_live_turn,
        })
    }

    async fn cancel_active_turn(&self, actor: &Actor, session_id: &str) -> ProtocolResult<()> {
        self.app
            .cancel_active_turn(actor, session_id)
            .await
            .map_err(AppServerErrorBody::from_talos_error)
    }

    async fn drain_background_tasks(&self, timeout: Duration) -> TalosResult<()> {
        if let Some(drain) = self.drain_provider.as_deref() {
            drain.drain(timeout).await
        } else {
            Ok(())
        }
    }
}

impl TalosApplicationAdapter {
    async fn create_session(
        &self,
        actor: &Actor,
        params: SessionCreateParams,
    ) -> ProtocolResult<talos_types::TalosSession> {
        self.app
            .create_session(
                actor,
                map_context(params.context)?,
                params.model,
                None,
                params.project_id,
                params.current_directory,
            )
            .await
            .map_err(AppServerErrorBody::from_talos_error)
    }

    async fn list_sessions(
        &self,
        actor: &Actor,
        params: SessionListParams,
    ) -> ProtocolResult<Vec<talos_types::TalosSession>> {
        self.app
            .list_sessions(
                actor,
                SessionQuery {
                    user_id: None,
                    channel_id: None,
                    provider: None,
                    project_id: params.project_id,
                    limit: params.limit,
                    offset: params.offset,
                },
            )
            .await
            .map_err(AppServerErrorBody::from_talos_error)
    }

    async fn patch_session(&self, actor: &Actor, params: SessionPatchParams) -> ProtocolResult<()> {
        let mut conf = SessionConf::default();
        if let Some(model) = params.model {
            conf = SessionConfigOverridePolicy::single_model_conf(&model);
        }
        let patch = SessionPatch {
            conf: (conf != SessionConf::default()).then_some(conf),
            project_id: params.project_id.map(map_project_patch),
            current_directory: params.current_directory.map(map_directory_patch),
        };
        self.app
            .patch_session(actor, &params.session_id, patch)
            .await
            .map_err(AppServerErrorBody::from_talos_error)?;
        Ok(())
    }

    async fn create_project(
        &self,
        actor: &Actor,
        params: ProjectCreateParams,
    ) -> ProtocolResult<talos_types::ProjectRecord> {
        self.app
            .create_project(actor, params.name, params.directory)
            .await
            .map_err(AppServerErrorBody::from_talos_error)
    }

    async fn patch_project(
        &self,
        actor: &Actor,
        params: ProjectPatchParams,
    ) -> ProtocolResult<talos_types::ProjectRecord> {
        self.app
            .patch_project(
                actor,
                &params.project_id,
                ProjectPatch { name: params.name },
            )
            .await
            .map_err(AppServerErrorBody::from_talos_error)
    }

    async fn list_admin_sessions(
        &self,
        actor: &Actor,
        params: AdminSessionListParams,
    ) -> ProtocolResult<AdminSessionListResponse> {
        let page = AdminSessionPage::from_query(SessionQuery {
            user_id: params.user_id,
            channel_id: params.channel_id,
            provider: params.provider,
            project_id: params.project_id,
            limit: params.limit,
            offset: params.offset,
        });
        let sessions = self
            .app
            .list_admin_sessions(actor, page.query)
            .await
            .map_err(AppServerErrorBody::from_talos_error)?;
        let total = self
            .app
            .count_admin_sessions(actor, page.count_query)
            .await
            .map_err(AppServerErrorBody::from_talos_error)?;
        Ok(AdminSessionListResponse {
            data: sessions,
            pagination: AdminSessionPagination {
                total,
                limit: page.limit,
                offset: page.offset,
            },
        })
    }

    async fn start_turn(
        &self,
        actor: &Actor,
        params: TurnStartParams,
    ) -> ProtocolResult<TurnStartResponse> {
        let ack = self
            .app
            .submit_message(
                actor,
                &params.session_id,
                TalosMessageRequest {
                    input: params.input.into_iter().map(map_input_item).collect(),
                    workspace_id: params.workspace_id,
                },
                params.client_request_id,
            )
            .await
            .map_err(AppServerErrorBody::from_talos_error)?;
        Ok(TurnStartResponse {
            request_id: ack.request_id,
            message_id: ack.message_id,
            session_id: ack.session_id,
            turn_id: ack.turn_id,
            status: map_message_command_status(ack.status),
        })
    }

    async fn read_memory_file(
        &self,
        params: MemoryFileReadParams,
    ) -> ProtocolResult<talos_types::MemoryFileDetail> {
        let date = memory_date_from_path(&params.path)?;
        self.app
            .memory
            .get_file(date)
            .await
            .map_err(AppServerErrorBody::from_talos_error)
    }

    async fn write_memory_file(
        &self,
        actor: &Actor,
        params: MemoryFileWriteParams,
    ) -> ProtocolResult<talos_types::MemoryFileDetail> {
        let date = memory_date_from_path(&params.path)?;
        self.app
            .memory
            .put_file(
                actor,
                date,
                DocUpdateRequest {
                    content: params.content,
                    base_version: expected_version(params.expected_version),
                },
            )
            .await
            .map_err(AppServerErrorBody::from_talos_error)
    }

    async fn write_config_value(
        &self,
        _actor: &Actor,
        params: ConfigValueWriteParams,
    ) -> ProtocolResult<Value> {
        if params.path != "llm" {
            return Err(AppServerErrorBody::invalid_params(format!(
                "unsupported config path: {}",
                params.path
            )));
        }
        let llm = serde_json::from_str(&params.value)
            .map_err(|error| AppServerErrorBody::invalid_params(error.to_string()))?;
        let updated = self
            .app
            .config
            .update_llm(llm)
            .map_err(AppServerErrorBody::from_talos_error)?;
        to_value(updated)
    }

    async fn write_config_batch(
        &self,
        actor: &Actor,
        params: ConfigBatchWriteParams,
    ) -> ProtocolResult<Value> {
        let mut last = None;
        for entry in params.entries {
            last = Some(self.write_config_value(actor, entry).await?);
        }
        Ok(last.unwrap_or_else(|| json!({ "success": true })))
    }

    fn list_tools(&self) -> Vec<Value> {
        let Some(registry) = self.tool_registry.as_ref() else {
            return Vec::new();
        };
        registry
            .tool_catalog()
            .specs()
            .iter()
            .map(|spec| {
                json!({
                    "name": spec.definition.name,
                    "description": spec.definition.description,
                    "parameters_schema": spec.definition.parameters,
                    "internal_name": spec.internal_name,
                    "aliases": spec.aliases,
                    "user_facing_name": spec.user_facing_name,
                    "source": spec.source,
                    "payload_kind": spec.payload_kind,
                    "visibility": spec.visibility,
                    "parallelism": spec.parallelism,
                    "mutating": spec.mutating,
                    "behavior_flags": spec.behavior_flags,
                    "result_policy": spec.result_policy,
                })
            })
            .collect()
    }

    fn list_skills(&self) -> Vec<Value> {
        self.list_tools()
            .into_iter()
            .filter(|tool| tool.get("source").and_then(Value::as_str) == Some("Skill"))
            .map(|tool| {
                json!({
                    "id": tool.get("internal_name").cloned().unwrap_or_else(|| json!("skill")),
                    "name": tool.get("name").cloned().unwrap_or_else(|| json!("Skill")),
                    "description": tool.get("description").cloned().unwrap_or_else(|| json!("")),
                    "source": "tool_registry",
                    "tool": tool,
                })
            })
            .collect()
    }

    fn list_apps(&self) -> Vec<Value> {
        vec![
            json!({
                "id": "webui",
                "name": "Talos WebUI",
                "description": "Browser shell backed by the HTTP gateway.",
                "runtime": "web",
                "available": true,
            }),
            json!({
                "id": "desktop",
                "name": "Talos Desktop",
                "description": "Electron shell backed by the stdio app-server bridge.",
                "runtime": "desktop",
                "available": true,
            }),
        ]
    }

    async fn dispatch_secondary(
        &self,
        _actor: &Actor,
        request: ClientRequest,
    ) -> ProtocolResult<Value> {
        match request.params {
            ClientRequestParams::MemorySearch(params) => {
                let results = self
                    .app
                    .memory
                    .search_files(MemorySearchParams {
                        query: params.query,
                        limit: params.limit,
                        date_from: None,
                        date_to: None,
                    })
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?;
                to_value(json!({ "data": results }))
            }
            _ => Err(AppServerErrorBody::method_not_found(
                request.method.as_str(),
            )),
        }
    }
}

fn map_context(context: ProtocolTalosContext) -> ProtocolResult<TalosContext> {
    let source_type = match context.source_type.as_str() {
        "http" => TalosSourceType::Http,
        "desktop" => TalosSourceType::Desktop,
        "im" => TalosSourceType::Im,
        "cli" => TalosSourceType::Cli,
        "ws" => TalosSourceType::Ws,
        "system" => TalosSourceType::System,
        other => {
            return Err(AppServerErrorBody::from_talos_error(
                TalosError::ValidationError(format!("unsupported source_type: {other}")),
            ))
        }
    };
    let provider = match context.provider.as_str() {
        "http" => TalosProvider::Http,
        "sdk" => TalosProvider::Sdk,
        "cli" => TalosProvider::Cli,
        "telegram" => TalosProvider::Telegram,
        "websocket" => TalosProvider::WebSocket,
        "system" => TalosProvider::System,
        other => {
            return Err(AppServerErrorBody::from_talos_error(
                TalosError::ValidationError(format!("unsupported provider: {other}")),
            ))
        }
    };
    Ok(TalosContext {
        source_type,
        provider,
        user_id: String::new(),
        message_id: context.message_id,
        trace_id: context.trace_id,
        channel_id: Some(context.channel_id),
        thread_id: context.thread_id,
        project_root: None,
        environment: None,
    })
}

fn render_active_turn(session_id: &str, active_turn: Option<ActiveTurnRecord>) -> Value {
    match active_turn {
        Some(record) => json!({
            "session_id": record.session_id,
            "turn_id": record.turn_id,
            "message_id": record.message_id,
            "status": record.status,
            "last_event_id": record.last_event_id.map(|value| value.to_string()),
            "pending_user_action": record.pending_user_action,
            "updated_at": record.updated_at,
        }),
        None => json!({
            "session_id": session_id,
            "turn_id": null,
            "message_id": null,
            "status": "completed",
            "last_event_id": null,
            "pending_user_action": null,
            "updated_at": current_time_millis(),
        }),
    }
}

fn current_time_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

fn map_input_item(item: ProtocolInputItem) -> TalosInputItem {
    match item {
        ProtocolInputItem::Text { text } => TalosInputItem::Text { text },
    }
}

fn map_project_patch(patch: OptionalStringPatch) -> OptionalProjectIdPatch {
    match patch {
        OptionalStringPatch::Set { value } => OptionalProjectIdPatch::Set { project_id: value },
        OptionalStringPatch::Clear => OptionalProjectIdPatch::Clear,
    }
}

fn map_directory_patch(patch: OptionalPathPatch) -> OptionalCurrentDirectoryPatch {
    match patch {
        OptionalPathPatch::Set { path } => OptionalCurrentDirectoryPatch::Set { path },
        OptionalPathPatch::Clear => OptionalCurrentDirectoryPatch::Clear,
    }
}

fn map_user_role(role: AdminUserRole) -> UserRole {
    match role {
        AdminUserRole::User => UserRole::User,
        AdminUserRole::Admin => UserRole::Admin,
    }
}

fn map_message_command_status(status: IdempotencyStatus) -> MessageCommandStatus {
    match status {
        IdempotencyStatus::Pending => MessageCommandStatus::Pending,
        IdempotencyStatus::Accepted => MessageCommandStatus::Accepted,
        IdempotencyStatus::Failed => MessageCommandStatus::Failed,
        IdempotencyStatus::FailedNoFirstEvent => MessageCommandStatus::FailedNoFirstEvent,
    }
}

fn request_identity(actor: &Actor) -> RequestIdentity {
    RequestIdentity {
        user_id: actor.user_id.clone(),
        username: Some(actor.user_id.clone()),
        role: if actor.has_full_access() {
            UserRole::Admin
        } else {
            UserRole::User
        },
        scopes: Vec::<AuthScope>::new(),
        auth_disabled: false,
    }
}

fn parse_event_id(value: Option<String>) -> ProtocolResult<Option<SessionEventId>> {
    value
        .map(|raw| {
            let event_id = SessionEventId::from(raw);
            event_id
                .event_seq()
                .map_err(AppServerErrorBody::from_talos_error)?;
            Ok(event_id)
        })
        .transpose()
}

fn to_value<T: Serialize>(value: T) -> ProtocolResult<Value> {
    serde_json::to_value(value)
        .map_err(|error| AppServerErrorBody::invalid_request(error.to_string()))
}

fn memory_date_from_path(path: &std::path::Path) -> ProtocolResult<MemoryDate> {
    let stem = path
        .file_stem()
        .or_else(|| path.file_name())
        .and_then(|value| value.to_str())
        .ok_or_else(|| AppServerErrorBody::invalid_params("memory file path is invalid"))?;
    MemoryDate::parse(stem).map_err(AppServerErrorBody::from_talos_error)
}

fn expected_version(value: Option<String>) -> ExpectedFileVersion {
    match value {
        Some(version_token) => ExpectedFileVersion::Match { version_token },
        None => ExpectedFileVersion::MustNotExist,
    }
}
