use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use talos_agent::{Agent, AgentHarness, SkillRootsResolver};
use talos_app_server::BackgroundTaskDrain;
use talos_auth::{AuthService, CloudAuthService, OpenModeUserService, UserService};
use talos_channel::{ChannelServer, TelegramChannel};
use talos_config::{
    effective_llm_protocol, AgentKernelConfig, Config, LlmConfigManager, LlmConfigService,
    LlmProtocol, McpTransportConfig, ProviderConfig, TalosPaths, YamlLlmConfigStore,
};
use talos_coordinator::{Coordinator, CoordinatorImpl, SessionRuntime};
use talos_error::{TalosError, TalosResult};
use talos_gateway::{Gateway, ServiceGateway};
use talos_gateway_http::{HttpServer, ServerLifecycle, ServerLifecycleState};
use talos_llm_anthropic::AnthropicProvider;
use talos_llm_bailian::BailianProvider;
use talos_llm_openai::{OpenAIProtocol, OpenAIProvider};
use talos_llm_provider::{
    ChatRequest, ChatResponse, ChatStream, LlmProvider, PromptCacheSupport, ReasoningSupport,
};
use talos_logging::LogGuard;
use talos_router::{Binding, MatchRule, Router, RouterImpl};
use talos_service::{
    Actor, AgentDispatchTurnExecutor, PluginManagerService, TalosService, TimelineSubscribeCommand,
    TimelineSubscriptionMessage,
};
use talos_service::{TimelineEventRetentionConfig, TimelineEventService};
use talos_storage::{create_storage, StorageConfig};
use talos_store::project::{ProjectStore, StorageProjectStore};
use talos_store::session::{
    ActiveTurnStore, DatabaseSessionStore, MessageIdempotencyStore, SessionStore,
    TimelineEventStore,
};
use talos_tools::{
    register_collaboration_tools, register_core_local_tools, register_network_tools,
    CollaborationAgentHost, CollaborationRuntime, McpConnectionManager, ToolRegistry,
};
use talos_types::talos::{
    TalosInputItem, TalosTimelineContent, TalosTimelineDelta, TalosTimelineEventPayload,
};
use talos_types::{
    resolve_model_catalog, AgentDispatchResponse, AgentDispatchRuntime, McpServerDefinition,
    McpServerRuntimeDescriptor, McpServerSource, McpTransportDefinition, StartTurnRequest,
    TalosContext, TalosSession,
};
use tokio::signal;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

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
            .access
            .get_or_create_session(&actor, &session_id, context, None, None, None)
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
    ) -> TalosResult<AgentDispatchResponse> {
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
            model: None,
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
                TalosTimelineEventPayload::ItemDelta(delta) => {
                    if let TalosTimelineDelta::Text { text } = delta.delta {
                        output.push_str(&text);
                    }
                }
                TalosTimelineEventPayload::ItemCompleted(completed) => {
                    if let Some(TalosTimelineContent::Text { text }) = completed.final_content {
                        output = text;
                    }
                }
                TalosTimelineEventPayload::TurnCompleted(completed) => {
                    return Ok(AgentDispatchResponse {
                        session_id: envelope.session_id,
                        content: output,
                        stop_reason: completed.stop_reason,
                        usage: completed.usage,
                        success: true,
                        model,
                        output_items: Vec::new(),
                        reasoning: Vec::new(),
                    });
                }
                TalosTimelineEventPayload::TurnError(error) => {
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

const MEMORY_FLUSH_DRAIN_TIMEOUT: Duration = Duration::from_secs(60);
const SERVER_SHUTDOWN_WAIT_TIMEOUT: Duration = Duration::from_secs(180);

pub async fn run_app_server() -> TalosResult<()> {
    let config = load_runtime_config()?;
    let config_source = resolve_config_source_path();
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
    let config_source = resolve_config_source_path();
    let server_options = TalosServerOptions {
        webui_dir: resolve_packaged_webui_dir(),
        ..TalosServerOptions::default()
    };
    let _log_guard = init_logging(&config, Some(&server_options))?;
    log_config_source(config_source);
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

fn resolve_config_source_path() -> Option<PathBuf> {
    let config_path = Config::source_path();
    config_path.exists().then(|| canonical_or_original(config_path))
}

fn resolve_packaged_webui_dir() -> Option<PathBuf> {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(PathBuf::from))?;
    let webui_dir = exe_dir.join("webui");
    webui_dir.join("index.html").exists().then_some(webui_dir)
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
        } = self;

        let signal_shutdown = shutdown_token.clone();
        let signal_task = tokio::spawn(async move {
            shutdown_signal().await;
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

        perform_shutdown(&coordinator, &lifecycle).await;

        result
    }
}

async fn perform_shutdown(coordinator: &Arc<CoordinatorImpl>, lifecycle: &Arc<ServerLifecycle>) {
    if lifecycle.current() == ServerLifecycleState::Stopped {
        return;
    }

    if !lifecycle.current().is_quiescing_or_later() {
        lifecycle.store(ServerLifecycleState::Quiescing);
    }
    lifecycle.store(ServerLifecycleState::Draining);

    let pending_flushes = coordinator.pending_memory_flushes().await;
    info!(
        pending_flushes,
        timeout_secs = MEMORY_FLUSH_DRAIN_TIMEOUT.as_secs(),
        "draining background tasks before shutdown"
    );
    if let Err(err) = coordinator
        .drain_background_tasks(MEMORY_FLUSH_DRAIN_TIMEOUT)
        .await
    {
        warn!(error = %err, "background task drain failed on shutdown");
    }

    lifecycle.store(ServerLifecycleState::Stopped);
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
        http_server,
        channel_server,
    })
}

pub async fn run_json_rpc_stdio(config: Config) -> TalosResult<()> {
    let runtime = build_application_runtime(config.clone()).await?;
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
    talos_app_server::run_stdio(
        app,
        validator,
        talos_app_server::AppServerConfig {
            auth_enabled: config.gateway.auth,
            cloud_auth_url: Some(config.cloud.web_base_url.clone()),
            ..talos_app_server::AppServerConfig::default()
        },
    )
    .await
    .map_err(|error| TalosError::InternalError(error.message))
}

pub async fn build_application_runtime(config: Config) -> TalosResult<TalosApplicationRuntime> {
    let storage_impl = create_storage(&StorageConfig {
        database_type: config.storage.database_type.clone(),
        connection_string: config.storage.connection_string.clone(),
        max_connections: config.storage.max_connections,
    })
    .await?;
    let storage: Arc<dyn talos_storage::Storage> = Arc::from(storage_impl);
    let session_store_impl = Arc::new(DatabaseSessionStore::new(storage.clone()).await?);
    let project_store_impl = Arc::new(StorageProjectStore::new(storage.clone()).await?);
    let session_store: Arc<dyn SessionStore> = session_store_impl.clone();
    let project_store: Arc<dyn ProjectStore> = project_store_impl.clone();
    let event_store: Arc<dyn TimelineEventStore> = session_store_impl.clone();
    let active_turn_store: Arc<dyn ActiveTurnStore> = session_store_impl.clone();
    let idempotency_store: Arc<dyn MessageIdempotencyStore> = session_store_impl.clone();
    let event_service = Arc::new(TimelineEventService::new(
        event_store,
        active_turn_store,
        TimelineEventRetentionConfig {
            retention_hours: config.agent.kernel.timeline_events.retention_hours,
            max_events_per_session: config.agent.kernel.timeline_events.max_events_per_session,
            gc_interval_minutes: config.agent.kernel.timeline_events.gc_interval_minutes,
            gc_batch_size: config.agent.kernel.timeline_events.gc_batch_size,
        },
    ));
    let router: Arc<dyn Router> = Arc::new(RouterImpl::new());
    if let Err(err) = router
        .add_binding(Binding {
            id: "default_binding".to_string(),
            agent_id: "default".to_string(),
            rule: MatchRule::Default,
            priority: 1,
        })
        .await
    {
        error!("failed to create default routing: {}", err);
    }

    let initial_llm_provider = create_llm_provider(&config)?;
    let reloadable_llm_provider = Arc::new(ReloadableLlmProvider::new(initial_llm_provider));
    let llm_provider: Arc<dyn LlmProvider> = reloadable_llm_provider.clone();
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

    let agent = AgentHarness::new_with_skill_roots_resolver(
        "default".to_string(),
        config.agent.name.clone(),
        "Default AI assistant".to_string(),
        llm_provider.clone(),
        config.llm.model.clone(),
        tool_registry.clone(),
        config.agent.kernel.clone(),
        skill_roots_resolver,
    );
    let agent: Arc<dyn Agent> = Arc::new(agent);

    let coordinator = Arc::new(CoordinatorImpl::new(router));

    coordinator.register_agent("default", agent).await?;

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

    let config_path = TalosPaths::resolve().config_file;
    let llm_store = Arc::new(YamlLlmConfigStore::new(config_path));
    let llm_config_service: Arc<dyn LlmConfigService> = Arc::new(LlmConfigManager::new(llm_store));

    let model_catalog = resolve_model_catalog(&config.llm);
    let provider_reloader = reloadable_llm_provider.clone();
    let runtime_reload_hook: talos_service::RuntimeConfigReloadHook = Arc::new(move |_| {
        let reloaded_config = Config::load()?;
        reloaded_config.validate_runtime()?;
        let reloaded_provider = create_llm_provider(&reloaded_config)?;
        provider_reloader.reload(reloaded_provider);
        Ok(())
    });

    let service = Arc::new(TalosService::new_with_reload_hook(
        turn_executor,
        session_runtime,
        session_store,
        project_store,
        event_service.clone(),
        idempotency_store,
        config.clone(),
        llm_provider.clone(),
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

    Ok(TalosApplicationRuntime {
        config,
        service,
        event_service,
        auth_service,
        gateway,
        agent_dispatch_runtime,
        coordinator,
        tool_registry,
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

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
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
        "anthropic" if config.llm.anthropic.enabled => {
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
        "openai" if config.llm.openai.enabled => {
            let api_key = config.llm.openai.api_key.clone().ok_or_else(|| {
                TalosError::InvalidConfiguration("llm.openai.api_key is required".to_string())
            })?;
            let openai_config = openai_config(&api_key, config.llm.openai.base_url.as_deref())
                .with_protocol(openai_protocol("openai", &config.llm.openai)?);
            Ok(Arc::new(OpenAIProvider::new(openai_config)))
        }
        "bailian" if config.llm.bailian.enabled => {
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
        "ollama" if config.llm.ollama.enabled => {
            let base_url = config.llm.ollama.base_url.clone().ok_or_else(|| {
                TalosError::InvalidConfiguration("llm.ollama.base_url is required".to_string())
            })?;
            let openai_config = openai_compatible_config(
                "ollama",
                "Ollama",
                "ollama",
                &base_url,
                &config.llm.model,
                &config.llm.ollama,
            )?;
            Ok(Arc::new(OpenAIProvider::new(openai_config)))
        }
        "openrouter" if config.llm.openrouter.enabled => {
            let api_key = config.llm.openrouter.api_key.clone().ok_or_else(|| {
                TalosError::InvalidConfiguration("llm.openrouter.api_key is required".to_string())
            })?;
            let base_url = config
                .llm
                .openrouter
                .base_url
                .clone()
                .unwrap_or_else(|| "https://openrouter.ai/api/v1".to_string());
            let openai_config = openai_compatible_config(
                "openrouter",
                "OpenRouter",
                &api_key,
                &base_url,
                &config.llm.model,
                &config.llm.openrouter,
            )?;
            Ok(Arc::new(OpenAIProvider::new(openai_config)))
        }
        _ => Err(TalosError::InvalidConfiguration(format!(
            "unsupported or disabled provider '{}'",
            config.llm.provider
        ))),
    }
}

fn openai_config(api_key: &str, base_url: Option<&str>) -> talos_llm_openai::OpenAIConfig {
    let mut config = talos_llm_openai::OpenAIConfig::new(api_key);
    if let Some(base_url) = base_url {
        config = config.with_base_url(base_url);
    }
    config
}

fn openai_compatible_config(
    provider: &str,
    display_name: &str,
    api_key: &str,
    base_url: &str,
    default_model: &str,
    provider_config: &ProviderConfig,
) -> TalosResult<talos_llm_openai::OpenAIConfig> {
    Ok(talos_llm_openai::OpenAIConfig::new(api_key)
        .with_base_url(base_url)
        .with_identity(provider, display_name, default_model)
        .with_protocol(openai_protocol(provider, provider_config)?))
}

fn openai_protocol(
    provider: &str,
    provider_config: &ProviderConfig,
) -> TalosResult<OpenAIProtocol> {
    match effective_llm_protocol(provider, provider_config).unwrap_or(LlmProtocol::Chat) {
        LlmProtocol::Chat => Ok(OpenAIProtocol::Chat),
        LlmProtocol::Responses if provider == "openai" => Ok(OpenAIProtocol::Responses),
        protocol => Err(TalosError::InvalidConfiguration(format!(
            "llm.{provider}.protocol does not support {protocol}"
        ))),
    }
}

struct ReloadableLlmProvider {
    current: RwLock<Arc<dyn LlmProvider>>,
}

impl ReloadableLlmProvider {
    fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self {
            current: RwLock::new(provider),
        }
    }

    fn reload(&self, provider: Arc<dyn LlmProvider>) {
        *self.current.write().expect("llm provider write lock") = provider;
    }

    fn current(&self) -> Arc<dyn LlmProvider> {
        self.current.read().expect("llm provider read lock").clone()
    }
}

#[async_trait::async_trait]
impl LlmProvider for ReloadableLlmProvider {
    fn name(&self) -> &str {
        "reloadable"
    }

    async fn chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        self.current().chat(request).await
    }

    async fn stream(&self, request: ChatRequest) -> TalosResult<ChatStream> {
        self.current().stream(request).await
    }

    fn default_model(&self) -> String {
        self.current().default_model()
    }

    fn reasoning_support(&self, model: &str) -> ReasoningSupport {
        self.current().reasoning_support(model)
    }

    fn prompt_cache_support(&self) -> PromptCacheSupport {
        self.current().prompt_cache_support()
    }

    async fn chat_with_tools(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        self.current().chat_with_tools(request).await
    }

    async fn stream_model_blocks(
        &self,
        request: ChatRequest,
    ) -> TalosResult<talos_llm_provider::ModelBlockStream> {
        self.current().stream_model_blocks(request).await
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
mod tests {
    use super::{
        apply_runtime_options, build_application_runtime, create_llm_provider,
        ModelSetupRequiredProvider, ReloadableLlmProvider, TalosServerHandle, TalosServerOptions,
        SERVER_SHUTDOWN_WAIT_TIMEOUT,
    };
    use std::sync::Arc;
    use talos_config::{Config, LlmProtocol};
    use talos_gateway_http::{ServerLifecycle, ServerLifecycleState};
    use talos_llm_provider::{ChatRequest, ChatResponse, ChatStream, LlmProvider, Usage};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::sync::oneshot;
    use tokio::time::Duration;
    use tokio_util::sync::CancellationToken;

    #[test]
    fn apply_runtime_options_updates_gateway_and_logging() {
        let config = Config::default();
        let options = TalosServerOptions {
            host: Some("127.0.0.1".to_string()),
            port: Some(18080),
            logging_stdout: Some(false),
            auth_disabled: None,
            enable_channels: None,
            webui_dir: None,
        };

        let updated = apply_runtime_options(config, Some(&options));

        assert_eq!(updated.gateway.host, "127.0.0.1");
        assert_eq!(updated.gateway.port, 18080);
        assert!(!updated.logging.stdout);
    }

    #[tokio::test]
    async fn application_runtime_builds_when_model_key_is_missing() {
        let mut config = Config::default();
        config.gateway.auth = false;
        config.storage.connection_string = ":memory:".to_string();
        config.llm.anthropic.api_key = None;

        let runtime = build_application_runtime(config)
            .await
            .expect("runtime should start before model setup is complete");

        assert_eq!(runtime.config.llm.provider, "anthropic");
    }

    #[tokio::test]
    async fn setup_required_provider_reports_configuration_error_for_model_blocks() {
        let provider = ModelSetupRequiredProvider {
            model: "missing-model".to_string(),
            message: "model setup required: missing key".to_string(),
        };

        let result = provider
            .stream_model_blocks(ChatRequest::new_user("hello", "missing-model"))
            .await;
        let err = match result {
            Ok(_) => panic!("setup-required provider should fail"),
            Err(err) => err,
        };

        assert!(format!("{err}").contains("model setup required: missing key"));
    }

    #[tokio::test]
    async fn openrouter_provider_uses_chat_completions_path_and_identity() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("test server should bind");
        let addr = listener
            .local_addr()
            .expect("test server addr should exist");
        let (path_tx, path_rx) = oneshot::channel();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("request should arrive");
            let mut buf = vec![0; 4096];
            let n = socket.read(&mut buf).await.expect("request should read");
            let request = String::from_utf8_lossy(&buf[..n]);
            let path = request
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .unwrap_or("")
                .to_string();
            let _ = path_tx.send(path);
            let body = r#"{"model":"test-model","choices":[{"message":{"content":"ok"},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            socket
                .write_all(response.as_bytes())
                .await
                .expect("response should write");
        });

        let mut config = Config::default();
        config.llm.provider = "openrouter".to_string();
        config.llm.model = "test-model".to_string();
        config.llm.openrouter.enabled = true;
        config.llm.openrouter.api_key = Some("test-key".to_string());
        config.llm.openrouter.base_url = Some(format!("http://{addr}"));

        let provider = create_llm_provider(&config).expect("provider should be created");
        assert_eq!(provider.name(), "openrouter");
        assert_eq!(provider.default_model(), "test-model");

        let response = provider
            .chat(ChatRequest::new_user("hello", "test-model"))
            .await
            .expect("chat should succeed");
        let path = path_rx.await.expect("path should be captured");

        assert_eq!(path, "/chat/completions");
        assert_eq!(response.content, "ok");
    }

    #[tokio::test]
    async fn openai_provider_uses_responses_path_when_protocol_selected() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("test server should bind");
        let addr = listener
            .local_addr()
            .expect("test server addr should exist");
        let (path_tx, path_rx) = oneshot::channel();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("request should arrive");
            let mut buf = vec![0; 4096];
            let n = socket.read(&mut buf).await.expect("request should read");
            let request = String::from_utf8_lossy(&buf[..n]);
            let path = request
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .unwrap_or("")
                .to_string();
            let _ = path_tx.send(path);
            let body = r#"{"id":"resp_1","model":"gpt-test","status":"completed","output_text":"ok","output":[],"usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            socket
                .write_all(response.as_bytes())
                .await
                .expect("response should write");
        });

        let mut config = Config::default();
        config.llm.provider = "openai".to_string();
        config.llm.model = "gpt-test".to_string();
        config.llm.openai.enabled = true;
        config.llm.openai.api_key = Some("test-key".to_string());
        config.llm.openai.base_url = Some(format!("http://{addr}"));
        config.llm.openai.protocol = Some(LlmProtocol::Responses);

        let provider = create_llm_provider(&config).expect("provider should be created");
        assert_eq!(provider.name(), "openai");

        let response = provider
            .chat(ChatRequest::new_user("hello", "gpt-test"))
            .await
            .expect("chat should succeed");
        let path = path_rx.await.expect("path should be captured");

        assert_eq!(path, "/responses");
        assert_eq!(response.content, "ok");
    }

    #[tokio::test]
    async fn reloadable_llm_provider_uses_reloaded_provider() {
        let provider = ReloadableLlmProvider::new(Arc::new(TestLlmProvider {
            text: "before".to_string(),
            model: "model-a".to_string(),
        }));

        let first = provider
            .chat(ChatRequest::new_user("hello", "model-a"))
            .await
            .expect("first provider should respond");
        provider.reload(Arc::new(TestLlmProvider {
            text: "after".to_string(),
            model: "model-b".to_string(),
        }));
        let second = provider
            .chat(ChatRequest::new_user("hello", "model-a"))
            .await
            .expect("reloaded provider should respond");

        assert_eq!(first.content, "before");
        assert_eq!(second.content, "after");
        assert_eq!(provider.default_model(), "model-b");
    }

    struct TestLlmProvider {
        text: String,
        model: String,
    }

    #[async_trait::async_trait]
    impl LlmProvider for TestLlmProvider {
        fn name(&self) -> &str {
            "test"
        }

        async fn chat(&self, request: ChatRequest) -> talos_error::TalosResult<ChatResponse> {
            Ok(ChatResponse {
                content: self.text.clone(),
                reasoning: Vec::new(),
                model: request.model,
                usage: Usage::default(),
                finish_reason: Some("stop".to_string()),
                tool_calls: None,
            })
        }

        async fn stream(&self, _request: ChatRequest) -> talos_error::TalosResult<ChatStream> {
            Err(talos_error::TalosError::InternalError(
                "stream is not used in this test".to_string(),
            ))
        }

        fn default_model(&self) -> String {
            self.model.clone()
        }
    }

    #[tokio::test]
    async fn handle_shutdown_waits_for_stopped_state() {
        let lifecycle = Arc::new(ServerLifecycle::new());
        let handle = TalosServerHandle {
            shutdown_token: CancellationToken::new(),
            lifecycle: lifecycle.clone(),
            wait_timeout: Duration::from_secs(1),
        };

        let token = handle.shutdown_token.clone();
        let lifecycle_for_task = lifecycle.clone();
        let waiter = tokio::spawn(async move { handle.shutdown().await });
        token.cancelled().await;
        lifecycle_for_task.store(ServerLifecycleState::Quiescing);
        lifecycle_for_task.store(ServerLifecycleState::Draining);
        lifecycle_for_task.store(ServerLifecycleState::Stopped);

        waiter
            .await
            .expect("task should join")
            .expect("shutdown should finish");
    }

    #[tokio::test]
    async fn handle_shutdown_times_out_without_stopped_state() {
        let lifecycle = Arc::new(ServerLifecycle::new());
        let handle = TalosServerHandle {
            shutdown_token: CancellationToken::new(),
            lifecycle,
            wait_timeout: Duration::from_millis(50),
        };

        let err = handle
            .shutdown()
            .await
            .expect_err("shutdown should time out");
        assert!(matches!(err, talos_error::TalosError::InternalError(_)));
    }

    #[tokio::test]
    async fn lifecycle_watch_reports_expected_state_order() {
        let lifecycle = Arc::new(ServerLifecycle::new());
        let mut updates = lifecycle.subscribe();

        lifecycle.store(ServerLifecycleState::Quiescing);
        updates
            .changed()
            .await
            .expect("quiescing update should arrive");
        assert_eq!(
            *updates.borrow_and_update(),
            ServerLifecycleState::Quiescing
        );

        lifecycle.store(ServerLifecycleState::Draining);
        updates
            .changed()
            .await
            .expect("draining update should arrive");
        assert_eq!(*updates.borrow_and_update(), ServerLifecycleState::Draining);

        lifecycle.store(ServerLifecycleState::Stopped);
        updates
            .changed()
            .await
            .expect("stopped update should arrive");
        assert_eq!(*updates.borrow_and_update(), ServerLifecycleState::Stopped);

        assert!(SERVER_SHUTDOWN_WAIT_TIMEOUT > Duration::from_secs(0));
    }
}
