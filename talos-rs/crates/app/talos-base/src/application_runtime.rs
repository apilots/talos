use std::path::Path;
use std::sync::Arc;

use talos_agent::SessionHistoryReader;
use talos_agent_provider_kernel::KernelMemoryProgressStore;
use talos_auth::{AuthService, CloudAuthService, OpenModeUserService, UserService};
use talos_config::{Config, ConfigStore, YamlConfigStore};
use talos_coordinator::CoordinatorImpl;
use talos_error::{TalosError, TalosResult};
use talos_gateway::{Gateway, ServiceGateway};
use talos_llm_provider::LlmProviderRouter;
use talos_router::{Binding, MatchRule, Router, RouterImpl};
use talos_service::{
    resolve_inner_model_ref, AgentNodeService, FileService, InnerModelClient,
    SessionHistoryService, TalosService, TimelineEventService,
};
use talos_store::node::DatabaseAgentNodeStore;
use talos_store::project::{ProjectStore, StorageProjectStore};
use talos_store::session::{AgentExecutionStore, DatabaseSessionStore, SessionStore};

use crate::memory_progress::SessionMemoryProgress;
use talos_tools::{AgentControl, ToolImageStore};
use talos_types::{resolve_default_model_selection, resolve_model_catalog};
use talos_worker::{create_llm_provider, prepare_mcp, TalosWorker, TalosWorkerResources};

use super::{
    application_automation, application_storage, ApplicationAgentControl, ReloadableLlmProvider,
    TalosApplicationRuntime,
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
    let session_store_impl = Arc::new(DatabaseSessionStore::new(storage.clone()).await?);
    let node_store = Arc::new(DatabaseAgentNodeStore::new(
        storage_connection.pool().clone(),
    ));
    let node_service = AgentNodeService::new(node_store.clone());
    let project_store_impl = Arc::new(StorageProjectStore::new(storage.clone()).await?);
    let session_store: Arc<dyn SessionStore> = session_store_impl.clone();
    let project_store: Arc<dyn ProjectStore> = project_store_impl.clone();
    let file_service = Arc::new(FileService::new(
        file_store,
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

    let initial_llm_provider = create_llm_provider(&config)?;
    let reloadable_llm_provider = Arc::new(ReloadableLlmProvider::new(initial_llm_provider));
    let llm_router_config = worker.config();
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
    let plugin_manager_service = worker.plugin_manager();
    let memory_service = worker.memory_service();
    let user_workspace_memory = worker.user_workspace_memory();
    let mcp = worker.mcp_runtime();
    let credentials = worker.mcp_credentials();

    let coordinator = Arc::new(CoordinatorImpl::new(
        router,
        session_store.clone(),
        node_store,
    ));
    for (agent_id, agent) in worker.harnesses() {
        coordinator
            .register_agent("embedded".into(), agent_id.clone(), agent.clone())
            .await?;
    }
    node_service
        .register_embedded(!worker.harnesses().is_empty())
        .await?;

    let cloud_auth = Arc::new(CloudAuthService::new(config.cloud.api_base_url.clone()));
    let auth_service: Arc<dyn AuthService> = cloud_auth.clone();
    let user_service: Arc<dyn UserService> = if config.gateway.auth {
        cloud_auth
    } else {
        Arc::new(OpenModeUserService)
    };

    let config_store: Arc<dyn ConfigStore> = Arc::new(YamlConfigStore::with_env_path(
        config.source_path(),
        config.talos_paths().env_file,
    ));

    let provider_reloader = reloadable_llm_provider.clone();
    let provider_router_reloader = reloadable_llm_provider_router.clone();
    let provider_router_config_reloader = llm_router_config.clone();
    let inner_model_reloader = inner_model.clone();
    let plugins_for_reload = plugin_manager_service.clone();
    let mcp_for_reload = mcp.clone();
    let credentials_for_reload = credentials.clone();
    let config_reload_hook: talos_service::ConfigReloadHook = Arc::new(move |trigger| {
        if let talos_service::ConfigReloadTrigger::McpCredentials(server) = trigger {
            let mut candidate = provider_router_config_reloader
                .read()
                .expect("application config lock poisoned")
                .clone();
            let target = candidate
                .agent
                .kernel
                .mcp
                .servers
                .iter_mut()
                .find(|current| current.name == server.name)
                .ok_or_else(|| {
                    TalosError::new(
                        talos_error::ErrorCode::ResourceNotFound,
                        "The configured MCP server was not found.",
                    )
                })?;
            *target = *server;
            let mcp_update = prepare_mcp(
                &mcp_for_reload,
                &candidate.agent.kernel,
                plugins_for_reload.mcp_servers()?.servers,
                &credentials_for_reload,
            )?;
            let active = provider_router_config_reloader.clone();
            return Ok(talos_service::ConfigReloadPlan::new(
                candidate.clone(),
                move || {
                    *active.write().expect("application config lock poisoned") = candidate;
                    mcp_update.publish();
                },
            ));
        }

        let mut reloaded_config = provider_router_config_reloader
            .read()
            .expect("application config lock poisoned")
            .reload()?;
        if let talos_service::ConfigReloadTrigger::Updated(value) = trigger {
            match value {
                talos_types::ConfigValue::Llm(llm) => {
                    let mut llm = *llm;
                    talos_config::resolve_llm_update_for_runtime(&reloaded_config.llm, &mut llm);
                    reloaded_config.llm = llm;
                }
                talos_types::ConfigValue::WebSearch(web_search) => {
                    let mut web_search = *web_search;
                    talos_config::resolve_web_search_update_for_runtime(
                        &reloaded_config.agent.kernel.tools.web_search,
                        &mut web_search,
                    );
                    reloaded_config.agent.kernel.tools.web_search = web_search;
                }
            }
        }
        reloaded_config.validate_runtime()?;
        let reloaded_provider = create_llm_provider(&reloaded_config)?;
        let reloaded_default_selection = resolve_default_model_selection(&reloaded_config.llm)?;
        let reloaded_model_catalog = resolve_model_catalog(&reloaded_config.llm);
        let reloaded_inner_model_ref = resolve_inner_model_ref(
            reloaded_config.agent.inner_model.as_ref(),
            &reloaded_default_selection.model_ref,
            &reloaded_model_catalog,
        )?;
        let mut inner_provider_config = reloaded_config.clone();
        inner_provider_config.llm.provider = reloaded_inner_model_ref.provider.clone();
        inner_provider_config.llm.model = reloaded_inner_model_ref.id.clone();
        create_llm_provider(&inner_provider_config)?;
        let provider_reloader = provider_reloader.clone();
        let provider_router_reloader = provider_router_reloader.clone();
        let provider_router_config_reloader = provider_router_config_reloader.clone();
        let inner_model_reloader = inner_model_reloader.clone();
        let mcp_update = prepare_mcp(
            &mcp_for_reload,
            &reloaded_config.agent.kernel,
            plugins_for_reload.mcp_servers()?.servers,
            &credentials_for_reload,
        )?;
        let plugins_for_cleanup = plugins_for_reload.clone();
        Ok(talos_service::ConfigReloadPlan::new(
            reloaded_config.clone(),
            move || {
                provider_reloader.reload(reloaded_provider);
                *provider_router_config_reloader
                    .write()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) = reloaded_config;
                provider_router_reloader.reload();
                inner_model_reloader.reload(reloaded_inner_model_ref);
                mcp_update.publish();
                plugins_for_cleanup.recover_credentials();
            },
        ))
    });
    let service = Arc::new(
        TalosService::new(
            coordinator.clone(),
            session_store.clone(),
            session_store_impl.clone(),
            project_store,
            profile_store.clone(),
            file_service,
            event_service.clone(),
            config.clone(),
            plugin_manager_service.clone(),
            memory_service.clone(),
            config_store,
            model_catalog,
            auth_service.clone(),
            user_service,
            config_reload_hook,
        )
        .with_user_workspace_memory(user_workspace_memory, inner_model.clone()),
    );
    let agent_control: Arc<dyn AgentControl> =
        Arc::new(ApplicationAgentControl::new(service.clone()));
    worker.install_agent_control(agent_control.clone());

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
    Ok(TalosApplicationRuntime {
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
