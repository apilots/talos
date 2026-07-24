use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use talos_agent::{AgentDefinitionLoader, AgentHarness, AgentHarnessFactory, SkillRootsResolver};
use talos_app_server::BackgroundTaskDrain;
use talos_auth::{AuthService, CloudAuthService, OpenModeUserService, UserService};
use talos_automation::Automation;
use talos_channel::{ChannelServer, TelegramChannel};
use talos_config::{
    effective_llm_protocol, AgentKernelConfig, Config, LlmConfigManager, LlmConfigService,
    LlmProtocol, McpTransportConfig, ProviderConfig, YamlLlmConfigStore,
};
use talos_coordinator::{Coordinator, CoordinatorImpl, SessionRuntime};
use talos_error::{TalosError, TalosResult};
use talos_gateway::{Gateway, ServiceGateway};
use talos_gateway_http::{HttpServer, ServerLifecycle, ServerLifecycleState};
use talos_llm_anthropic::AnthropicProvider;
use talos_llm_bailian::BailianProvider;
use talos_llm_deepseek::{DeepSeekConfig, DeepSeekProtocol, DeepSeekProvider};
use talos_llm_ollama::{OllamaConfig, OllamaProvider};
use talos_llm_openai::{OpenAIProtocol, OpenAIProvider};
use talos_llm_openrouter::{OpenRouterConfig, OpenRouterProtocol, OpenRouterProvider};
use talos_llm_provider::{
    ChatRequest, ChatResponse, ChatStream, LlmProvider, LlmProviderRouter, ModelRef,
    ReloadableLlmProviderRouter,
};
use talos_logging::LogGuard;
use talos_router::{Binding, MatchRule, Router, RouterImpl};
use talos_service::{
    Actor, AgentDispatchTurnExecutor, CreateSessionRequest, FileService, MemoryService,
    PluginManagerService, TalosService, TimelineSubscribeCommand, TimelineSubscriptionMessage,
};
use talos_service::{TimelineEventRetentionConfig, TimelineEventService};
use talos_store::memory::MemoryStore;
use talos_store::project::{ProjectStore, StorageProjectStore};
use talos_store::session::{
    ActiveTurnStore, DatabaseSessionStore, FileSessionArtifactStore, MessageIdempotencyStore,
    SessionArtifactStore, SessionStore, TimelineEventStore,
};
use talos_tools::{
    register_collaboration_tools, register_core_local_tools, register_network_tools,
    CollaborationAgentHost, CollaborationRuntime, McpConnectionManager, ToolRegistry,
};
use talos_types::talos::{
    AgentEventPayload, TalosInputItem, TalosTimelineContent, TalosTimelineDelta,
};
use talos_types::{
    resolve_default_model_selection, resolve_model_catalog, AgentDispatchResult,
    AgentDispatchRuntime, McpServerDefinition, McpServerRuntimeDescriptor, McpServerSource,
    McpTransportDefinition, StartTurnRequest, TalosContext, TalosSession,
};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

const TALOS_WEBUI_DIR_ENV: &str = "TALOS_WEBUI_DIR";

mod application_automation;
mod application_storage;
mod reloadable_llm_provider;

use reloadable_llm_provider::ReloadableLlmProvider;

#[derive(Clone)]
struct AgentSkillRootsResolver {
    plugins: PluginManagerService,
}

impl SkillRootsResolver for AgentSkillRootsResolver {
    fn skill_roots(
        &self,
        kernel_config: &AgentKernelConfig,
    ) -> TalosResult<talos_skills::SkillRoots> {
        self.plugins.skill_roots_for_kernel_config(kernel_config)
    }
}

async fn register_mcp_tools(
    tool_registry: &ToolRegistry,
    kernel_config: &AgentKernelConfig,
    plugin_manager: &PluginManagerService,
) -> TalosResult<()> {
    if !kernel_config.tools.enable_mcp_tools {
        return Ok(());
    }
    let mut servers = configured_mcp_servers(kernel_config)?;
    servers.extend(plugin_manager.mcp_servers()?);
    if servers.is_empty() {
        return Ok(());
    }
    let runtime = Arc::new(talos_mcp::TalosMcpRuntime::connect_all(servers).await);
    let manager = Arc::new(McpConnectionManager::from_runtime(runtime).await);
    tool_registry.register_mcp_server(manager);
    Ok(())
}

fn configured_mcp_servers(
    kernel_config: &AgentKernelConfig,
) -> TalosResult<Vec<McpServerRuntimeDescriptor>> {
    kernel_config
        .mcp
        .servers
        .iter()
        .map(|server| {
            let transport = match &server.transport {
                McpTransportConfig::Stdio { command, args, env } => McpTransportDefinition::Stdio {
                    command: command.clone(),
                    args: args.clone(),
                    env: env
                        .iter()
                        .map(|(key, value)| (key.clone(), value.clone()))
                        .collect(),
                    cwd: None,
                },
                McpTransportConfig::Http { url, .. } => McpTransportDefinition::StreamableHttp {
                    url: url.clone(),
                    headers: Default::default(),
                },
            };
            Ok(McpServerRuntimeDescriptor {
                server: McpServerDefinition {
                    name: server.name.clone(),
                    display_name: None,
                    description: None,
                    transport,
                },
                source: McpServerSource::Config,
            })
        })
        .collect()
}

#[derive(Debug, Clone, Default)]
pub struct TalosServerOptions {
    pub host: Option<String>,
    pub port: Option<u16>,
    pub logging_stdout: Option<bool>,
    pub auth_disabled: Option<bool>,
    pub enable_channels: Option<bool>,
    pub webui_dir: Option<PathBuf>,
}

pub struct TalosServerRuntime {
    http_server: HttpServer,
    channel_server: Option<ChannelServer>,
    coordinator: Arc<CoordinatorImpl>,
    gateway_url: String,
    shutdown_token: CancellationToken,
    lifecycle: Arc<ServerLifecycle>,
    automation: Automation,
}

#[derive(Clone)]
pub struct TalosApplicationRuntime {
    pub config: Config,
    pub service: Arc<TalosService>,
    pub event_service: Arc<TimelineEventService>,
    pub auth_service: Arc<dyn AuthService>,
    pub gateway: Arc<dyn Gateway>,
    pub agent_dispatch_runtime: Arc<dyn AgentDispatchRuntime>,
    pub coordinator: Arc<CoordinatorImpl>,
    pub tool_registry: Arc<ToolRegistry>,
    automation: Automation,
}

struct ApplicationCollaborationHost {
    service: Arc<TalosService>,
    session_users: RwLock<HashMap<String, String>>,
}

impl ApplicationCollaborationHost {
    fn new(service: Arc<TalosService>) -> Self {
        Self {
            service,
            session_users: RwLock::new(HashMap::new()),
        }
    }

    fn next_session_id() -> String {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        format!("collab_{nanos}")
    }

    fn actor_for_user(user_id: &str) -> Actor {
        Actor::regular(user_id.to_string())
    }
}

#[async_trait]
impl CollaborationAgentHost for ApplicationCollaborationHost {
    async fn create_session(&self, context: TalosContext) -> TalosResult<TalosSession> {
        let session_id = Self::next_session_id();
        let user_id = context.user_id.clone();
        let actor = Self::actor_for_user(&user_id);
        let session = self
            .service
            .create_session(
                &actor,
                CreateSessionRequest {
                    session_id,
                    context,
                    model_selection: None,
                    workspace_id: None,
                    project_id: None,
                    current_directory: None,
                },
            )
            .await?;
        self.session_users
            .write()
            .expect("collaboration session users write lock")
            .insert(session.context.session_id.clone(), user_id);
        Ok(session)
    }

    async fn get_session(&self, session_id: &str) -> TalosResult<Option<TalosSession>> {
        let Some(user_id) = self
            .session_users
            .read()
            .expect("collaboration session users read lock")
            .get(session_id)
            .cloned()
        else {
            return Ok(None);
        };
        match self
            .service
            .get_session(&Self::actor_for_user(&user_id), session_id)
            .await
        {
            Ok(session) => Ok(Some(session)),
            Err(TalosError::ResourceNotFound(_)) => Ok(None),
            Err(error) => Err(error),
        }
    }

    async fn close_session(&self, session: TalosSession) -> TalosResult<()> {
        let session_id = session.context.session_id.clone();
        self.service
            .close_session(&Self::actor_for_user(&session.user_id), &session_id)
            .await?;
        self.session_users
            .write()
            .expect("collaboration session users write lock")
            .remove(&session_id);
        Ok(())
    }

    async fn submit_input(
        &self,
        session: TalosSession,
        message: String,
    ) -> TalosResult<AgentDispatchResult> {
        let session_id = session.context.session_id.clone();
        let model = session.config.llm.model.clone();
        let request = StartTurnRequest {
            session_id: session_id.clone(),
            input: vec![TalosInputItem::Text { text: message }],
            workspace_id: session.context.workspace_id.clone(),
            context: Some(TalosContext {
                source_type: session.context.source_type.clone(),
                provider: session.context.provider.clone(),
                user_id: session.context.user_id.clone(),
                message_id: Self::next_session_id(),
                trace_id: Self::next_session_id(),
                channel_id: session.context.channel_id.clone(),
                thread_id: session.context.thread_id.clone(),
                project_root: session.context.project_root.clone(),
                environment: session.context.environment.clone(),
            }),
            model_selection: None,
            project_id: None,
            current_directory: None,
            client_request_id: None,
        };
        let ack = self
            .service
            .start_turn(&Self::actor_for_user(&session.user_id), request, None)
            .await?;
        let mut subscription = self
            .service
            .events
            .subscribe(TimelineSubscribeCommand {
                session_id,
                turn_id: Some(ack.turn_id.clone()),
                after_event_id: None,
                replay_limit: 1_000,
                close_when_idle: true,
                close_on_terminal: true,
            })
            .await?;
        let mut output = String::new();
        while let Some(message) = subscription.events.recv().await {
            let TimelineSubscriptionMessage::Event(envelope) = message else {
                continue;
            };
            if envelope.turn_id != ack.turn_id {
                continue;
            }
            match envelope.event.payload {
                AgentEventPayload::ItemDelta(delta) => {
                    if let TalosTimelineDelta::Text { text } = delta.delta {
                        output.push_str(&text);
                    }
                }
                AgentEventPayload::ItemCompleted(completed) => {
                    if let Some(TalosTimelineContent::Text { text }) = completed.final_content {
                        output = text;
                    }
                }
                AgentEventPayload::TurnCompleted(completed) => {
                    return Ok(AgentDispatchResult {
                        session_id: envelope.session_id,
                        output_text: output,
                        stop_reason: completed.stop_reason,
                        usage: completed.usage,
                        model,
                        output_items: Vec::new(),
                        reasoning: Default::default(),
                    });
                }
                AgentEventPayload::TurnError(error) => {
                    return Err(TalosError::GatewayError(error.error));
                }
                _ => {}
            }
        }
        Err(TalosError::GatewayError(format!(
            "collaboration turn {} ended without terminal event",
            ack.turn_id
        )))
    }
}

const SERVER_SHUTDOWN_WAIT_TIMEOUT: Duration = Duration::from_secs(180);

pub async fn run_app_server() -> TalosResult<()> {
    let config = load_runtime_config()?;
    let config_source = resolve_config_source_path(&config);
    let logging_options = TalosServerOptions {
        logging_stdout: Some(false),
        ..TalosServerOptions::default()
    };
    let _log_guard = init_logging(&config, Some(&logging_options))?;
    log_config_source(config_source);

    run_json_rpc_stdio(config).await.map_err(|error| {
        error!("app-server error: {}", error);
        error
    })
}

pub async fn run_server() -> TalosResult<()> {
    let config = load_runtime_config()?;
    let config_source = resolve_config_source_path(&config);
    let mut server_options = TalosServerOptions::default();
    let _log_guard = init_logging(&config, Some(&server_options))?;
    log_config_source(config_source);
    server_options.webui_dir = resolve_packaged_webui_dir();
    info!(
        "Starting Talos server at {}:{}",
        config.gateway.host, config.gateway.port
    );

    let server = start_server(config, Some(server_options)).await?;
    server.run().await.map_err(|error| {
        error!("host error: {}", error);
        error
    })
}

pub fn should_print_version(arg: Option<&str>) -> bool {
    matches!(arg, Some("--version" | "-V" | "version"))
}

pub fn should_print_help(arg: Option<&str>) -> bool {
    matches!(arg, Some("--help" | "-h" | "help"))
}

fn load_runtime_config() -> TalosResult<Config> {
    let config = Config::load()?;
    config.validate_runtime()?;
    Ok(config)
}

fn resolve_config_source_path(config: &Config) -> Option<PathBuf> {
    let config_path = config.source_path();
    config_path
        .exists()
        .then(|| canonical_or_original(config_path))
}

fn resolve_packaged_webui_dir() -> Option<PathBuf> {
    let resolution = resolve_webui_dir(WebuiLookupInput {
        env_dir: std::env::var_os(TALOS_WEBUI_DIR_ENV).map(PathBuf::from),
        current_exe: std::env::current_exe().ok(),
        current_dir: std::env::current_dir().ok(),
    });
    match resolution {
        WebuiLookupResult::Found(path) => {
            info!("Serving Talos WebUI from {}", path.display());
            Some(path)
        }
        WebuiLookupResult::InvalidEnv(path) => {
            let index_path = path.join("index.html");
            warn!(
                "{} is set but {} was not found; starting API-only server.",
                TALOS_WEBUI_DIR_ENV,
                index_path.display()
            );
            None
        }
        WebuiLookupResult::Missing => {
            warn!(
                "Talos WebUI bundle not found; starting API-only server. Set {} to serve a WebUI bundle.",
                TALOS_WEBUI_DIR_ENV
            );
            None
        }
    }
}

#[derive(Debug)]
struct WebuiLookupInput {
    env_dir: Option<PathBuf>,
    current_exe: Option<PathBuf>,
    current_dir: Option<PathBuf>,
}

#[derive(Debug, PartialEq, Eq)]
enum WebuiLookupResult {
    Found(PathBuf),
    InvalidEnv(PathBuf),
    Missing,
}

fn resolve_webui_dir(input: WebuiLookupInput) -> WebuiLookupResult {
    if let Some(path) = input.env_dir {
        return if is_webui_dir(&path) {
            WebuiLookupResult::Found(canonical_or_original(path))
        } else {
            WebuiLookupResult::InvalidEnv(path)
        };
    }

    for candidate in webui_dir_candidates(&input) {
        if is_webui_dir(&candidate) {
            return WebuiLookupResult::Found(canonical_or_original(candidate));
        }
    }

    WebuiLookupResult::Missing
}

fn webui_dir_candidates(input: &WebuiLookupInput) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(exe_dir) = input
        .current_exe
        .as_ref()
        .and_then(|path| path.parent().map(PathBuf::from))
    {
        candidates.push(exe_dir.join("webui"));
        candidates.push(exe_dir.join("..").join("webui"));
    }
    if let Some(current_dir) = input.current_dir.as_ref() {
        candidates.push(current_dir.join("webui"));
    }
    candidates
}

fn is_webui_dir(path: &std::path::Path) -> bool {
    path.join("index.html").is_file()
}

fn canonical_or_original(path: PathBuf) -> PathBuf {
    path.canonicalize().unwrap_or(path)
}

fn log_config_source(config_source: Option<PathBuf>) {
    if let Some(path) = config_source {
        info!("Config loaded from {}", path.display());
    }
}

pub fn exit_with_error(context: &str, error: TalosError) -> ! {
    eprintln!("{context}: {error}");
    std::process::exit(1);
}

/// Bridges `CoordinatorImpl` to the `BackgroundTaskDrain` trait used by the app-server.
struct CoordinatorDrain {
    coordinator: Arc<CoordinatorImpl>,
}

#[async_trait::async_trait]
impl BackgroundTaskDrain for CoordinatorDrain {
    async fn drain(&self, timeout: Duration) -> TalosResult<()> {
        self.coordinator.drain_background_tasks(timeout).await
    }
}

#[derive(Clone)]
pub struct TalosServerHandle {
    shutdown_token: CancellationToken,
    lifecycle: Arc<ServerLifecycle>,
    wait_timeout: Duration,
}

impl TalosServerHandle {
    pub async fn shutdown(&self) -> TalosResult<()> {
        if self.lifecycle.current() == ServerLifecycleState::Stopped {
            return Ok(());
        }

        self.shutdown_token.cancel();

        let mut updates = self.lifecycle.subscribe();
        let wait_timeout = self.wait_timeout;
        tokio::time::timeout(wait_timeout, async move {
            loop {
                if *updates.borrow_and_update() == ServerLifecycleState::Stopped {
                    return Ok(());
                }
                updates.changed().await.map_err(|_| {
                    TalosError::InternalError(
                        "server runtime stopped before reporting shutdown completion".to_string(),
                    )
                })?;
            }
        })
        .await
        .map_err(|_| {
            TalosError::InternalError(format!(
                "server shutdown timed out after {} secs",
                wait_timeout.as_secs()
            ))
        })?
    }
}

impl TalosServerRuntime {
    pub fn handle(&self) -> TalosServerHandle {
        TalosServerHandle {
            shutdown_token: self.shutdown_token.clone(),
            lifecycle: self.lifecycle.clone(),
            wait_timeout: SERVER_SHUTDOWN_WAIT_TIMEOUT,
        }
    }

    pub fn gateway_url(&self) -> &str {
        &self.gateway_url
    }

    pub async fn run(self) -> TalosResult<()> {
        let TalosServerRuntime {
            http_server,
            channel_server,
            coordinator,
            gateway_url: _,
            shutdown_token,
            lifecycle,
            automation,
        } = self;

        let signal_shutdown = shutdown_token.clone();
        let signal_task = tokio::spawn(async move {
            application_automation::shutdown_signal().await;
            signal_shutdown.cancel();
        });

        let quiesce_lifecycle = lifecycle.clone();
        let quiesce_shutdown = shutdown_token.clone();
        let quiesce_task = tokio::spawn(async move {
            quiesce_shutdown.cancelled().await;
            if !quiesce_lifecycle.current().is_quiescing_or_later() {
                quiesce_lifecycle.store(ServerLifecycleState::Quiescing);
            }
        });

        let result = match channel_server {
            Some(channel_server) => {
                let mut tasks = JoinSet::new();
                let shutdown_token_for_http = shutdown_token.clone();
                tasks.spawn(async move {
                    http_server
                        .run_with_shutdown(shutdown_token_for_http.clone())
                        .await
                });
                tasks.spawn(async move { channel_server.run().await });

                let result = match tasks.join_next().await {
                    Some(Ok(result)) => result,
                    Some(Err(err)) => Err(TalosError::InternalError(format!(
                        "server task join error: {}",
                        err
                    ))),
                    None => Ok(()),
                };
                shutdown_token.cancel();
                tasks.abort_all();
                while tasks.join_next().await.is_some() {}
                result
            }
            None => http_server.run_with_shutdown(shutdown_token.clone()).await,
        };

        signal_task.abort();
        quiesce_task.abort();

        let shutdown_result =
            application_automation::shutdown_server(&coordinator, &automation, &lifecycle).await;
        application_automation::combine(result, shutdown_result)
    }
}

pub fn init_logging(
    config: &Config,
    options: Option<&TalosServerOptions>,
) -> TalosResult<LogGuard> {
    let mut config = apply_runtime_options(config.clone(), options);
    if config.logging.timezone.is_none() {
        config.logging.timezone = Some(config.timezone.clone());
    }
    talos_logging::init_with_config(config.logging)
}

pub async fn start_server(
    config: Config,
    options: Option<TalosServerOptions>,
) -> TalosResult<TalosServerRuntime> {
    let config = apply_runtime_options(config, options.as_ref());
    let runtime = build_application_runtime(config.clone()).await?;

    let mut http_server = HttpServer::new(
        runtime.service.clone(),
        runtime.event_service.clone(),
        runtime.auth_service.clone(),
        config.clone(),
        config.gateway.host.clone(),
        config.gateway.port,
    );

    let auth_disabled = options
        .as_ref()
        .and_then(|value| value.auth_disabled)
        .unwrap_or(!config.gateway.auth);

    if auth_disabled {
        http_server = http_server.with_auth_disabled(true);
    }

    if let Some(webui_dir) = options.as_ref().and_then(|value| value.webui_dir.clone()) {
        http_server = http_server.with_webui_dir(webui_dir);
    }

    let enable_channels = options
        .as_ref()
        .and_then(|value| value.enable_channels)
        .unwrap_or(true);
    let channel_server = if enable_channels {
        let mut channel_server = ChannelServer::new(
            Arc::new(config.clone()),
            runtime.gateway.clone(),
            runtime.service.clone(),
        );
        channel_server.register(Box::new(TelegramChannel::new()));
        Some(channel_server)
    } else {
        None
    };

    Ok(TalosServerRuntime {
        coordinator: runtime.coordinator,
        gateway_url: format!("http://{}:{}", config.gateway.host, config.gateway.port),
        shutdown_token: CancellationToken::new(),
        lifecycle: http_server.lifecycle(),
        automation: runtime.automation,
        http_server,
        channel_server,
    })
}

pub async fn run_json_rpc_stdio(config: Config) -> TalosResult<()> {
    let max_incoming_frame_bytes = usize::try_from(
        config
            .storage
            .files
            .limits
            .max_upload_bytes
            .div_ceil(3)
            .saturating_mul(4)
            .saturating_add(1024 * 1024),
    )
    .unwrap_or(usize::MAX);
    let runtime = build_application_runtime(config.clone()).await?;
    let automation = runtime.automation.clone();
    let replay_limit = config.agent.kernel.timeline_events.replay_batch_size.max(1);
    let service = runtime.service.clone();
    let app = Arc::new(
        talos_app_server::TalosApplicationAdapter::new(service.clone(), replay_limit)
            .with_drain_provider(Box::new(CoordinatorDrain {
                coordinator: runtime.coordinator,
            }))
            .with_cloud_api_base_url(config.cloud.api_base_url.clone()),
    );
    let validator = Arc::new(talos_app_server::AuthServiceCredentialValidator::new(
        runtime.auth_service,
        service.account.clone(),
    ));
    let result = talos_app_server::run_stdio_with_max_frame_bytes(
        app,
        validator,
        talos_app_server::AppServerConfig {
            auth_enabled: config.gateway.auth,
            cloud_auth_url: Some(config.cloud.web_base_url.clone()),
            ..talos_app_server::AppServerConfig::default()
        },
        max_incoming_frame_bytes,
    )
    .await
    .map_err(|error| TalosError::InternalError(error.message));
    application_automation::finish(result, &automation).await
}

pub async fn build_application_runtime(config: Config) -> TalosResult<TalosApplicationRuntime> {
    config.validate_runtime()?;
    let (storage_impl, profile_store, file_store) =
        application_storage::build_application_storage(&config).await?;
    let storage: Arc<dyn talos_storage::Storage> = storage_impl;
    let session_store_impl = Arc::new(DatabaseSessionStore::new(storage.clone()).await?);
    let project_store_impl = Arc::new(StorageProjectStore::new(storage.clone()).await?);
    let session_store: Arc<dyn SessionStore> = session_store_impl.clone();
    let project_store: Arc<dyn ProjectStore> = project_store_impl.clone();
    let file_service = Arc::new(FileService::new(
        file_store,
        project_store.clone(),
        application_storage::build_file_operators(&config.storage.files)?,
        config.storage.files.clone(),
    )?);
    file_service.reconcile_incomplete().await?;
    let event_store: Arc<dyn TimelineEventStore> = session_store_impl.clone();
    let active_turn_store: Arc<dyn ActiveTurnStore> = session_store_impl.clone();
    let idempotency_store: Arc<dyn MessageIdempotencyStore> = session_store_impl.clone();
    let event_service = Arc::new(TimelineEventService::new(
        event_store,
        active_turn_store.clone(),
        TimelineEventRetentionConfig {
            retention_hours: config.agent.kernel.timeline_events.retention_hours,
            max_events_per_session: config.agent.kernel.timeline_events.max_events_per_session,
            gc_interval_minutes: config.agent.kernel.timeline_events.gc_interval_minutes,
            gc_batch_size: config.agent.kernel.timeline_events.gc_batch_size,
        },
    ));
    let default_model_selection = resolve_default_model_selection(&config.llm)?;
    let agent_definitions = AgentDefinitionLoader::load(&config.agent, &default_model_selection)?;
    let default_agent_id = agent_definitions
        .iter()
        .find(|definition| definition.enabled)
        .map(|definition| definition.id.clone())
        .ok_or_else(|| TalosError::ValidationError("no enabled agent definitions".to_string()))?;
    let router: Arc<dyn Router> = Arc::new(RouterImpl::new());
    if let Err(err) = router
        .add_binding(Binding {
            id: "default_binding".to_string(),
            agent_id: default_agent_id,
            rule: MatchRule::Default,
            priority: 1,
        })
        .await
    {
        error!("failed to create default routing: {}", err);
    }

    let initial_llm_provider = create_llm_provider(&config)?;
    let reloadable_llm_provider = Arc::new(ReloadableLlmProvider::new(initial_llm_provider));
    let llm_router_config = Arc::new(RwLock::new(config.clone()));
    let llm_router_config_for_factory = llm_router_config.clone();
    let reloadable_llm_provider_router = Arc::new(ReloadableLlmProviderRouter::new(Arc::new(
        move |model_ref: &ModelRef| {
            let provider_name = model_ref.provider.trim().to_ascii_lowercase();
            let mut provider_config = llm_router_config_for_factory
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone();
            provider_config.llm.provider = provider_name;
            provider_config.llm.model = model_ref.id.clone();
            create_llm_provider(&provider_config)
        },
    )));
    let llm_provider_router: Arc<dyn LlmProviderRouter> = reloadable_llm_provider_router.clone();
    let tool_registry = Arc::new(ToolRegistry::new());
    register_core_local_tools(tool_registry.as_ref());
    register_network_tools(tool_registry.as_ref());
    let plugin_manager_service = PluginManagerService::new(
        config.agent.kernel.plugins.clone(),
        config.agent.kernel.skills.clone(),
    );
    register_mcp_tools(
        tool_registry.as_ref(),
        &config.agent.kernel,
        &plugin_manager_service,
    )
    .await?;
    let skill_roots_resolver = Arc::new(AgentSkillRootsResolver {
        plugins: plugin_manager_service.clone(),
    });
    let session_artifact_store: Arc<dyn SessionArtifactStore> =
        Arc::new(FileSessionArtifactStore::default());
    let memory_database = config.talos_paths().data_dir.join("memory-index.sqlite");
    let memory_store = Arc::new(MemoryStore::open(&memory_database).await?);
    let memory_service = MemoryService::new(config.clone(), memory_store)?;
    memory_service.rebuild_known_scopes().await?;
    tool_registry.set_memory_reader(Arc::new(memory_service.clone()));

    let agent_factory = AgentHarnessFactory::new(
        llm_provider_router.clone(),
        tool_registry.clone(),
        skill_roots_resolver,
        session_artifact_store.clone(),
        session_store_impl.clone(),
        Arc::new(memory_service.clone()),
        file_service.clone(),
    );

    let coordinator = Arc::new(CoordinatorImpl::new(router));

    for definition in agent_definitions {
        if !definition.enabled {
            continue;
        }
        let agent_id = definition.id.clone();
        let agent: Arc<dyn AgentHarness> = Arc::new(agent_factory.build(&definition)?);
        coordinator.register_agent(&agent_id, agent).await?;
    }

    let agent_dispatch_runtime: Arc<dyn AgentDispatchRuntime> = coordinator.clone();
    let session_runtime: Arc<dyn SessionRuntime> = coordinator.clone();
    let turn_executor = Arc::new(AgentDispatchTurnExecutor::new(
        agent_dispatch_runtime.clone(),
    ));
    let cloud_auth = Arc::new(CloudAuthService::new(config.cloud.api_base_url.clone()));
    let auth_service: Arc<dyn AuthService> = cloud_auth.clone();
    let user_service: Arc<dyn UserService> = if config.gateway.auth {
        cloud_auth
    } else {
        Arc::new(OpenModeUserService)
    };

    let llm_store = Arc::new(YamlLlmConfigStore::with_env_path(
        config.source_path(),
        config.talos_paths().env_file,
    ));
    let llm_config_service: Arc<dyn LlmConfigService> = Arc::new(LlmConfigManager::new(llm_store));

    let model_catalog = resolve_model_catalog(&config.llm);
    let provider_reloader = reloadable_llm_provider.clone();
    let provider_router_reloader = reloadable_llm_provider_router.clone();
    let provider_router_config_reloader = llm_router_config.clone();
    let runtime_reload_hook: talos_service::RuntimeConfigReloadHook = Arc::new(move |_| {
        let reloaded_config = Config::load()?;
        reloaded_config.validate_runtime()?;
        let reloaded_provider = create_llm_provider(&reloaded_config)?;
        provider_reloader.reload(reloaded_provider);
        *provider_router_config_reloader
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = reloaded_config;
        provider_router_reloader.reload();
        Ok(())
    });
    let service = Arc::new(TalosService::new_with_reload_hook(
        turn_executor,
        session_runtime,
        session_store,
        session_artifact_store.clone(),
        project_store,
        profile_store.clone(),
        file_service,
        event_service.clone(),
        idempotency_store,
        config.clone(),
        memory_service.clone(),
        llm_config_service,
        model_catalog,
        auth_service.clone(),
        user_service,
        Some(runtime_reload_hook),
    ));

    if config.agent.kernel.tools.enable_collaboration_tools {
        register_collaboration_tools(tool_registry.as_ref());
        let collaboration_host = Arc::new(ApplicationCollaborationHost::new(service.clone()));
        tool_registry
            .set_collaboration_runtime(Arc::new(CollaborationRuntime::new(collaboration_host)));
    }

    let gateway: Arc<dyn Gateway> = Arc::new(ServiceGateway::new(service.clone()));
    let memory_maintenance = talos_service::MemoryDailyMaintenance::new(
        session_store_impl,
        active_turn_store,
        session_artifact_store,
        memory_service,
        llm_provider_router,
        config.clone(),
        config.automation.memory_daily_maintenance.clone(),
    );
    let automation = application_automation::start_maintenance(
        profile_store,
        config.automation.profile_usage_retention.clone(),
        memory_maintenance,
        config.automation.memory_daily_maintenance.clone(),
        talos_config::parse_timezone(&config.timezone)?,
    )
    .await?;

    Ok(TalosApplicationRuntime {
        config,
        service,
        event_service,
        auth_service,
        gateway,
        agent_dispatch_runtime,
        coordinator,
        tool_registry,
        automation,
    })
}

fn apply_runtime_options(mut config: Config, options: Option<&TalosServerOptions>) -> Config {
    let Some(options) = options else {
        return config;
    };

    if let Some(host) = &options.host {
        config.gateway.host = host.clone();
    }
    if let Some(port) = options.port {
        config.gateway.port = port;
    }
    if let Some(logging_stdout) = options.logging_stdout {
        config.logging.stdout = logging_stdout;
    }

    config
}

fn create_llm_provider(config: &Config) -> TalosResult<Arc<dyn LlmProvider>> {
    let readiness = config.model_readiness();
    if !readiness.ready {
        warn!(
            provider = readiness.provider,
            model = readiness.model,
            issues = ?readiness.issues,
            "model provider is not ready; runtime will start in setup-required mode"
        );
        return Ok(Arc::new(ModelSetupRequiredProvider {
            model: config.llm.model.clone(),
            message: format!("model setup required: {}", readiness.issues.join("; ")),
        }));
    }

    match config.llm.provider.as_str() {
        "anthropic" => {
            let api_key = config.llm.anthropic.api_key.clone().ok_or_else(|| {
                TalosError::InvalidConfiguration("llm.anthropic.api_key is required".to_string())
            })?;
            let base_url = config
                .llm
                .anthropic
                .base_url
                .clone()
                .unwrap_or_else(|| "https://api.anthropic.com/v1".to_string());
            let anthropic_config = talos_llm_anthropic::AnthropicConfig::new(&api_key)
                .with_base_url(&base_url)
                .with_default_model(&config.llm.model);
            Ok(Arc::new(AnthropicProvider::new(anthropic_config)))
        }
        "openai" => {
            let api_key = config.llm.openai.api_key.clone().ok_or_else(|| {
                TalosError::InvalidConfiguration("llm.openai.api_key is required".to_string())
            })?;
            let mut openai_config = talos_llm_openai::OpenAIConfig::new(&api_key)
                .with_default_model(&config.llm.model)
                .with_protocol(openai_protocol("openai", &config.llm.openai)?);
            if let Some(base_url) = config.llm.openai.base_url.as_deref() {
                openai_config = openai_config.with_base_url(base_url);
            }
            Ok(Arc::new(OpenAIProvider::new(openai_config)))
        }
        "bailian" => {
            let api_key = config.llm.bailian.api_key.clone().ok_or_else(|| {
                TalosError::InvalidConfiguration("llm.bailian.api_key is required".to_string())
            })?;
            let base_url = config.llm.bailian.base_url.clone().ok_or_else(|| {
                TalosError::InvalidConfiguration("llm.bailian.base_url is required".to_string())
            })?;
            let bailian_config =
                talos_llm_bailian::BailianConfig::new(&api_key).with_base_url(&base_url);
            Ok(Arc::new(BailianProvider::new(bailian_config)))
        }
        "deepseek" => {
            let api_key = config.llm.deepseek.api_key.clone().ok_or_else(|| {
                TalosError::InvalidConfiguration("llm.deepseek.api_key is required".to_string())
            })?;
            let base_url = config
                .llm
                .deepseek
                .base_url
                .clone()
                .unwrap_or_else(|| "https://api.deepseek.com".to_string());
            let deepseek_config = DeepSeekConfig::new(&api_key)
                .with_base_url(&base_url)
                .with_default_model(&config.llm.model)
                .with_protocol(deepseek_protocol(&config.llm.deepseek)?);
            Ok(Arc::new(DeepSeekProvider::new(deepseek_config)))
        }
        "ollama" => {
            let base_url = config.llm.ollama.base_url.clone().ok_or_else(|| {
                TalosError::InvalidConfiguration("llm.ollama.base_url is required".to_string())
            })?;
            let mut ollama_config = OllamaConfig::new(&base_url, &config.llm.model);
            if let Some(api_key) = config.llm.ollama.api_key.as_deref() {
                ollama_config = ollama_config.with_api_key(api_key);
            }
            Ok(Arc::new(OllamaProvider::new(ollama_config)))
        }
        "openrouter" => {
            let api_key = config.llm.openrouter.api_key.clone().ok_or_else(|| {
                TalosError::InvalidConfiguration("llm.openrouter.api_key is required".to_string())
            })?;
            let base_url = config
                .llm
                .openrouter
                .base_url
                .clone()
                .unwrap_or_else(|| "https://openrouter.ai/api/v1".to_string());
            let openrouter_config = OpenRouterConfig::new(&api_key)
                .with_base_url(&base_url)
                .with_default_model(&config.llm.model)
                .with_protocol(openrouter_protocol(&config.llm.openrouter)?);
            Ok(Arc::new(OpenRouterProvider::new(openrouter_config)))
        }
        _ => Err(TalosError::InvalidConfiguration(format!(
            "unsupported provider '{}'",
            config.llm.provider
        ))),
    }
}

fn openai_protocol(
    provider: &str,
    provider_config: &ProviderConfig,
) -> TalosResult<OpenAIProtocol> {
    match effective_llm_protocol(provider, provider_config).ok_or_else(|| {
        TalosError::InvalidConfiguration(format!("llm.{provider}.protocol is required"))
    })? {
        LlmProtocol::Chat => Ok(OpenAIProtocol::Chat),
        LlmProtocol::Responses => Ok(OpenAIProtocol::Responses),
        protocol => Err(TalosError::InvalidConfiguration(format!(
            "llm.{provider}.protocol does not support {protocol}"
        ))),
    }
}

fn openrouter_protocol(provider_config: &ProviderConfig) -> TalosResult<OpenRouterProtocol> {
    match effective_llm_protocol("openrouter", provider_config).ok_or_else(|| {
        TalosError::InvalidConfiguration("llm.openrouter.protocol is required".to_string())
    })? {
        LlmProtocol::Chat => Ok(OpenRouterProtocol::Chat),
        LlmProtocol::Responses => Ok(OpenRouterProtocol::Responses),
        LlmProtocol::Messages => Ok(OpenRouterProtocol::Messages),
    }
}

fn deepseek_protocol(provider_config: &ProviderConfig) -> TalosResult<DeepSeekProtocol> {
    match effective_llm_protocol("deepseek", provider_config).ok_or_else(|| {
        TalosError::InvalidConfiguration("llm.deepseek.protocol is required".to_string())
    })? {
        LlmProtocol::Chat => Ok(DeepSeekProtocol::Chat),
        LlmProtocol::Messages => Ok(DeepSeekProtocol::Messages),
        LlmProtocol::Responses => Err(TalosError::InvalidConfiguration(
            "llm.deepseek.protocol does not support responses".to_string(),
        )),
    }
}

struct ModelSetupRequiredProvider {
    model: String,
    message: String,
}

#[async_trait::async_trait]
impl LlmProvider for ModelSetupRequiredProvider {
    fn name(&self) -> &str {
        "setup-required"
    }

    async fn chat(&self, _request: ChatRequest) -> TalosResult<ChatResponse> {
        Err(TalosError::InvalidConfiguration(self.message.clone()))
    }

    async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
        Err(TalosError::InvalidConfiguration(self.message.clone()))
    }

    fn default_model(&self) -> String {
        self.model.clone()
    }

    async fn stream_model_blocks(
        &self,
        _request: ChatRequest,
    ) -> TalosResult<talos_llm_provider::ModelBlockStream> {
        Err(TalosError::InvalidConfiguration(self.message.clone()))
    }
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
