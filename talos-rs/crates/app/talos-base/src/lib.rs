#[cfg(feature = "server")]
mod server_product_routing;
#[cfg(feature = "server")]
mod server_runtime;
#[cfg(all(test, feature = "server"))]
use server_runtime::start_server_after_startup;
#[cfg(feature = "server")]
pub use server_runtime::{run_server, TalosServerHandle, TalosServerRuntime};
mod webui;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use webui::canonical_or_original;
#[cfg(test)]
use webui::{resolve_webui_dir, WebuiLookupInput, WebuiLookupResult};

use talos_app_server::BackgroundTaskDrain;
use talos_auth::AuthService;
use talos_automation::Automation;
use talos_config::Config;
use talos_coordinator::CoordinatorImpl;
use talos_error::{TalosError, TalosResult};
use talos_gateway::Gateway;
use talos_logging::LogGuard;
#[cfg(feature = "server")]
use talos_service::AgentNodeService;
use talos_service::{TalosService, TimelineEventService};
use talos_tools::{AgentControl, AgentControlFuture};
use talos_types::{
    AgentTreeSnapshot, FollowUpSubagentRequest, InterruptSubagentRequest, ListSubagentsRequest,
    SendMessageResult, SendSubagentMessageRequest, SpawnSubagentRequest, SubagentSnapshot,
    SubagentSpawnResult, WaitSubagentRequest,
};
use tracing::info;

#[cfg(feature = "server")]
const TALOS_WEBUI_DIR_ENV: &str = "TALOS_WEBUI_DIR";

mod agent_node_resources_rpc;
#[cfg(feature = "server")]
mod agent_node_rpc;
mod application_automation_control;
mod application_native_jobs;
mod desktop_relay;
#[cfg(feature = "server")]
pub use agent_node_rpc::{
    NodeResourceServices, NodeRpcServer, NodeSessionDataRpc, NodeToolImageRpc,
};
mod application_automation;
mod application_computer_directory;
mod application_desktop;
mod application_host;
#[cfg(feature = "server")]
mod application_managed_worker;
mod application_management_host;
mod application_runtime;
mod application_storage;
#[cfg(feature = "sync")]
mod application_sync;
mod application_sync_background;
mod application_sync_computers;
mod application_sync_deletion;
#[cfg(feature = "sync")]
mod application_sync_enrollment;
mod application_sync_files;
#[cfg(feature = "sync")]
mod application_sync_hub_recovery;
#[cfg(feature = "sync")]
mod application_sync_management;
#[cfg(feature = "sync")]
mod application_sync_metrics;
#[cfg(feature = "sync")]
mod application_sync_observer;
#[cfg(all(test, feature = "e2e", feature = "sync"))]
#[path = "application_sync_tests.rs"]
mod application_sync_tests;
mod desktop_application;
mod desktop_cloud;
mod desktop_media;
mod desktop_worker;
mod dream_memory_maintenance;
mod host_identity;
#[cfg(feature = "relay-client")]
mod relay_host;
#[cfg(all(test, feature = "server"))]
#[path = "application_sync_server_tests.rs"]
mod server_registration_tests;
mod startup;
mod tool_images;

#[cfg(test)]
pub(crate) use application_runtime::build_application_runtime_with_migration_dir;

pub use talos_agent::SessionHold;

/// Who holds a session lock in this process, and for how long.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionOwnershipPolicy {
    pub kind: talos_error::SessionOwnerKind,
    pub hold: talos_agent::SessionHold,
}

impl Default for SessionOwnershipPolicy {
    fn default() -> Self {
        Self {
            kind: talos_error::SessionOwnerKind::Server,
            hold: talos_agent::SessionHold::Turn,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct TalosServerOptions {
    pub host: Option<String>,
    pub port: Option<u16>,
    pub logging_stdout: Option<bool>,
    pub auth_disabled: Option<bool>,
    pub enable_channels: Option<bool>,
    pub webui_dir: Option<PathBuf>,
    pub session_ownership: Option<SessionOwnershipPolicy>,
}

/// Owns the resources created for an in-process app-server runtime.
///
/// This is the product-level embedded entrypoint result used by local clients such as the TUI.
/// Callers must keep the `log_guard` alive for the lifetime of the embedded runtime.
pub struct EmbeddedAppServer {
    pub log_guard: LogGuard,
    pub runtime: TalosApplicationRuntime,
}

/// Product-definition ports for short management commands. No execution runtime is owned.
pub struct TalosProductRuntime {
    pub config: Config,
    pub service: Arc<TalosService>,
    pub event_service: Arc<TimelineEventService>,
    pub auth_service: Arc<dyn AuthService>,
    pub gateway: Arc<dyn Gateway>,
}
pub struct EmbeddedProductServer {
    pub log_guard: LogGuard,
    pub runtime: TalosProductRuntime,
}

#[derive(Clone)]
pub struct TalosApplicationRuntime {
    pub(crate) host: Arc<application_host::ApplicationHost>,
    #[cfg(all(test, feature = "sync"))]
    pub(crate) sync: Arc<talos_sync::TalosSync>,
    pub(crate) host_id: String,
    pub(crate) worker: Option<talos_worker::TalosWorker>,
    pub(crate) worker_rpc: Option<Arc<talos_worker::WorkerRpcHost>>,
    pub(crate) resource_stop: tokio_util::sync::CancellationToken,
    _resource_stop_guard: Arc<tokio_util::sync::DropGuard>,
    pub(crate) worker_registration: Arc<tokio::sync::Mutex<Option<talos_worker::RegisteredWorker>>>,
    pub(crate) worker_failures: tokio::sync::watch::Sender<Option<talos_error::ApiError>>,
    pub config: Config,
    pub service: Arc<TalosService>,
    pub event_service: Arc<TimelineEventService>,
    pub auth_service: Arc<dyn AuthService>,
    pub gateway: Arc<dyn Gateway>,
    pub coordinator: Arc<CoordinatorImpl>,
    #[cfg(feature = "server")]
    pub node_service: Option<AgentNodeService>,
    #[cfg(feature = "server")]
    pub(crate) session_store: Arc<dyn talos_store::session::SessionStore>,
    #[cfg(feature = "server")]
    pub(crate) attachment_reader: Arc<dyn talos_agent::AttachmentReader>,
    #[cfg(feature = "server")]
    pub(crate) agent_control: Arc<dyn talos_tools::AgentControl>,
    #[cfg(feature = "server")]
    pub(crate) automation_control: Arc<dyn talos_tools::AutomationControl>,
    #[cfg(feature = "server")]
    pub(crate) tool_image_store: Arc<dyn talos_tools::ToolImageStore>,
    automation: Automation,
    _native_job_host: Arc<dyn talos_service::NativeJobHost>,
}

impl TalosApplicationRuntime {
    /// Stable management identity shared with authenticated Connect and sync.
    pub fn host_id(&self) -> &str {
        &self.host_id
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
        build_application_runtime_after_startup(
            config,
            &startup_report,
            options,
            talos_types::NodeProductOwner::Computer,
        )
        .await
    }
    .await
    .inspect_err(talos_logging::report_error)?;
    Ok(EmbeddedAppServer { log_guard, runtime })
}

/// Open existing product definitions without starting Worker, Sync, registration or schedulers.
pub async fn run_embedded_product_server(
    options: Option<&TalosServerOptions>,
) -> TalosResult<EmbeddedProductServer> {
    let log_guard = startup::prepare_startup(options)?;
    let config = load_runtime_config()?;
    let report = startup::finish_startup(&config)?;
    let runtime = application_runtime::build_product_runtime(
        config,
        &report.resources.database_migrations.backend_dir,
    )
    .await?;
    Ok(EmbeddedProductServer { log_guard, runtime })
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

fn log_config_source(config_source: Option<PathBuf>) {
    if let Some(path) = config_source {
        info!("Config loaded from {}", path.display());
    }
}

pub fn exit_with_error(context: &str, error: TalosError) -> ! {
    eprintln!("{context}: {}", error.to_api_error().message);
    std::process::exit(1);
}

/// Stops local execution and drains product delivery before subscriptions close.
/// The runtime alone owns final Coordinator and Provider teardown.
struct ProductDeliveryDrain {
    worker: talos_worker::TalosWorker,
    service: Arc<TalosService>,
    automation: Automation,
}

#[async_trait::async_trait]
impl BackgroundTaskDrain for ProductDeliveryDrain {
    async fn drain(&self, timeout: Duration) -> TalosResult<()> {
        let result = self.automation.shutdown().await;
        self.service.begin_execution_drain();
        let result =
            application_automation::combine(result, self.worker.stop_execution(timeout).await);
        application_automation::combine(
            result,
            application_automation::drain_product_delivery(&self.service, timeout).await,
        )
    }
}

pub fn init_logging(
    config: &Config,
    options: Option<&TalosServerOptions>,
) -> TalosResult<LogGuard> {
    let config = apply_runtime_options(config.clone(), options);
    talos_logging::init_with_config(config.logging, &config.timezone)
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
    let runtime = build_application_runtime_after_startup(
        runtime_config,
        startup_report,
        desktop_session_options().as_ref(),
        talos_types::NodeProductOwner::Computer,
    )
    .await?;
    let desktop_host_bridge = if desktop_host_name.is_some() {
        let bridge = talos_app_server::DesktopHostBridge::new();
        runtime
            .worker
            .as_ref()
            .expect("Computer composition owns its embedded Worker")
            .browser()
            .set_backend(talos_browser::NativeBrowser::new(bridge.clone()))?;
        Some(bridge)
    } else {
        None
    };
    let replay_limit = config.agent.timeline_replay_batch_size.max(1);
    let service = runtime.service.clone();
    let app = Arc::new(
        talos_app_server::TalosApplicationAdapter::new(service.clone(), replay_limit)
            .with_drain_provider(Box::new(ProductDeliveryDrain {
                worker: runtime
                    .worker
                    .clone()
                    .expect("Computer composition owns its embedded Worker"),
                service: service.clone(),
                automation: runtime.automation.clone(),
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
        None => Arc::new(desktop_application::DesktopApplication::new(
            &runtime,
            config.agent.name.clone(),
            app,
            talos_app_server::DesktopHostBridge::new(),
        )),
    };
    let result = talos_app_server::run_stdio_with_max_frame_bytes(
        app,
        validator,
        talos_app_server::AppServerConfig {
            desktop_host: desktop_host_bridge,
            capabilities: talos_app_protocol::requests::ServerCapabilities {
                sync: cfg!(feature = "sync"),
                relay: cfg!(feature = "relay-client"),
            },
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
    runtime.host.shutdown_refreshes().await;
    let result = application_automation::combine(result, runtime.shutdown().await);
    let result = application_automation::combine(result, runtime.shutdown_host().await);
    runtime.auth_service.shutdown().await;
    {
        runtime.resource_stop.cancel();
        result
    }
}

/// Product capability discovery reports assembly, independently of account settings.
#[cfg(any(feature = "server", feature = "relay-client", test))]
pub(crate) fn product_capability_routes(owner: talos_types::NodeProductOwner) -> axum::Router {
    let relay = match owner {
        talos_types::NodeProductOwner::Computer => cfg!(feature = "relay-client"),
        talos_types::NodeProductOwner::Server => cfg!(feature = "relay"),
    };
    axum::Router::new().route(
        "/v1/capabilities",
        axum::routing::get(move || async move {
            axum::Json(talos_types::ApiResponse::success(
                talos_app_protocol::requests::ServerCapabilities {
                    sync: cfg!(feature = "sync"),
                    relay,
                },
            ))
        }),
    )
}

async fn build_application_runtime_after_startup(
    config: Config,
    startup_report: &startup::FinishStartupReport,
    options: Option<&TalosServerOptions>,
    product_owner: talos_types::NodeProductOwner,
) -> TalosResult<TalosApplicationRuntime> {
    application_runtime::build_application_runtime_with_policy(
        config,
        &startup_report.resources.database_migrations.backend_dir,
        options
            .and_then(|options| options.session_ownership)
            .unwrap_or_default(),
        product_owner,
    )
    .await
}

fn desktop_session_options() -> Option<TalosServerOptions> {
    std::env::var_os("TALOS_DESKTOP_HOSTNAME").map(|_| TalosServerOptions {
        session_ownership: Some(SessionOwnershipPolicy {
            kind: talos_error::SessionOwnerKind::Desktop,
            hold: talos_agent::SessionHold::Turn,
        }),
        ..TalosServerOptions::default()
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

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;

pub use talos_worker::run_browser_mcp;
