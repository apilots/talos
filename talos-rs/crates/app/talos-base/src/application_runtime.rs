use std::path::Path;
use std::sync::Arc;

use talos_auth::{AuthService, CloudAuthService, OpenModeUserService, UserService};
use talos_config::Config;
use talos_coordinator::CoordinatorImpl;
use talos_error::TalosResult;
use talos_gateway::{Gateway, ServiceGateway};
use talos_llm_provider::LlmProviderRouter;
use talos_router::{Binding, MatchRule, Router, RouterImpl};
use talos_service::{FileService, InnerModelClient, TalosService, TimelineEventService};
use talos_store::node::DatabaseAgentNodeStore;
use talos_store::project::{ProjectStore, StorageProjectStore};
use talos_store::session::{AgentExecutionStore, DatabaseSessionStore, SessionStore};

#[cfg(feature = "server")]
use talos_tools::AgentControl;
use talos_tools::ToolImageStore;
use talos_types::{resolve_default_model_selection, resolve_model_catalog};
use talos_worker::{TalosWorker, TalosWorkerResources};

use super::{
    application_automation, application_storage, ApplicationAgentControl, TalosApplicationRuntime,
};

#[cfg(test)]
pub(crate) async fn build_application_runtime_with_migration_dir(
    config: Config,
    migration_dir: &Path,
) -> TalosResult<TalosApplicationRuntime> {
    let product_owner = if config.agent_node.control_plane.is_some() {
        talos_types::NodeProductOwner::Server
    } else {
        talos_types::NodeProductOwner::Computer
    };
    build_application_runtime_with_policy(
        config,
        migration_dir,
        crate::SessionOwnershipPolicy::default(),
        product_owner,
    )
    .await
}

pub(crate) async fn build_application_runtime_with_policy(
    config: Config,
    migration_dir: &Path,
    session_ownership: crate::SessionOwnershipPolicy,
    product_owner: talos_types::NodeProductOwner,
) -> TalosResult<TalosApplicationRuntime> {
    #[cfg(not(feature = "server"))]
    if product_owner == talos_types::NodeProductOwner::Server {
        return Err(talos_error::TalosError::new(
            talos_error::ErrorCode::ServiceUnavailable,
            "This product does not include Server composition.",
        ));
    }
    let ProductStores {
        storage_connection,
        storage,
        profile_store,
        #[cfg(feature = "sync")]
        file_store,
        host_id,
        session_store_impl,
        node_store,
        project_store_impl,
        file_service,
        event_service,
    } = build_product_stores(&config, migration_dir).await?;
    #[cfg(feature = "sync")]
    let sync = Arc::new(
        talos_sync::TalosSync::open(
            storage.clone(),
            talos_sync::SyncOptions {
                hub_endpoint: config.sync.hub_endpoint.clone(),
            },
        )
        .await?,
    );
    let session_store: Arc<dyn SessionStore> = session_store_impl.clone();
    let project_store: Arc<dyn ProjectStore> = project_store_impl.clone();
    let attachment_reader: Arc<dyn talos_agent::AttachmentReader> = file_service.clone();
    let tool_image_store: Arc<dyn ToolImageStore> =
        Arc::new(super::tool_images::ToolImages(file_service.clone()));
    let worker = TalosWorker::build_with_browser_host(
        config.clone(),
        TalosWorkerResources {
            attachment_reader: file_service.clone(),
            tool_image_store: tool_image_store.clone(),
            agent_control: None,
            automation_control: None,
        },
        if std::env::var_os("TALOS_DESKTOP_HOSTNAME").is_some() {
            talos_types::BrowserHostKind::Desktop
        } else {
            talos_types::BrowserHostKind::Server
        },
    )
    .await?;
    file_service.reconcile_incomplete().await?;
    let default_model_selection = resolve_default_model_selection(&config.llm)?;
    let default_agent_id = worker
        .deployments()
        .first()
        .map(|deployment| deployment.agent_id.clone())
        .expect("TalosWorker requires an enabled Agent deployment");
    let router: Arc<dyn Router> = Arc::new(RouterImpl::new());
    router
        .add_binding(Binding {
            id: "default_binding".to_string(),
            agent_id: default_agent_id.clone(),
            rule: MatchRule::Default,
            priority: 1,
        })
        .await?;

    let reloadable_llm_provider_router = worker.provider_router();
    let llm_provider_router: Arc<dyn LlmProviderRouter> = reloadable_llm_provider_router.clone();
    let model_catalog = resolve_model_catalog(&config.llm);
    let inner_model = Arc::new(
        InnerModelClient::builder()
            .configured_model(config.agent.inner_model.clone())
            .agent_default_model(default_model_selection.model_ref.clone())
            .model_catalog(model_catalog.clone())
            .provider_router(llm_provider_router.clone())
            .profile_store(profile_store.clone())
            .profile_timezone(config.agent.kernel.timezone.clone())
            .build()?,
    );
    let tool_registry = worker.tool_registry();

    let coordinator = Arc::new(CoordinatorImpl::new(
        router,
        session_store.clone(),
        node_store.clone(),
    ));
    let (worker_rpc, local_auth) = worker
        .start_local(session_ownership.kind, session_ownership.hold)
        .await?;
    coordinator
        .register_environment(
            "embedded".into(),
            worker.local_environment(&worker_rpc, local_auth.clone())?,
        )
        .await;
    for (agent_id, agent) in worker.local_harnesses(&worker_rpc, &local_auth, host_id.as_str())? {
        coordinator
            .register_agent("embedded".into(), agent_id, agent)
            .await?;
    }
    use talos_store::node::AgentNodeStore;
    let embedded = node_store
        .upsert_embedded(
            if worker.deployments().is_empty() {
                talos_types::NodeStatus::Disabled
            } else {
                talos_types::NodeStatus::Enabled
            },
            chrono::Utc::now().timestamp(),
        )
        .await?;
    #[cfg(feature = "server")]
    let node_service = (product_owner == talos_types::NodeProductOwner::Server)
        .then(|| talos_service::AgentNodeService::new(node_store.clone()));
    #[cfg(feature = "server")]
    let product_nodes = match &node_service {
        Some(server) => talos_service::ProductNodeAccess::from(server.clone()),
        None => talos_service::ProductNodeAccess::computer(
            embedded.into_info(),
            Arc::new(agent_protocol::NodeAdmissionGates::default()),
        ),
    };

    #[cfg(not(feature = "server"))]
    let product_nodes = talos_service::ProductNodeAccess::computer(
        embedded.into_info(),
        Arc::new(agent_protocol::NodeAdmissionGates::default()),
    );

    let cloud_auth = Arc::new(CloudAuthService::new(config.cloud.api_base_url.clone()));
    let auth_service: Arc<dyn AuthService> = cloud_auth.clone();
    let user_service: Arc<dyn UserService> = if config.gateway.auth {
        cloud_auth
    } else {
        Arc::new(OpenModeUserService)
    };

    let service = Arc::new(
        TalosService::new(
            coordinator.clone(),
            product_nodes,
            session_store.clone(),
            session_store_impl.clone(),
            project_store,
            profile_store.clone(),
            file_service,
            event_service.clone(),
            config.clone(),
            auth_service.clone(),
            user_service,
        )
        .with_inner_model(inner_model.clone())
        .with_product_owner(product_owner),
    );
    service
        .account
        .set_cloud_session_cache(talos_auth::CloudSessionCache::for_talos_home(
            config.talos_paths().talos_home,
        ))?;
    service.set_coordinator_id(host_id.as_str().into());
    let session_owner = worker_rpc.session_owner();
    let loop_scan_enabled = session_owner.background_scan_enabled();
    service.set_session_owner(session_owner);
    service.install_session_automation(
        session_store_impl.automation_store(),
        Arc::new(
            super::application_automation_control::ApplicationAutomationSkillResolver::new(
                Arc::downgrade(&coordinator),
                session_store.clone(),
            ),
        ),
    )?;
    let stores = Arc::new(super::application_host::ApplicationStores {
        #[cfg(any(feature = "sync", test))]
        storage: storage.clone(),
        sessions: session_store_impl.clone(),
        #[cfg(feature = "sync")]
        projects: project_store_impl,
        nodes: node_store,
        #[cfg(feature = "sync")]
        files: file_store,
        #[cfg(feature = "sync")]
        handles: Default::default(),
        #[cfg(feature = "sync")]
        observers: Default::default(),
        #[cfg(feature = "sync")]
        credentials: Default::default(),
    });
    #[cfg(feature = "server")]
    let agent_control: Arc<dyn AgentControl> =
        Arc::new(ApplicationAgentControl::new(service.clone()));
    #[cfg(feature = "server")]
    let automation_control: Arc<dyn talos_tools::AutomationControl> = Arc::new(
        super::application_automation_control::ApplicationAutomationControl::new(service.clone()),
    );
    let local_data = super::agent_node_resources_rpc::NodeSessionDataRpc::local(
        session_store.clone(),
        attachment_reader.clone(),
    );
    worker.use_local_resources({
        let service = Arc::downgrade(&service);
        let attachments = attachment_reader.clone();
        let images = tool_image_store.clone();
        move |session| {
            let data = local_data.clone();
            let service = service.clone();
            let (attachments, images) = (attachments.clone(), images.clone());
            Box::pin(async move {
                let record = data.authorize_local_session(&session).await?;
                let files = Arc::new(super::tool_images::InvocationFiles {
                    owner_user_id: record.user_id,
                    attachments,
                    images,
                });
                let service = service.upgrade().ok_or_else(|| {
                    talos_error::TalosError::new(
                        talos_error::ErrorCode::ServiceUnavailable,
                        "Local application is shutting down",
                    )
                })?;
                Ok(TalosWorkerResources {
                    attachment_reader: files.clone(),
                    tool_image_store: files,
                    agent_control: Some(Arc::new(ApplicationAgentControl::new(service.clone()))),
                    automation_control: Some(Arc::new(
                        super::application_automation_control::ApplicationAutomationControl::new(
                            service,
                        ),
                    )),
                })
            })
        }
    });
    let resource_stop = tokio_util::sync::CancellationToken::new();
    let resource_stop_guard = std::sync::Arc::new(resource_stop.clone().drop_guard());
    let media = super::desktop_relay::DesktopRelay::new(resource_stop.clone());
    service.set_desktop_ingress(Arc::new(
        super::application_desktop::ApplicationDesktopIngress {
            local: media.clone(),
            remote: super::application_sync_computers::gateway_client(&config).await?,
            auth: auth_service.clone(),
        },
    ))?;
    if worker.browser().snapshot()?.host == talos_types::BrowserHostKind::Docker {
        let registered = worker.registered_desktop_connector();
        worker
            .browser()
            .set_desktop_connector(Arc::new(move |request| {
                let local = media.clone();
                let registered = registered.clone();
                Box::pin(async move {
                    // Route by the trusted reservation owner; an invalid local nonce
                    // must fail locally rather than retry through a different ingress.
                    if local.contains_reservation(&request.binding.stream_id).await {
                        local.attach_local_worker(request).await
                    } else {
                        registered(request).await
                    }
                })
            }))?;
    }

    let gateway: Arc<dyn Gateway> = Arc::new(ServiceGateway::new(service.clone()));
    let dream_memory_maintenance = crate::dream_memory_maintenance::DreamMemoryMaintenance::new(
        session_store_impl,
        service.clone(),
        coordinator.clone(),
        inner_model,
        &config,
        config.automation.dream_memory_maintenance.clone(),
    );
    use talos_store::automation::native_jobs::NativeJobControlStore;
    let native_job_controls = Arc::new(
        talos_store::automation::native_jobs::DatabaseNativeJobControlStore::new(
            storage_connection.pool().clone(),
        ),
    );
    let initial_paused = native_job_controls.load_paused(host_id.as_str()).await?;
    let automation = application_automation::start_maintenance(
        profile_store,
        config.automation.profile_usage_retention.clone(),
        dream_memory_maintenance,
        config.automation.dream_memory_maintenance.clone(),
        talos_config::parse_timezone(&config.timezone)?,
        initial_paused,
        loop_scan_enabled.then(|| Arc::downgrade(&service)),
    )
    .await?;

    // No external MCP starts until every fallible assembly step has completed.
    worker.activate_mcp()?;
    let worker_failures = tokio::sync::watch::channel(None).0;
    let registration = if let Some(runner) = &config.agent_node.runner {
        let failures = worker_failures.clone();
        match talos_worker::RegisteredWorker::attach(
            &worker,
            worker_rpc.clone(),
            runner,
            product_owner,
            Some(Arc::new(move |error| {
                talos_logging::report_error(&error);
                failures.send_replace(Some(error.to_api_error()));
            })),
        )
        .await
        {
            Ok(registration) => Some(registration),
            Err(error) => {
                // Registration failure belongs to the cloud surface. The
                // already assembled local management runtime remains usable.
                talos_logging::report_error(&error);
                worker_failures.send_replace(Some(error.to_api_error()));
                None
            }
        }
    } else {
        None
    };
    let worker_registration = Arc::new(tokio::sync::Mutex::new(registration));
    let host = Arc::new_cyclic(|host| super::application_host::ApplicationHost {
        self_weak: host.clone(),
        computer_directory: Default::default(),
        registration_retirement: Default::default(),
        #[cfg(feature = "relay-client")]
        product_router: talos_gateway_http::HttpServer::new(
            service.clone(),
            event_service.clone(),
            auth_service.clone(),
            config.clone(),
            "127.0.0.1",
            0,
        )
        .with_routes(crate::product_capability_routes(
            talos_types::NodeProductOwner::Computer,
        ))
        .router(),
        #[cfg(feature = "relay-client")]
        product_relay: Default::default(),
        config: config.clone(),
        #[cfg(feature = "relay-client")]
        host_id: host_id.clone(),
        product_owner,
        #[cfg(feature = "sync")]
        sync: sync.clone(),
        stores: stores.clone(),
        auth_service: auth_service.clone(),
        service: service.clone(),
        #[cfg(feature = "sync")]
        event_service: event_service.clone(),
        worker: worker.clone(),
        worker_rpc: worker_rpc.clone(),
        worker_registration: worker_registration.clone(),
        worker_failures: worker_failures.clone(),
        trusted_local_host: std::env::var_os("TALOS_DESKTOP_HOSTNAME").is_some()
            || (product_owner == talos_types::NodeProductOwner::Computer
                && matches!(
                    config.gateway.host.as_str(),
                    "127.0.0.1" | "::1" | "localhost"
                )),
        local_owner: Default::default(),
        tokens: Default::default(),
        lifecycle: Default::default(),
        background: Default::default(),
        failures: Default::default(),
    });
    host.install_peer_access();
    #[cfg(feature = "relay-client")]
    host.refresh_product_relay().await?;
    #[cfg(feature = "sync")]
    for binding in sync.saved_bindings().await? {
        host.install_capture(binding)?;
    }
    let native_job_host: Arc<dyn talos_service::NativeJobHost> =
        Arc::new(super::application_native_jobs::ApplicationNativeJobHost {
            host: Arc::downgrade(&host),
            storage: storage.clone(),
            automation: automation.clone(),
            execution_instance_id: host_id.as_str().to_owned(),
        });
    service.install_native_job_control(Arc::new(
        talos_service::automation::native_jobs::NativeJobService::new(
            native_job_controls,
            Arc::downgrade(&native_job_host),
        ),
    ))?;
    let management: Arc<dyn talos_service::ManagementHost> = host.clone();
    service.set_management_host(Arc::downgrade(&management));
    if let Some(session) = service
        .account
        .read_auth_session(
            &talos_service::Actor::regular("anonymous"),
            Default::default(),
        )?
        .session
    {
        service.resume_account_host(
            &talos_service::Actor::regular(session.user.user_id)
                .with_role(session.user.role)
                .with_user_type(session.user.r#type)
                .with_access_token(session.access_token),
        );
    }
    #[cfg(feature = "server")]
    if product_owner == talos_types::NodeProductOwner::Server {
        let management = Arc::downgrade(&management);
        node_service
            .as_ref()
            .expect("Server authority assembled")
            .set_directory_observer(Arc::new(move |actor| {
                if let Some(host) = management.upgrade() {
                    host.resume(actor);
                }
            }));
    }
    Ok(TalosApplicationRuntime {
        host,
        #[cfg(all(test, feature = "sync"))]
        sync,
        host_id,
        worker,
        worker_rpc,
        resource_stop,
        _resource_stop_guard: resource_stop_guard,
        worker_registration,
        worker_failures,
        config,
        service,
        event_service,
        auth_service,
        gateway,
        coordinator,
        #[cfg(feature = "server")]
        node_service,
        tool_registry,
        #[cfg(feature = "server")]
        session_store,
        #[cfg(feature = "server")]
        attachment_reader,
        #[cfg(feature = "server")]
        agent_control,
        #[cfg(feature = "server")]
        automation_control,
        #[cfg(feature = "server")]
        tool_image_store,
        automation,
        _native_job_host: native_job_host,
    })
}

struct ProductStores {
    storage_connection: Arc<talos_storage::SqliteStorage>,
    storage: Arc<dyn talos_storage::Storage>,
    profile_store: Arc<dyn talos_store::profile::ProfileStore>,
    #[cfg(feature = "sync")]
    file_store: Arc<talos_store::file::DatabaseFileStore>,
    host_id: String,
    session_store_impl: Arc<DatabaseSessionStore>,
    node_store: Arc<DatabaseAgentNodeStore>,
    project_store_impl: Arc<StorageProjectStore>,
    file_service: Arc<FileService>,
    event_service: Arc<TimelineEventService>,
}

async fn build_product_stores(config: &Config, migration_dir: &Path) -> TalosResult<ProductStores> {
    let storage_connection = application_storage::connect_application_storage(config).await?;
    let upgrade_report = talos_upgrade::run_startup_upgrades(talos_upgrade::StartupUpgradeParams {
        config,
        storage: &storage_connection,
        database_migration_dir: migration_dir,
    })
    .await?;
    tracing::info!(report = ?upgrade_report, "startup upgrades completed");
    let (storage_impl, profile_store, file_store) =
        application_storage::build_database_stores(&storage_connection);
    let storage: Arc<dyn talos_storage::Storage> = storage_impl;
    let host_id = super::host_identity::open(storage.as_ref()).await?;
    let session_store_impl = Arc::new(DatabaseSessionStore::new(storage.clone()).await?);
    let node_store = Arc::new(DatabaseAgentNodeStore::new(&storage_connection));
    let project_store_impl = Arc::new(StorageProjectStore::new(storage.clone()).await?);
    let file_service = Arc::new(FileService::new(
        file_store.clone(),
        project_store_impl.clone(),
        application_storage::build_file_operators(&config.storage.files)?,
        config.storage.files.clone(),
    )?);
    let agent_execution_store: Arc<dyn AgentExecutionStore> = session_store_impl.clone();
    let event_service = Arc::new(TimelineEventService::new(agent_execution_store.clone()));
    Ok(ProductStores {
        storage_connection,
        storage,
        profile_store,
        #[cfg(feature = "sync")]
        file_store,
        host_id,
        session_store_impl,
        node_store,
        project_store_impl,
        file_service,
        event_service,
    })
}

/// Assemble product-definition management without execution, registration or background work.
pub(crate) async fn build_product_runtime(
    config: Config,
    migration_dir: &Path,
) -> TalosResult<super::TalosProductRuntime> {
    let stores = build_product_stores(&config, migration_dir).await?;
    use talos_store::node::AgentNodeStore;
    let embedded = stores
        .node_store
        .upsert_embedded(
            talos_types::NodeStatus::Enabled,
            chrono::Utc::now().timestamp(),
        )
        .await?;
    let product_nodes = talos_service::ProductNodeAccess::computer(
        embedded.into_info(),
        Arc::new(agent_protocol::NodeAdmissionGates::default()),
    );
    let coordinator = Arc::new(CoordinatorImpl::new(
        Arc::new(RouterImpl::new()),
        stores.session_store_impl.clone(),
        stores.node_store,
    ));
    coordinator
        .register_environment(
            "embedded".into(),
            Arc::new(talos_service::WorkerEnvironmentService::new(
                Arc::new(std::sync::RwLock::new(config.clone())),
                std::env::var_os("HOME").map(std::path::PathBuf::from),
            )),
        )
        .await;
    let cloud = Arc::new(CloudAuthService::new(config.cloud.api_base_url.clone()));
    let auth_service: Arc<dyn AuthService> = cloud.clone();
    let users: Arc<dyn UserService> = if config.gateway.auth {
        cloud
    } else {
        Arc::new(OpenModeUserService)
    };
    let service = Arc::new(
        TalosService::new(
            coordinator.clone(),
            product_nodes,
            stores.session_store_impl.clone(),
            stores.session_store_impl.clone(),
            stores.project_store_impl,
            stores.profile_store,
            stores.file_service,
            stores.event_service.clone(),
            config.clone(),
            auth_service.clone(),
            users,
        )
        .with_product_owner(talos_types::NodeProductOwner::Computer),
    );
    service
        .account
        .set_cloud_session_cache(talos_auth::CloudSessionCache::for_talos_home(
            config.talos_paths().talos_home,
        ))?;
    service.set_coordinator_id(stores.host_id);
    service.install_session_automation_definitions(
        stores.session_store_impl.automation_store(),
        Arc::new(
            super::application_automation_control::ApplicationAutomationSkillResolver::new(
                Arc::downgrade(&coordinator),
                stores.session_store_impl,
            ),
        ),
    )?;
    let gateway: Arc<dyn Gateway> = Arc::new(ServiceGateway::new(service.clone()));
    Ok(super::TalosProductRuntime {
        config,
        service,
        event_service: stores.event_service,
        auth_service,
        gateway,
    })
}
