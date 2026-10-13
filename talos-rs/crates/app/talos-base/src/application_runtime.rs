use std::path::Path;
use std::sync::Arc;

use talos_agent::SessionHistoryReader;
use talos_agent_provider_kernel::KernelMemoryProgressStore;
use talos_auth::{AuthService, CloudAuthService, OpenModeUserService, UserService};
use talos_config::Config;
use talos_coordinator::CoordinatorImpl;
use talos_error::TalosResult;
use talos_gateway::{Gateway, ServiceGateway};
use talos_llm_provider::LlmProviderRouter;
use talos_router::{Binding, MatchRule, Router, RouterImpl};
use talos_service::{
    AgentNodeService, FileService, InnerModelClient, SessionHistoryService, TalosService,
    TimelineEventService,
};
use talos_store::node::DatabaseAgentNodeStore;
use talos_store::project::{ProjectStore, StorageProjectStore};
use talos_store::session::{AgentExecutionStore, DatabaseSessionStore, SessionStore};

use crate::memory_progress::SessionMemoryProgress;
use talos_tools::{AgentControl, ToolImageStore};
use talos_types::{resolve_default_model_selection, resolve_model_catalog};
use talos_worker::{TalosWorker, TalosWorkerResources};

use super::{
    application_automation, application_storage, ApplicationAgentControl, TalosApplicationRuntime,
};

pub(crate) async fn build_application_runtime_with_migration_dir(
    config: Config,
    migration_dir: &Path,
) -> TalosResult<TalosApplicationRuntime> {
    let storage_connection = application_storage::connect_application_storage(&config).await?;
    let upgrade_report = talos_upgrade::run_startup_upgrades(talos_upgrade::StartupUpgradeParams {
        config: &config,
        database_pool: storage_connection.pool(),
        database_migration_dir: migration_dir,
    })
    .await?;
    tracing::info!(report = ?upgrade_report, "startup upgrades completed");
    let (storage_impl, profile_store, file_store) =
        application_storage::build_database_stores(&storage_connection);
    let storage: Arc<dyn talos_storage::Storage> = storage_impl;
    let sync = Arc::new(
        talos_sync::TalosSync::open(
            storage.clone(),
            talos_sync::SyncOptions {
                hub_endpoint: config.sync.hub_endpoint.clone(),
            },
        )
        .await?,
    );
    let session_store_impl = Arc::new(DatabaseSessionStore::new(storage.clone()).await?);
    let node_store = Arc::new(DatabaseAgentNodeStore::new(
        storage_connection.pool().clone(),
    ));
    let node_service = AgentNodeService::new(node_store.clone());
    let project_store_impl = Arc::new(StorageProjectStore::new(storage.clone()).await?);
    let session_store: Arc<dyn SessionStore> = session_store_impl.clone();
    let project_store: Arc<dyn ProjectStore> = project_store_impl.clone();
    let file_service = Arc::new(FileService::new(
        file_store.clone(),
        project_store.clone(),
        application_storage::build_file_operators(&config.storage.files)?,
        config.storage.files.clone(),
    )?);
    let attachment_reader: Arc<dyn talos_agent::AttachmentReader> = file_service.clone();
    file_service.reconcile_incomplete().await?;
    let agent_execution_store: Arc<dyn AgentExecutionStore> = session_store_impl.clone();
    let event_service = Arc::new(TimelineEventService::new(agent_execution_store.clone()));
    let session_history_reader: Arc<dyn SessionHistoryReader> =
        Arc::new(SessionHistoryService::new(event_service.clone()));
    let memory_progress_store: Arc<dyn KernelMemoryProgressStore> =
        Arc::new(SessionMemoryProgress::new(session_store_impl.clone()));
    let tool_image_store: Arc<dyn ToolImageStore> =
        Arc::new(super::tool_images::ToolImages(file_service.clone()));
    let worker = TalosWorker::build(
        config.clone(),
        TalosWorkerResources {
            session_history_reader: session_history_reader.clone(),
            memory_progress_store: memory_progress_store.clone(),
            attachment_reader: file_service.clone(),
            tool_image_store: tool_image_store.clone(),
            agent_control: None,
        },
    )
    .await?;
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
    let (worker_rpc, local_auth) = worker.start_local_rpc().await?;
    coordinator
        .register_environment(
            "embedded".into(),
            worker
                .local_rpc_environment(&worker_rpc, local_auth.clone())
                .await?,
        )
        .await;
    for (agent_id, agent) in worker
        .local_rpc_harnesses(&worker_rpc, &local_auth, sync.peer_id())
        .await?
    {
        coordinator
            .register_agent("embedded".into(), agent_id, agent)
            .await?;
    }
    node_service
        .register_embedded(!worker.harnesses().is_empty())
        .await?;

    // Upgrade only idle, locally owned Sessions before publishing a cloud entry.
    // Existing Worker markers (including a newer remote coordinator) always win.
    for record in session_store
        .list_session_records(&talos_store::session::SessionQuery {
            execution_node_id: Some("embedded".into()),
            ..Default::default()
        })
        .await?
    {
        if agent_execution_store
            .get_agent_active_turn(&record.id)
            .await?
            .is_some()
        {
            continue;
        }
        let Some(history_cursor) = session_store.get_history_cursor(&record.id).await? else {
            continue;
        };
        if history_cursor == Default::default() {
            continue;
        }
        let namespace = session_store
            .get_workspace_user_id(&record.id)
            .await?
            .unwrap_or_else(|| record.user_id.clone());
        let context = agent_protocol::AgentSessionContext {
            agent_id: record.agent_id.clone(),
            session_id: record.id.clone(),
            workspace: config.resolve_workspace(&record.workspace_id, &namespace)?,
        };
        let harness =
            talos_coordinator::AgentCoordinator::session_harness(&*coordinator, &context).await?;
        let state = harness
            .read_session_execution_state(context.clone())
            .await?
            .ok_or_else(|| {
                talos_error::TalosError::internal("Worker execution state is unavailable")
            })?;
        if state.coordinator_id.is_none() {
            harness
                .commit_session_execution_state(agent_protocol::CommitSessionExecutionStateParams {
                    session: context,
                    identity: history_cursor
                        .completed_turn_id
                        .as_ref()
                        .map(|turn| talos_types::AgentTurnRef::new(&record.id, turn)),
                    history_cursor,
                    visibility: agent_protocol::SessionStateVisibility::Local,
                })
                .await?;
        }
    }

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
            node_service.clone(),
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
        .with_user_workspace_memory_reflection(inner_model.clone()),
    );
    service
        .account
        .set_cloud_session_cache(talos_auth::CloudSessionCache::for_talos_home(
            config.talos_paths().talos_home,
        ))?;
    service.set_coordinator_id(sync.peer_id().into());
    let sync_stores = Arc::new(super::application_sync::ApplicationSyncStores {
        storage: storage.clone(),
        sessions: session_store_impl.clone(),
        projects: project_store_impl,
        nodes: node_store,
        files: file_store,
        handles: Default::default(),
        credentials: Default::default(),
        connections: Default::default(),
    });
    let agent_control: Arc<dyn AgentControl> =
        Arc::new(ApplicationAgentControl::new(service.clone()));
    let (resource_rpc, resource_client) = super::local_resource_rpc::LocalResourceRpc::start(
        local_auth,
        super::agent_node_rpc::NodeSessionDataRpc::new(
            node_service.clone(),
            session_store.clone(),
            session_history_reader.clone(),
            attachment_reader.clone(),
            memory_progress_store.clone(),
        ),
        super::agent_node_rpc::NodeToolImageRpc::new(
            node_service.clone(),
            tool_image_store.clone(),
            config.storage.files.limits.max_upload_bytes,
        ),
        agent_control.clone(),
    )
    .await?;
    worker.use_local_resource_rpc(resource_client);

    let gateway: Arc<dyn Gateway> = Arc::new(ServiceGateway::new(service.clone()));
    let dream_memory_maintenance = crate::dream_memory_maintenance::DreamMemoryMaintenance::new(
        session_store_impl,
        service.clone(),
        coordinator.clone(),
        inner_model,
        &config,
        config.automation.dream_memory_maintenance.clone(),
    );
    let automation = application_automation::start_maintenance(
        profile_store,
        config.automation.profile_usage_retention.clone(),
        dream_memory_maintenance,
        config.automation.dream_memory_maintenance.clone(),
        talos_config::parse_timezone(&config.timezone)?,
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
    let sync_host = Arc::new(super::application_sync::ApplicationSyncHost {
        config: config.clone(),
        sync: sync.clone(),
        sync_stores: sync_stores.clone(),
        auth_service: auth_service.clone(),
        service: service.clone(),
        coordinator: coordinator.clone(),
        event_service: event_service.clone(),
        data: super::NodeSessionDataRpc::new(
            node_service.clone(),
            session_store.clone(),
            session_history_reader.clone(),
            attachment_reader.clone(),
            memory_progress_store.clone(),
        ),
        images: super::NodeToolImageRpc::new(
            node_service.clone(),
            tool_image_store.clone(),
            config.storage.files.limits.max_upload_bytes,
        ),
        control: agent_control.clone(),
        worker: worker.clone(),
        worker_rpc: worker_rpc.clone(),
        worker_registration: worker_registration.clone(),
        worker_failures: worker_failures.clone(),
        trusted_local_host: std::env::var_os("TALOS_DESKTOP_HOSTNAME").is_some()
            || (config.mode == talos_config::DeploymentMode::Local
                && config.agent_node.control_plane.is_none()
                && matches!(
                    config.gateway.host.as_str(),
                    "127.0.0.1" | "::1" | "localhost"
                )),
        local_owner: Default::default(),
        tokens: Default::default(),
        lifecycle: Default::default(),
        failures: Default::default(),
    });
    sync_host.install_peer_access();
    for binding in sync.saved_bindings().await? {
        sync_host.install_capture(binding)?;
    }
    let management: Arc<dyn talos_service::ManagementSync> = sync_host.clone();
    service.set_sync_management(Arc::downgrade(&management));
    if config.agent_node.control_plane.is_some() {
        node_service.set_account_sync(Arc::downgrade(&management));
    }
    Ok(TalosApplicationRuntime {
        sync_host,
        sync,
        sync_stores,
        worker,
        worker_rpc,
        resource_rpc,
        worker_registration,
        worker_failures,
        config,
        service,
        event_service,
        auth_service,
        gateway,
        coordinator,
        node_service,
        tool_registry,
        session_store,
        session_history_reader,
        attachment_reader,
        memory_progress_store,
        agent_control,
        tool_image_store,
        automation,
    })
}
