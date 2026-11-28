//! Assemble the existing cloud product, control plane and optional Relay.
use super::*;
use talos_channel::{ChannelServer, TelegramChannel};
use talos_gateway_http::{HttpServer, ServerLifecycle, ServerLifecycleState};
use talos_service::AgentNodeService;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use webui::resolve_packaged_webui_dir;

pub struct TalosServerRuntime {
    auth_service: Arc<dyn AuthService>,
    service: Arc<TalosService>,
    pub(super) host: Arc<application_host::ApplicationHost>,
    http_server: HttpServer,
    channel_server: Option<ChannelServer>,
    node_rpc_server: Option<agent_node_rpc::NodeRpcServer>,
    managed_worker: Option<(AgentNodeService, talos_fs::FileLock)>,
    pub(super) coordinator: Arc<CoordinatorImpl>,
    gateway_url: String,
    shutdown_token: CancellationToken,
    lifecycle: Arc<ServerLifecycle>,
    automation: Automation,
    _native_job_host: Arc<dyn talos_service::NativeJobHost>,
    worker_rpc: Option<Arc<talos_worker::WorkerRpcHost>>,
    resource_stop: tokio_util::sync::CancellationToken,
    _resource_stop_guard: Arc<tokio_util::sync::DropGuard>,
    worker_registration: Arc<tokio::sync::Mutex<Option<talos_worker::RegisteredWorker>>>,
}

// Allow cooperative Worker source/Browser preparation to finish before the
// execution/product drains, MCP cleanup, and gateway shutdown complete.
const SERVER_SHUTDOWN_WAIT_TIMEOUT: Duration = Duration::from_secs(900);

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
            auth_service,
            service,
            host,
            http_server,
            channel_server,
            node_rpc_server,
            managed_worker,
            coordinator,
            gateway_url: _,
            shutdown_token,
            lifecycle,
            automation,
            worker_rpc,
            resource_stop,
            _resource_stop_guard,
            worker_registration,
            _native_job_host,
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
        if let Some((nodes, lock)) = managed_worker {
            let shutdown = shutdown_token.clone();
            tasks.spawn(async move {
                let _lock = lock;
                nodes.run_managed_workers(shutdown).await;
                Ok(())
            });
        }
        let shutdown_token_for_http = shutdown_token.clone();
        tasks.spawn(async move { http_server.run_with_shutdown(shutdown_token_for_http).await });
        if let Some(channel_server) = channel_server {
            tasks.spawn(async move { channel_server.run().await });
        }
        if let Some(node_rpc_server) = node_rpc_server {
            let shutdown_token_for_rpc = shutdown_token.clone();
            tasks.spawn(async move { node_rpc_server.run(shutdown_token_for_rpc).await });
        }
        if let Some(transport) = worker_rpc.clone() {
            tasks.spawn(async move {
                transport.stopped().await;
                transport.shutdown().await
            });
        }
        let result = match tasks.join_next().await {
            Some(Ok(result)) => result,
            Some(Err(error)) => Err(TalosError::internal_with_source(
                "server task join error; technical details were withheld.",
                error,
            )),
            None => Ok(()),
        };
        shutdown_token.cancel();
        host.shutdown_refreshes().await;
        tasks.abort_all();
        while tasks.join_next().await.is_some() {}

        signal_task.abort();
        quiesce_task.abort();

        let background_result = match &host.worker {
            Some(worker) => worker.shutdown_background_tasks().await,
            None => Ok(()),
        };
        let shutdown_result = application_automation::shutdown_server(
            host.worker.as_ref(),
            &coordinator,
            &automation,
            &service,
            &lifecycle,
        )
        .await;
        let registration = match worker_registration.lock().await.take() {
            Some(registration) => registration.shutdown().await,
            None => Ok(()),
        };
        let result = application_automation::combine(result, registration);
        let result = application_automation::combine(result, background_result);
        let result = match &host.worker {
            Some(worker) => {
                application_automation::combine(result, worker.browser().shutdown().await)
            }
            None => result,
        };
        let result = application_automation::combine(result, host.shutdown().await);
        let result = application_automation::combine(result, shutdown_result);
        let result = match &worker_rpc {
            Some(rpc) => application_automation::combine(result, rpc.shutdown().await),
            None => result,
        };
        auth_service.shutdown().await;
        {
            resource_stop.cancel();
            result
        }
    }
}

pub(super) async fn start_server_after_startup(
    config: Config,
    options: Option<TalosServerOptions>,
    startup_report: &startup::FinishStartupReport,
) -> TalosResult<TalosServerRuntime> {
    let config = apply_runtime_options(config, options.as_ref());
    config.validate_runtime()?;
    let runtime = build_application_runtime_after_startup(
        config.clone(),
        startup_report,
        options.as_ref(),
        talos_types::NodeProductOwner::Server,
    )
    .await?;

    let nodes = runtime
        .node_service
        .as_ref()
        .expect("Server product owns the control plane");
    let managed_worker =
        application_managed_worker::configure(&config, nodes, runtime.auth_service.clone()).await?;

    let node_rpc_server = config
        .agent_node
        .control_plane
        .as_ref()
        .map(|node_config| {
            let session_data = agent_node_rpc::NodeSessionDataRpc::new(
                nodes.clone(),
                runtime.session_store.clone(),
                runtime.attachment_reader.clone(),
            );
            agent_node_rpc::NodeRpcServer::new(
                node_config,
                nodes.clone(),
                runtime.auth_service.clone(),
                runtime.coordinator.clone(),
                agent_node_rpc::NodeResourceServices {
                    session_data,
                    agent_control: runtime.agent_control.clone(),
                    automation_control: runtime.automation_control.clone(),
                    tool_images: agent_node_rpc::NodeToolImageRpc::new(
                        nodes.clone(),
                        runtime.tool_image_store.clone(),
                        runtime.config.storage.files.limits.max_upload_bytes,
                    ),
                },
            )
            .and_then(|server| {
                let server = match &runtime.worker_rpc {
                    Some(rpc) => server.with_worker_adapter(rpc.adapter()),
                    None => server,
                };
                server.with_coordinator_id(runtime.host_id.as_str().into())
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
    )
    .with_routes(product_capability_routes(
        talos_types::NodeProductOwner::Server,
    ));

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

    let shutdown_token = CancellationToken::new();
    http_server = server_product_routing::ServerProductRouter::assemble(
        http_server,
        nodes.clone(),
        runtime.service.clone(),
        runtime.host_id.clone(),
        shutdown_token.clone(),
    );

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
        auth_service: runtime.auth_service,
        service: runtime.service,
        host: runtime.host,
        coordinator: runtime.coordinator,
        gateway_url: format!("http://{}:{}", config.gateway.host, config.gateway.port),
        shutdown_token,
        lifecycle: http_server.lifecycle(),
        automation: runtime.automation,
        _native_job_host: runtime._native_job_host,
        worker_rpc: runtime.worker_rpc,
        resource_stop: runtime.resource_stop,
        _resource_stop_guard: runtime._resource_stop_guard,
        worker_registration: runtime.worker_registration,
        http_server,
        channel_server,
        node_rpc_server,
        managed_worker,
    })
}

#[cfg(test)]
#[path = "server_runtime_tests.rs"]
mod tests;
