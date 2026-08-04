use std::path::Path;
use std::sync::{Arc, RwLock};

use talos_agent::{AgentDefinitionLoader, AgentHarness, AgentHarnessFactory};
use talos_auth::{AuthService, CloudAuthService, OpenModeUserService, UserService};
use talos_config::{Config, LlmConfigManager, LlmConfigService, YamlLlmConfigStore};
use talos_coordinator::{Coordinator, CoordinatorImpl, SessionRuntime};
use talos_error::{TalosError, TalosResult};
use talos_gateway::{Gateway, ServiceGateway};
use talos_llm_provider::{LlmProviderRouter, ModelRef, ReloadableLlmProviderRouter};
use talos_router::{Binding, MatchRule, Router, RouterImpl};
use talos_service::{
    AgentDispatchTurnExecutor, FileService, MemoryService, PluginManagerService, TalosService,
    TimelineEventRetentionConfig, TimelineEventService,
};
use talos_store::memory::MemoryStore;
use talos_store::project::{ProjectStore, StorageProjectStore};
use talos_store::session::{
    ActiveTurnStore, DatabaseSessionStore, FileSessionArtifactStore, MessageIdempotencyStore,
    SessionArtifactStore, SessionStore, TimelineEventStore,
};
use talos_tools::{
    register_core_local_tools, register_network_tools, register_subagent_tools, ToolRegistry,
};
use talos_types::{resolve_default_model_selection, resolve_model_catalog, AgentDispatchRuntime};
use tracing::error;

use super::{
    application_automation, application_storage, create_llm_provider, AgentSkillRootsResolver,
    ApplicationAgentControl, ReloadableLlmProvider, TalosApplicationRuntime,
};

pub(crate) async fn build_application_runtime_with_migration_dir(
    config: Config,
    migration_dir: &Path,
) -> TalosResult<TalosApplicationRuntime> {
    let (storage_impl, profile_store, file_store) =
        application_storage::build_application_storage(&config, migration_dir).await?;
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
        .ok_or_else(|| {
            TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                "no enabled agent definitions".to_string(),
            )
        })?;
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
    super::register_mcp_tools(
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
        register_subagent_tools(tool_registry.as_ref());
        tool_registry.set_agent_control(Arc::new(ApplicationAgentControl::new(service.clone())));
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
