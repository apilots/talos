use async_trait::async_trait;
use serde::Serialize;
use serde_json::{json, Value};
use talos_app_protocol::errors::AppServerErrorBody;
use talos_app_protocol::requests::{
    AuthSessionReadResponse, AuthSessionSnapshot, AuthSessionUserSnapshot, ClientRequest,
    ClientRequestParams, ConfigBatchWriteParams, ConfigReadParams, ConfigValueWriteParams,
    EventSubscribeParams, MemoryFileReadParams, MemoryFileWriteParams, MessageCommandStatus,
    OptionalPathPatch, OptionalStringPatch, ProjectCreateParams, ProjectPatchParams,
    ProtocolInputItem, ProtocolTalosContext, SessionListParams, SessionPatchParams,
    TimelineActiveTurnSnapshot, TimelineActiveTurnStatus, TimelineCursorStatus,
    TimelinePendingDynamicToolResult, TimelinePendingUserAction, TimelineRecoverySnapshot,
    TimelineReplay, TurnStartParams, TurnStartResponse, WorkspaceInfoResponse,
};
use talos_application_service::{
    Actor, MemoryDate, MemoryListParams, MemorySearchParams, TalosApplicationService,
    TimelineCursorStatus as ApplicationTimelineCursorStatus, TimelineSubscribeParams,
    TimelineSubscription, TurnRetryRequest, WorkspaceDocService,
};
use talos_auth::{
    AuthScope, CloudLoginService, CloudSessionCache, CloudSessionCacheEntry,
    CloudSessionCacheManager, CloudSessionUserSnapshot, RequestIdentity,
};
use talos_config::TalosPaths;
use talos_error::TalosError;
use talos_session_store::{
    ActiveTurnRecord, ActiveTurnStatus, IdempotencyStatus, PendingUserAction, TimelineEventId,
};
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

    async fn subscribe_timeline_events(
        &self,
        actor: &Actor,
        params: EventSubscribeParams,
    ) -> ProtocolResult<SubscriptionSource>;

    async fn cancel_active_turn(&self, actor: &Actor, session_id: &str) -> ProtocolResult<()>;

    async fn drain_background_tasks(&self, timeout: Duration) -> TalosResult<()>;
}

pub type SubscriptionSource = TimelineSubscription;

#[derive(Clone)]
pub struct TalosApplicationAdapter {
    app: std::sync::Arc<TalosApplicationService>,
    replay_limit: usize,
    drain_provider: Arc<Option<Box<dyn BackgroundTaskDrain>>>,
    tool_registry: Option<Arc<ToolRegistry>>,
    cloud_session_cache: CloudSessionCacheManager,
    cloud_api_base_url: String,
    cloud_login: Arc<CloudLoginService>,
}

impl TalosApplicationAdapter {
    pub fn new(app: std::sync::Arc<TalosApplicationService>, replay_limit: usize) -> Self {
        let cloud_session_cache =
            CloudSessionCacheManager::for_talos_home(TalosPaths::resolve().talos_home);
        let cloud_api_base_url = "http://127.0.0.1:8787".to_string();
        Self {
            app,
            replay_limit: replay_limit.max(1),
            drain_provider: Arc::new(None),
            tool_registry: None,
            cloud_session_cache: cloud_session_cache.clone(),
            cloud_api_base_url: cloud_api_base_url.clone(),
            cloud_login: Arc::new(CloudLoginService::new(
                cloud_api_base_url,
                cloud_session_cache,
            )),
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

    pub fn with_cloud_session_cache(mut self, cache: CloudSessionCache) -> Self {
        self.cloud_session_cache = CloudSessionCacheManager::new(cache);
        self.cloud_login = Arc::new(CloudLoginService::new(
            self.cloud_api_base_url.clone(),
            self.cloud_session_cache.clone(),
        ));
        self
    }

    pub fn with_cloud_api_base_url(mut self, api_base_url: impl Into<String>) -> Self {
        self.cloud_api_base_url = api_base_url.into();
        self.cloud_login = Arc::new(CloudLoginService::new(
            self.cloud_api_base_url.clone(),
            self.cloud_session_cache.clone(),
        ));
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
            ClientRequestParams::SessionHistory(params) => to_value(
                self.app
                    .get_session_history(actor, &params.session_id)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?,
            ),
            ClientRequestParams::SessionRecover(params) => {
                self.app
                    .get_session(actor, &params.session_id)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?;
                let after_event_id = parse_event_id(params.after_event_id)?;
                let snapshot = self
                    .app
                    .events
                    .recover_session(
                        &params.session_id,
                        after_event_id.as_ref(),
                        params.replay_limit.unwrap_or(self.replay_limit).max(1),
                    )
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?;
                to_value(TimelineRecoverySnapshot {
                    replay: TimelineReplay {
                        events: snapshot.replay.events,
                        high_water_mark_event_id: snapshot
                            .replay
                            .high_water_mark_event_id
                            .map(|value| value.to_string()),
                        last_replay_seq: snapshot.replay.last_replay_seq,
                        cursor_status: map_timeline_cursor_status(snapshot.replay.cursor_status),
                    },
                    active_turn: map_active_turn_snapshot(&params.session_id, snapshot.active_turn),
                })
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
            ClientRequestParams::ModelList(_) => {
                let models = self.app.config.list_models().await;
                to_value(json!({ "data": models }))
            }
            ClientRequestParams::ConfigRead(ConfigReadParams {}) => {
                let llm = self
                    .app
                    .config
                    .get_llm_read()
                    .map_err(AppServerErrorBody::from_talos_error)?;
                to_value(llm)
            }
            ClientRequestParams::MemoryFileList(params) => {
                let files = self
                    .app
                    .memory
                    .list_files(
                        actor,
                        MemoryListParams {
                            limit: params.limit,
                            before_date: None,
                            date_from: None,
                            date_to: None,
                        },
                    )
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?;
                to_value(json!({ "data": files }))
            }
            ClientRequestParams::MemoryFileRead(params) => {
                to_value(self.read_memory_file(actor, params).await?)
            }
            ClientRequestParams::MemoryFileWrite(params) => {
                to_value(self.write_memory_file(actor, params).await?)
            }
            ClientRequestParams::WorkspaceDocList(_) => {
                let docs = self
                    .app
                    .workspace_docs
                    .list_docs(actor)
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
                        .get_doc(actor, kind)
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
            ClientRequestParams::WorkspaceInfo(_) => {
                let info = self
                    .app
                    .workspace_docs
                    .workspace_info(actor)
                    .map_err(AppServerErrorBody::from_talos_error)?;
                to_value(WorkspaceInfoResponse {
                    user_id: info.user_id,
                    username: info.username,
                    base_workspace_dir: info.base_workspace_dir,
                    user_workspace_dir: info.user_workspace_dir,
                })
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
            ClientRequestParams::AuthLogout(_) => {
                self.app
                    .account
                    .logout(&request_identity(actor))
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?;
                self.cloud_session_cache
                    .clear()
                    .map_err(AppServerErrorBody::from_talos_error)?;
                to_value(json!({ "success": true }))
            }
            ClientRequestParams::AuthLoginStart(params) => to_value(
                self.cloud_login
                    .start(params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?,
            ),
            ClientRequestParams::AuthLoginStatus(params) => to_value(
                self.cloud_login
                    .status(params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?,
            ),
            ClientRequestParams::AuthLoginCancel(params) => {
                self.cloud_login
                    .cancel(params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?;
                to_value(json!({ "success": true }))
            }
            ClientRequestParams::AuthSessionRead(_) => {
                let session = self
                    .cloud_session_cache
                    .read_candidate(CloudSessionCache::now_ms())
                    .map_err(AppServerErrorBody::from_talos_error)?
                    .map(auth_session_snapshot_from_cache_entry);
                to_value(AuthSessionReadResponse { session })
            }
            ClientRequestParams::AuthSessionStore(params) => {
                let actor_token = actor.access_token.as_deref().ok_or_else(|| {
                    AppServerErrorBody::from_talos_error(TalosError::AuthError(
                        "cloud session store requires verified cloud actor credentials".to_string(),
                    ))
                })?;
                if actor_token != params.session.access_token {
                    return Err(AppServerErrorBody::from_talos_error(TalosError::AuthError(
                        "cloud session token does not match actor credentials".to_string(),
                    )));
                }
                let entry = auth_session_cache_entry_from_snapshot(
                    params.session,
                    CloudSessionCache::now_ms(),
                )?;
                self.cloud_session_cache
                    .store_verified_session(&entry)
                    .map_err(AppServerErrorBody::from_talos_error)?;
                to_value(json!({ "success": true }))
            }
            ClientRequestParams::AuthSessionClear(_) => {
                self.cloud_session_cache
                    .clear()
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
            ClientRequestParams::MemorySearch(_) => self.dispatch_secondary(actor, request).await,
        }
    }

    async fn subscribe_timeline_events(
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
        self.app
            .events
            .subscribe(TimelineSubscribeParams {
                session_id: params.session_id,
                after_event_id,
                replay_limit: limit,
                close_when_idle: params.close_when_idle.unwrap_or(false),
                close_on_terminal: params.close_on_terminal.unwrap_or(false),
            })
            .await
            .map_err(AppServerErrorBody::from_talos_error)
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
                    context: params.context.map(map_context).transpose()?,
                    model: params.model,
                    project_id: params.project_id,
                    current_directory: params.current_directory,
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
        actor: &Actor,
        params: MemoryFileReadParams,
    ) -> ProtocolResult<talos_types::MemoryFileDetail> {
        let date = memory_date_from_path(&params.path)?;
        self.app
            .memory
            .get_file(actor, date)
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
            .update_llm_read(llm)
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
        actor: &Actor,
        request: ClientRequest,
    ) -> ProtocolResult<Value> {
        match request.params {
            ClientRequestParams::MemorySearch(params) => {
                let results = self
                    .app
                    .memory
                    .search_files(
                        actor,
                        MemorySearchParams {
                            query: params.query,
                            limit: params.limit,
                            date_from: None,
                            date_to: None,
                        },
                    )
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
    serde_json::to_value(map_active_turn_snapshot(session_id, active_turn))
        .unwrap_or_else(|_| json!({ "session_id": session_id, "status": "failed" }))
}

fn map_active_turn_snapshot(
    session_id: &str,
    active_turn: Option<ActiveTurnRecord>,
) -> TimelineActiveTurnSnapshot {
    match active_turn {
        Some(record) => TimelineActiveTurnSnapshot {
            session_id: record.session_id,
            turn_id: Some(record.turn_id),
            message_id: record.message_id,
            status: map_active_turn_status(record.status),
            last_event_id: record.last_event_id.map(|value| value.to_string()),
            pending_user_action: record.pending_user_action.map(map_pending_user_action),
            updated_at: record.updated_at,
        },
        None => TimelineActiveTurnSnapshot {
            session_id: session_id.to_string(),
            turn_id: None,
            message_id: None,
            status: TimelineActiveTurnStatus::Completed,
            last_event_id: None,
            pending_user_action: None,
            updated_at: current_time_millis(),
        },
    }
}

fn map_active_turn_status(status: ActiveTurnStatus) -> TimelineActiveTurnStatus {
    match status {
        ActiveTurnStatus::Running => TimelineActiveTurnStatus::Running,
        ActiveTurnStatus::WaitingUserAction => TimelineActiveTurnStatus::WaitingUserAction,
        ActiveTurnStatus::Cancelling => TimelineActiveTurnStatus::Cancelling,
        ActiveTurnStatus::Completed => TimelineActiveTurnStatus::Completed,
        ActiveTurnStatus::Failed => TimelineActiveTurnStatus::Failed,
    }
}

fn map_pending_user_action(action: PendingUserAction) -> TimelinePendingUserAction {
    match action {
        PendingUserAction::DynamicToolResult(pending) => {
            TimelinePendingUserAction::DynamicToolResult(TimelinePendingDynamicToolResult {
                item_id: pending.item_id,
                call_id: pending.call_id,
                tool_name: pending.tool_name,
            })
        }
    }
}

fn map_timeline_cursor_status(status: ApplicationTimelineCursorStatus) -> TimelineCursorStatus {
    match status {
        ApplicationTimelineCursorStatus::Current => TimelineCursorStatus::Current,
        ApplicationTimelineCursorStatus::FromBeginning => TimelineCursorStatus::FromBeginning,
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
        role: actor.role,
        scopes: Vec::<AuthScope>::new(),
        auth_disabled: false,
        access_token: actor.access_token.clone(),
    }
}

fn auth_session_snapshot_from_cache_entry(entry: CloudSessionCacheEntry) -> AuthSessionSnapshot {
    AuthSessionSnapshot {
        schema_version: entry.schema_version,
        cloud_api_base_url: entry.cloud_api_base_url,
        access_token: entry.access_token,
        token_type: entry.token_type,
        expires_at_ms: entry.expires_at_ms.map(|value| value.to_string()),
        last_used_at_ms: entry.last_used_at_ms.to_string(),
        user: AuthSessionUserSnapshot {
            user_id: entry.user.user_id,
            username: entry.user.username,
            email: entry.user.email,
            role: entry.user.role,
        },
    }
}

fn auth_session_cache_entry_from_snapshot(
    snapshot: AuthSessionSnapshot,
    last_used_at_ms: i64,
) -> ProtocolResult<CloudSessionCacheEntry> {
    Ok(CloudSessionCacheEntry {
        schema_version: snapshot.schema_version,
        cloud_api_base_url: snapshot.cloud_api_base_url,
        access_token: snapshot.access_token,
        token_type: snapshot.token_type,
        expires_at_ms: parse_optional_i64(snapshot.expires_at_ms.as_deref(), "expires_at_ms")?,
        last_used_at_ms,
        user: CloudSessionUserSnapshot {
            user_id: snapshot.user.user_id,
            username: snapshot.user.username,
            email: snapshot.user.email,
            role: snapshot.user.role,
        },
    })
}

fn parse_optional_i64(value: Option<&str>, field: &str) -> ProtocolResult<Option<i64>> {
    value.map(|value| parse_i64(value, field)).transpose()
}

fn parse_i64(value: &str, field: &str) -> ProtocolResult<i64> {
    value.parse::<i64>().map_err(|err| {
        AppServerErrorBody::from_talos_error(TalosError::ValidationError(format!(
            "invalid cloud session {field}: {err}"
        )))
    })
}

fn parse_event_id(value: Option<String>) -> ProtocolResult<Option<TimelineEventId>> {
    value
        .map(|raw| {
            let event_id = TimelineEventId::from(raw);
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

#[cfg(test)]
mod tests {
    use super::*;
    use talos_auth::UserRole;

    #[test]
    fn auth_session_store_uses_server_last_used_time() {
        let client_last_used_at_ms = 9_999_999;
        let server_last_used_at_ms = 1_000_000;

        let entry = auth_session_cache_entry_from_snapshot(
            AuthSessionSnapshot {
                schema_version: 1,
                cloud_api_base_url: "http://127.0.0.1:8787".to_string(),
                access_token: "token".to_string(),
                token_type: "Bearer".to_string(),
                expires_at_ms: Some("2000000".to_string()),
                last_used_at_ms: client_last_used_at_ms.to_string(),
                user: AuthSessionUserSnapshot {
                    user_id: "user-1".to_string(),
                    username: "user-1".to_string(),
                    email: Some("user@example.com".to_string()),
                    role: UserRole::Admin,
                },
            },
            server_last_used_at_ms,
        )
        .expect("snapshot should map");

        assert_eq!(entry.last_used_at_ms, server_last_used_at_ms);
    }
}
