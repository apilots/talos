use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use talos_app_server::BackgroundTaskDrain;
use talos_auth::AuthService;
use talos_automation::Automation;
use talos_channel::{ChannelServer, TelegramChannel};
use talos_config::Config;
use talos_coordinator::{AgentCoordinator, CoordinatorImpl};
use talos_error::{TalosError, TalosResult};
use talos_gateway::Gateway;
use talos_gateway_http::{HttpServer, ServerLifecycle, ServerLifecycleState};
use talos_logging::LogGuard;
use talos_service::{AgentNodeService, PluginManagerService, TalosService, TimelineEventService};
use talos_tools::{AgentControl, AgentControlFuture, ToolRegistry};
use talos_types::{
    AgentTreeSnapshot, FollowUpSubagentRequest, InterruptSubagentRequest, ListSubagentsRequest,
    SendMessageResult, SendSubagentMessageRequest, SpawnSubagentRequest, SubagentSnapshot,
    SubagentSpawnResult, WaitSubagentRequest,
};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

const TALOS_WEBUI_DIR_ENV: &str = "TALOS_WEBUI_DIR";

mod agent_node_rpc;
mod local_resource_rpc;
pub use agent_node_rpc::{NodeRpcServer, NodeSessionDataRpc, NodeToolImageRpc};
mod application_automation;
mod application_runtime;
mod application_storage;
mod application_sync;
mod application_sync_background;
mod application_sync_computers;
mod application_sync_deletion;
mod application_sync_files;
mod application_sync_management;
mod application_sync_metrics;
mod application_sync_observer;
#[cfg(all(test, feature = "e2e"))]
#[path = "application_sync_tests.rs"]
mod application_sync_tests;
mod desktop_application;
mod desktop_media;
mod desktop_worker;
mod dream_memory_maintenance;
mod memory_progress;
#[cfg(test)]
#[path = "application_sync_server_tests.rs"]
mod server_registration_tests;
mod startup;
mod tool_images;

#[cfg(test)]
pub(crate) use application_runtime::build_application_runtime_with_migration_dir;
#[cfg(test)]
use talos_worker::create_llm_provider;

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
    sync_host: Arc<application_sync::ApplicationSyncHost>,
    http_server: HttpServer,
    channel_server: Option<ChannelServer>,
    node_rpc_server: Option<agent_node_rpc::NodeRpcServer>,
    coordinator: Arc<CoordinatorImpl>,
    gateway_url: String,
    shutdown_token: CancellationToken,
    lifecycle: Arc<ServerLifecycle>,
    automation: Automation,
    plugins: PluginManagerService,
    browser: Arc<talos_browser::BrowserRuntime>,
    worker_rpc: Arc<talos_worker::WorkerRpcHost>,
    resource_rpc: Arc<local_resource_rpc::LocalResourceRpc>,
    worker_registration: Arc<tokio::sync::Mutex<Option<talos_worker::RegisteredWorker>>>,
}

/// Owns the resources created for an in-process app-server runtime.
///
/// This is the product-level embedded entrypoint result used by local clients such as the TUI.
/// Callers must keep the `log_guard` alive for the lifetime of the embedded runtime.
pub struct EmbeddedAppServer {
    pub log_guard: LogGuard,
    pub runtime: TalosApplicationRuntime,
}

#[derive(Clone)]
pub struct TalosApplicationRuntime {
    pub(crate) sync_host: Arc<application_sync::ApplicationSyncHost>,
    pub(crate) sync: Arc<talos_sync::TalosSync>,
    pub(crate) worker: talos_worker::TalosWorker,
    pub(crate) worker_rpc: Arc<talos_worker::WorkerRpcHost>,
    pub(crate) resource_rpc: Arc<local_resource_rpc::LocalResourceRpc>,
    pub(crate) worker_registration: Arc<tokio::sync::Mutex<Option<talos_worker::RegisteredWorker>>>,
    pub(crate) worker_failures: tokio::sync::watch::Sender<Option<talos_error::ApiError>>,
    pub config: Config,
    pub service: Arc<TalosService>,
    pub event_service: Arc<TimelineEventService>,
    pub auth_service: Arc<dyn AuthService>,
    pub gateway: Arc<dyn Gateway>,
    pub coordinator: Arc<CoordinatorImpl>,
    pub node_service: AgentNodeService,
    pub tool_registry: Arc<ToolRegistry>,
    pub(crate) session_store: Arc<dyn talos_store::session::SessionStore>,
    pub(crate) session_history_reader: Arc<dyn talos_agent::SessionHistoryReader>,
    pub(crate) attachment_reader: Arc<dyn talos_agent::AttachmentReader>,
    pub(crate) memory_progress_store:
        Arc<dyn talos_agent_provider_kernel::KernelMemoryProgressStore>,
    pub(crate) agent_control: Arc<dyn talos_tools::AgentControl>,
    pub(crate) tool_image_store: Arc<dyn talos_tools::ToolImageStore>,
    automation: Automation,
}

impl TalosApplicationRuntime {
    /// Stable management identity shared with authenticated Connect and sync.
    pub fn sync_peer_id(&self) -> &str {
        self.sync.peer_id()
    }

    /// Compose the original product resource owners for one selected Computer.
    /// Use only with the authenticated coordinator SDK, which verifies the live
    /// invocation before dispatch. This handler still checks persisted Session
    /// ownership and binding; it neither imports data nor owns network reconnect.
    pub fn coordinator_resource_handler(
        &self,
        computer_id: String,
    ) -> Arc<dyn agent_client::AgentResourceRpcHandler> {
        Arc::new(agent_node_rpc::CoordinatorResources {
            service: self.service.clone(),
            computer_id,
            data: agent_node_rpc::NodeSessionDataRpc::new(
                self.node_service.clone(),
                self.session_store.clone(),
                self.session_history_reader.clone(),
                self.attachment_reader.clone(),
                self.memory_progress_store.clone(),
            ),
            images: agent_node_rpc::NodeToolImageRpc::new(
                self.node_service.clone(),
                self.tool_image_store.clone(),
                self.config.storage.files.limits.max_upload_bytes,
            ),
            control: self.agent_control.clone(),
            files: self.service.files.clone(),
            auth: self.auth_service.clone(),
        })
    }
}

struct ApplicationAgentControl {
    service: Arc<TalosService>,
}

impl ApplicationAgentControl {
    fn new(service: Arc<TalosService>) -> Self {
        Self { service }
    }
}

impl AgentControl for ApplicationAgentControl {
    fn spawn(&self, request: SpawnSubagentRequest) -> AgentControlFuture<'_, SubagentSpawnResult> {
        Box::pin(async move { self.service.spawn_subagent(request).await })
    }

    fn send_message(
        &self,
        request: SendSubagentMessageRequest,
    ) -> AgentControlFuture<'_, SendMessageResult> {
        Box::pin(async move { self.service.send_subagent_message(request).await })
    }

    fn wait(&self, request: WaitSubagentRequest) -> AgentControlFuture<'_, SubagentSnapshot> {
        Box::pin(async move { self.service.wait_subagent(request).await })
    }

    fn interrupt(
        &self,
        request: InterruptSubagentRequest,
    ) -> AgentControlFuture<'_, SubagentSnapshot> {
        Box::pin(async move { self.service.interrupt_subagent(request).await })
    }

    fn follow_up(
        &self,
        request: FollowUpSubagentRequest,
    ) -> AgentControlFuture<'_, SubagentSnapshot> {
        Box::pin(async move { self.service.follow_up_subagent(request).await })
    }

    fn list(&self, request: ListSubagentsRequest) -> AgentControlFuture<'_, AgentTreeSnapshot> {
        Box::pin(async move { self.service.list_subagents(request).await })
    }
}

// Includes coordinator drain (60s), MCP drain/cleanup (130s), and gateway shutdown.
const SERVER_SHUTDOWN_WAIT_TIMEOUT: Duration = Duration::from_secs(240);

pub async fn run_app_server() -> TalosResult<()> {
    let logging_options = TalosServerOptions {
        logging_stdout: Some(false),
        ..TalosServerOptions::default()
    };
    let _log_guard = startup::prepare_startup(Some(&logging_options))?;
    async {
        let config = load_runtime_config()?;
        let config_source = resolve_config_source_path(&config);
        log_config_source(config_source);
        let startup_report = startup::finish_startup(&config)?;

        run_json_rpc_stdio_after_startup(config, &startup_report).await
    }
    .await
    .inspect_err(talos_logging::report_error)
}

pub async fn run_server() -> TalosResult<()> {
    let _log_guard = startup::prepare_startup(None)?;
    async {
        let config = load_runtime_config()?;
        let config_source = resolve_config_source_path(&config);
        let startup_report = startup::finish_startup(&config)?;
        let mut server_options = TalosServerOptions::default();
        log_config_source(config_source);
        server_options.webui_dir = resolve_packaged_webui_dir();
        info!(
            "Starting Talos server at {}:{}",
            config.gateway.host, config.gateway.port
        );

        let server =
            start_server_after_startup(config, Some(server_options), &startup_report).await?;
        server.run().await
    }
    .await
    .inspect_err(talos_logging::report_error)
}

/// Starts the product-managed in-process app-server runtime.
///
/// This entrypoint owns the full startup lifecycle for embedded clients: startup logging,
/// resource preparation, config loading, serial upgrades, and runtime assembly.
/// Lower-level runtime builders must not repeat startup lifecycle work.
///
/// The bundled Browser MCP re-enters the current executable with `--browser-mcp`.
/// Embedding hosts must handle that argument before normal CLI parsing or startup
/// by awaiting [`run_browser_mcp`] and then returning. Keep stdout reserved for
/// MCP in that branch; it requires neither authentication nor model configuration.
pub async fn run_embedded_app_server(
    options: Option<&TalosServerOptions>,
) -> TalosResult<EmbeddedAppServer> {
    let log_guard = startup::prepare_startup(options)?;
    let runtime = async {
        let config = load_runtime_config()?;
        let config_source = resolve_config_source_path(&config);
        log_config_source(config_source);
        let startup_report = startup::finish_startup(&config)?;
        build_application_runtime_after_startup(config, &startup_report).await
    }
    .await
    .inspect_err(talos_logging::report_error)?;
    Ok(EmbeddedAppServer { log_guard, runtime })
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
    eprintln!("{context}: {}", error.to_api_error().message);
    std::process::exit(1);
}

/// Bridges `CoordinatorImpl` to the `BackgroundTaskDrain` trait used by the app-server.
struct CoordinatorDrain {
    coordinator: Arc<CoordinatorImpl>,
}

#[async_trait::async_trait]
impl BackgroundTaskDrain for CoordinatorDrain {
    async fn drain(&self, timeout: Duration) -> TalosResult<()> {
        self.coordinator.shutdown(timeout).await
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
                    TalosError::internal(
                        "server runtime stopped before reporting shutdown completion",
                    )
                })?;
            }
        })
        .await
        .map_err(|_| {
            TalosError::internal("Server shutdown exceeded its configured timeout.").context(
                format!(
                    "server shutdown timed out after {} secs",
                    wait_timeout.as_secs()
                ),
            )
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
            sync_host,
            http_server,
            channel_server,
            node_rpc_server,
            coordinator,
            gateway_url: _,
            shutdown_token,
            lifecycle,
            automation,
            plugins,
            browser,
            worker_rpc,
            resource_rpc,
            worker_registration,
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

        let mut tasks = JoinSet::new();
        let shutdown_token_for_http = shutdown_token.clone();
        tasks.spawn(async move { http_server.run_with_shutdown(shutdown_token_for_http).await });
        if let Some(channel_server) = channel_server {
            tasks.spawn(async move { channel_server.run().await });
        }
        if let Some(node_rpc_server) = node_rpc_server {
            let shutdown_token_for_rpc = shutdown_token.clone();
            tasks.spawn(async move { node_rpc_server.run(shutdown_token_for_rpc).await });
        }
        let transport = worker_rpc.clone();
        tasks.spawn(async move {
            transport.stopped().await;
            transport.shutdown().await
        });
        let result = match tasks.join_next().await {
            Some(Ok(result)) => result,
            Some(Err(error)) => Err(TalosError::internal_with_source(
                "server task join error; technical details were withheld.",
                error,
            )),
            None => Ok(()),
        };
        shutdown_token.cancel();
        sync_host.shutdown_refreshes().await;
        let registration = match worker_registration.lock().await.take() {
            Some(registration) => registration.shutdown().await,
            None => Ok(()),
        };
        tasks.abort_all();
        while tasks.join_next().await.is_some() {}

        signal_task.abort();
        quiesce_task.abort();

        let shutdown_result = application_automation::shutdown_server(
            &coordinator,
            &automation,
            &lifecycle,
            &plugins,
        )
        .await;
        let result = application_automation::combine(result, registration);
        let result = application_automation::combine(result, browser.shutdown().await);
        let result = application_automation::combine(result, sync_host.shutdown().await);
        let result = application_automation::combine(result, shutdown_result);
        let result = application_automation::combine(result, worker_rpc.shutdown().await);
        application_automation::combine(result, resource_rpc.shutdown().await)
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

async fn start_server_after_startup(
    config: Config,
    options: Option<TalosServerOptions>,
    startup_report: &startup::FinishStartupReport,
) -> TalosResult<TalosServerRuntime> {
    let config = apply_runtime_options(config, options.as_ref());
    config.validate_runtime()?;
    let runtime = build_application_runtime_after_startup(config.clone(), startup_report).await?;

    let node_rpc_server = config
        .agent_node
        .control_plane
        .as_ref()
        .map(|node_config| {
            let session_data = agent_node_rpc::NodeSessionDataRpc::new(
                runtime.node_service.clone(),
                runtime.session_store.clone(),
                runtime.session_history_reader.clone(),
                runtime.attachment_reader.clone(),
                runtime.memory_progress_store.clone(),
            );
            agent_node_rpc::NodeRpcServer::new(
                node_config,
                runtime.node_service.clone(),
                runtime.auth_service.clone(),
                runtime.coordinator.clone(),
                session_data,
                runtime.agent_control.clone(),
                agent_node_rpc::NodeToolImageRpc::new(
                    runtime.node_service.clone(),
                    runtime.tool_image_store.clone(),
                    runtime.config.storage.files.limits.max_upload_bytes,
                ),
            )
            .and_then(|server| {
                server
                    .with_worker_adapter(runtime.worker_rpc.adapter())
                    .with_coordinator_id(runtime.sync.peer_id().into())
            })
        })
        .transpose()?;

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
        let mut channel_server =
            ChannelServer::new(Arc::new(config.clone()), runtime.service.clone());
        channel_server.register(Box::new(TelegramChannel::new()));
        Some(channel_server)
    } else {
        None
    };

    Ok(TalosServerRuntime {
        sync_host: runtime.sync_host,
        coordinator: runtime.coordinator,
        gateway_url: format!("http://{}:{}", config.gateway.host, config.gateway.port),
        shutdown_token: CancellationToken::new(),
        lifecycle: http_server.lifecycle(),
        automation: runtime.automation,
        plugins: runtime.worker.plugin_manager(),
        browser: runtime.worker.browser(),
        worker_rpc: runtime.worker_rpc,
        resource_rpc: runtime.resource_rpc,
        worker_registration: runtime.worker_registration,
        http_server,
        channel_server,
        node_rpc_server,
    })
}

async fn run_json_rpc_stdio_after_startup(
    config: Config,
    startup_report: &startup::FinishStartupReport,
) -> TalosResult<()> {
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
    let desktop_host_name = std::env::var("TALOS_DESKTOP_HOSTNAME").ok();
    // Desktop enrollment verifies the current saved login before attaching a
    // cloud identity. A saved node credential alone must not bypass that check.
    let mut runtime_config = config.clone();
    if desktop_host_name.is_some() {
        runtime_config.agent_node.runner = None;
    }
    let runtime = build_application_runtime_after_startup(runtime_config, startup_report).await?;
    let desktop_host_bridge = if desktop_host_name.is_some() {
        let bridge = talos_app_server::DesktopHostBridge::new();
        runtime
            .worker
            .browser()
            .set_backend(talos_browser::NativeBrowser::new(bridge.clone()))?;
        Some(bridge)
    } else {
        None
    };
    let automation = runtime.automation.clone();
    let plugins = runtime.worker.plugin_manager();
    let worker_rpc = runtime.worker_rpc.clone();
    let replay_limit = config.agent.timeline_replay_batch_size.max(1);
    let service = runtime.service.clone();
    let app = Arc::new(
        talos_app_server::TalosApplicationAdapter::new(service.clone(), replay_limit)
            .with_drain_provider(Box::new(CoordinatorDrain {
                coordinator: runtime.coordinator.clone(),
            }))
            .with_cloud_api_base_url(config.cloud.api_base_url.clone()),
    );
    let validator = Arc::new(talos_app_server::AuthServiceCredentialValidator::new(
        runtime.auth_service.clone(),
        service.account.clone(),
    ));
    let app: Arc<dyn talos_app_server::AppServerApplication> = match desktop_host_name {
        Some(device_name) => Arc::new(desktop_application::DesktopApplication::new(
            &runtime,
            device_name,
            app,
            desktop_host_bridge
                .clone()
                .expect("Desktop host bridge is composed together"),
        )),
        None => app,
    };
    let result = talos_app_server::run_stdio_with_max_frame_bytes(
        app,
        validator,
        talos_app_server::AppServerConfig {
            desktop_host: desktop_host_bridge,
            auth_enabled: config.gateway.auth,
            cloud_auth_url: Some(config.cloud.web_base_url.clone()),
            ..talos_app_server::AppServerConfig::default()
        },
        max_incoming_frame_bytes,
    )
    .await
    .map_err(|error| {
        TalosError::internal_with_source(
            "App-server failed to serve the connection; technical details were withheld.",
            error,
        )
    });
    let result = application_automation::finish(result, &automation, &plugins).await;
    runtime.sync_host.shutdown_refreshes().await;
    let registration = match runtime.worker_registration.lock().await.take() {
        Some(registration) => registration.shutdown().await,
        None => Ok(()),
    };
    let result = application_automation::combine(result, registration);
    let result = application_automation::combine(result, runtime.worker.browser().shutdown().await);
    let result = application_automation::combine(result, worker_rpc.shutdown().await);
    let result = application_automation::combine(result, runtime.shutdown_sync().await);
    application_automation::combine(result, runtime.resource_rpc.shutdown().await)
}

async fn build_application_runtime_after_startup(
    config: Config,
    startup_report: &startup::FinishStartupReport,
) -> TalosResult<TalosApplicationRuntime> {
    application_runtime::build_application_runtime_with_migration_dir(
        config,
        &startup_report.resources.database_migrations.backend_dir,
    )
    .await
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

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;

pub use talos_worker::run_browser_mcp;
