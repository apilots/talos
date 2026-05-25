use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use talos_agent::{AgentHarness, AgentOrchestrator};
use talos_app_server::BackgroundTaskDrain;
use talos_application_service::TalosApplicationService;
use talos_auth::{AuthService, CloudAuthService, UserService};
use talos_channel::{ChannelServer, TelegramChannel};
use talos_config::{Config, LlmConfigManager, LlmConfigService, TalosPaths, YamlLlmConfigStore};
use talos_coordinator::{Coordinator, CoordinatorImpl};
use talos_error::{TalosError, TalosResult};
use talos_gateway::{Gateway, GatewayImpl};
use talos_gateway_http::{HttpServer, ServerLifecycle, ServerLifecycleState};
use talos_llm_anthropic::AnthropicProvider;
use talos_llm_bailian::BailianProvider;
use talos_llm_openai::OpenAIProvider;
use talos_llm_provider::{
    ChatRequest, ChatResponse, ChatStream, LlmProvider, NativeThinkingSupport, PromptCacheSupport,
};
use talos_logging::LogGuard;
use talos_router::{Binding, MatchRule, Router, RouterImpl};
use talos_session_store::{
    ActiveTurnStore, DatabaseSessionStore, MessageIdempotencyStore, SessionStore,
    TimelineEventRetentionConfig, TimelineEventService, TimelineEventStore,
};
use talos_storage::{create_storage, StorageConfig};
use talos_tools::{
    register_collaboration_tools, register_core_local_tools, register_network_tools,
    CollaborationRuntime, ToolRegistry,
};
use talos_types::{resolve_model_catalog, RequestHandler, SessionRuntime};
use tokio::signal;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

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
    pub application_service: Arc<TalosApplicationService>,
    pub event_service: Arc<TimelineEventService>,
    pub auth_service: Arc<dyn AuthService>,
    pub request_handler: Arc<dyn RequestHandler>,
    pub coordinator: Arc<CoordinatorImpl>,
    pub tool_registry: Arc<ToolRegistry>,
}

const MEMORY_FLUSH_DRAIN_TIMEOUT: Duration = Duration::from_secs(60);
const SERVER_SHUTDOWN_WAIT_TIMEOUT: Duration = Duration::from_secs(180);

pub async fn run_app_server() -> TalosResult<()> {
    let config_source = resolve_default_config_path();
    let config = load_runtime_config()?;
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
    let config_source = resolve_default_config_path();
    let config = load_runtime_config()?;
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

fn resolve_default_config_path() -> Option<PathBuf> {
    let config_path = Config::default_path();
    config_path
        .exists()
        .then(|| canonical_or_original(config_path))
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
        runtime.application_service.clone(),
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
        let mut channel_server =
            ChannelServer::new(Arc::new(config.clone()), runtime.request_handler.clone());
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
    let app = Arc::new(
        talos_app_server::TalosApplicationAdapter::new(runtime.application_service, replay_limit)
            .with_drain_provider(Box::new(CoordinatorDrain {
                coordinator: runtime.coordinator,
            }))
            .with_tool_registry(runtime.tool_registry),
    );
    let local_launch_access = if config.gateway.auth {
        talos_app_server::LocalLaunchAccess::Regular
    } else {
        talos_app_server::LocalLaunchAccess::Full
    };
    let validator = Arc::new(
        talos_app_server::AuthServiceCredentialValidator::new(
            runtime.auth_service,
            std::env::var("TALOS_LOCAL_LAUNCH_TOKEN").ok(),
        )
        .with_local_launch_access(local_launch_access),
    );
    talos_app_server::run_stdio(
        app,
        validator,
        talos_app_server::AppServerConfig {
            auth_enabled: config.gateway.auth,
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
    let session_store: Arc<dyn SessionStore> = session_store_impl.clone();
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

    let agent = AgentHarness::new(
        "default".to_string(),
        config.agent.name.clone(),
        "Default AI assistant".to_string(),
        llm_provider.clone(),
        config.llm.model.clone(),
        tool_registry.clone(),
        config.agent.kernel.clone(),
    );
    let agent: Arc<dyn AgentOrchestrator> = Arc::new(agent);

    let coordinator_builder = CoordinatorImpl::new(router, session_store.clone(), config.clone())
        .with_channel_dm_scope("telegram", config.channels.telegram.dm_scope.to_string())
        .with_default_workspace_id(config.agent.kernel.default_workspace_id.clone());
    let coordinator = Arc::new(coordinator_builder);

    if config.agent.kernel.tools.enable_collaboration_tools {
        register_collaboration_tools(tool_registry.as_ref());
        tool_registry.set_collaboration_runtime(Arc::new(CollaborationRuntime::new(
            coordinator.clone(),
            coordinator.clone(),
        )));
    }

    coordinator.register_agent("default", agent).await?;

    let request_handler: Arc<dyn RequestHandler> = coordinator.clone();
    let session_runtime: Arc<dyn SessionRuntime> = coordinator.clone();
    let gateway: Arc<dyn Gateway> = Arc::new(GatewayImpl::new(request_handler.clone()));
    let cloud_auth = Arc::new(CloudAuthService::new(config.cloud.api_base_url.clone()));
    let auth_service: Arc<dyn AuthService> = cloud_auth.clone();
    let user_service: Arc<dyn UserService> = cloud_auth;

    let config_path = TalosPaths::resolve().config_file;
    let llm_store = Arc::new(YamlLlmConfigStore::new(config_path));
    let llm_config_service: Arc<dyn LlmConfigService> = Arc::new(LlmConfigManager::new(llm_store));

    let model_catalog = resolve_model_catalog(&config.llm);
    let provider_reloader = reloadable_llm_provider.clone();
    let coordinator_reloader = coordinator.clone();
    let runtime_reload_hook: talos_application_service::RuntimeConfigReloadHook =
        Arc::new(move |_| {
            let reloaded_config = Config::load()?;
            reloaded_config.validate_runtime()?;
            let reloaded_provider = create_llm_provider(&reloaded_config)?;
            coordinator_reloader.update_base_llm(&reloaded_config.llm);
            provider_reloader.reload(reloaded_provider);
            Ok(())
        });

    let application_service = Arc::new(TalosApplicationService::new_with_reload_hook(
        gateway,
        session_runtime,
        session_store,
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

    Ok(TalosApplicationRuntime {
        config,
        application_service,
        event_service,
        auth_service,
        request_handler,
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
            let mut openai_config = talos_llm_openai::OpenAIConfig::new(&api_key);
            if let Some(base_url) = &config.llm.openai.base_url {
                openai_config = openai_config.with_base_url(base_url);
            }
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
            let openai_config =
                talos_llm_openai::OpenAIConfig::new("ollama").with_base_url(&base_url);
            Ok(Arc::new(OpenAIProvider::new(openai_config)))
        }
        _ => Err(TalosError::InvalidConfiguration(format!(
            "unsupported or disabled provider '{}'",
            config.llm.provider
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

    fn native_thinking_support(&self, model: &str) -> NativeThinkingSupport {
        self.current().native_thinking_support(model)
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
}

#[cfg(test)]
mod tests {
    use super::{
        apply_runtime_options, build_application_runtime, ReloadableLlmProvider, TalosServerHandle,
        TalosServerOptions, SERVER_SHUTDOWN_WAIT_TIMEOUT,
    };
    use std::sync::Arc;
    use talos_config::Config;
    use talos_gateway_http::{ServerLifecycle, ServerLifecycleState};
    use talos_llm_provider::{ChatRequest, ChatResponse, ChatStream, LlmProvider, Usage};
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
