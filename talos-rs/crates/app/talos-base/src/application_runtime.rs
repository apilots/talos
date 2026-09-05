use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, RwLock};

use talos_agent::{
    create_agent_harness, AgentDefinitionLoader, AgentUserContentKind, SessionHistoryReader,
};
use talos_agent_provider_kernel::{
    FileKernelSessionArtifactStore, KernelAgentProvider, KernelAgentProviderParams,
    KernelMemoryProgressStore, KernelSessionArtifactStore,
};
use talos_auth::{AuthService, CloudAuthService, OpenModeUserService, UserService};
use talos_config::{AgentProviderConfig, Config, ConfigStore, YamlConfigStore};
use talos_coordinator::CoordinatorImpl;
use talos_error::{TalosError, TalosResult};
use talos_gateway::{Gateway, ServiceGateway};
use talos_llm_provider::{LlmProviderRouter, ModelRef, ReloadableLlmProviderRouter};
use talos_router::{Binding, MatchRule, Router, RouterImpl};
use talos_service::{
    resolve_inner_model_ref, FileService, InnerModelClient, MemoryService, PluginManagerService,
    SessionHistoryService, TalosService, TimelineEventService,
};
use talos_store::memory::MemoryStore;
use talos_store::project::{ProjectStore, StorageProjectStore};
use talos_store::session::{AgentExecutionStore, DatabaseSessionStore, SessionStore};

use crate::memory_progress::SessionMemoryProgress;
use talos_tools::{
    register_core_local_tools, register_network_tools, register_subagent_tools, ToolRegistry,
};
use talos_tools_sandbox::{SandboxRuntimePaths, ToolSandbox};
use talos_types::{resolve_default_model_selection, resolve_model_catalog};

use super::{
    application_automation, application_storage, create_llm_provider, AgentSkillRootsResolver,
    ApplicationAgentControl, ReloadableLlmProvider, TalosApplicationRuntime,
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
    let agent_execution_store: Arc<dyn AgentExecutionStore> = session_store_impl.clone();
    let event_service = Arc::new(TimelineEventService::new(agent_execution_store.clone()));
    let session_history_reader: Arc<dyn SessionHistoryReader> =
        Arc::new(SessionHistoryService::new(event_service.clone()));
    let default_model_selection = resolve_default_model_selection(&config.llm)?;
    let agent_definitions = AgentDefinitionLoader::load(&config.agent)?;
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
    let sandbox = ToolSandbox::with_runtime_paths(sandbox_runtime_paths(&config));
    let tool_registry = Arc::new(ToolRegistry::new_with_sandbox(sandbox));
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
    let session_artifact_store: Arc<dyn KernelSessionArtifactStore> =
        Arc::new(FileKernelSessionArtifactStore);
    let memory_progress_store: Arc<dyn KernelMemoryProgressStore> =
        Arc::new(SessionMemoryProgress::new(session_store_impl.clone()));
    let memory_database = config.talos_paths().data_dir.join("memory-index.sqlite");
    let memory_store = Arc::new(MemoryStore::open(&memory_database).await?);
    let memory_service = MemoryService::new(config.clone(), memory_store)?;
    memory_service.rebuild_known_scopes().await?;
    tool_registry.set_memory_reader(Arc::new(memory_service.clone()));
    let user_workspace_memory = Arc::new(talos_service::UserWorkspaceMemoryService::new());
    tool_registry.set_user_workspace_memory_writer(user_workspace_memory.clone());

    let coordinator = Arc::new(CoordinatorImpl::new(router));
    let mut kernel_providers = HashMap::new();

    for definition in &agent_definitions {
        if !definition.enabled {
            continue;
        }
        let provider = match &definition.provider {
            AgentProviderConfig::Kernel { kernel } => {
                KernelAgentProvider::new(KernelAgentProviderParams {
                    agent_id: definition.id.clone(),
                    name: definition.name.clone(),
                    description: definition.description.clone(),
                    llm_provider_router: llm_provider_router.clone(),
                    tool_registry: tool_registry.clone(),
                    kernel_config: kernel.clone(),
                    skill_roots_resolver: skill_roots_resolver.clone(),
                    session_artifact_store: session_artifact_store.clone(),
                    session_history_reader: session_history_reader.clone(),
                    memory_progress_store: memory_progress_store.clone(),
                    dream_memory_store: Arc::new(memory_service.clone()),
                    user_workspace_memory_reader: user_workspace_memory.clone(),
                    attachment_reader: file_service.clone(),
                    rich_input: vec![
                        AgentUserContentKind::SkillSelection,
                        AgentUserContentKind::File,
                    ],
                })
                .map_err(talos_agent::AgentError::into_talos_error)?
            }
        };
        kernel_providers.insert(definition.id.clone(), provider.clone());
        let agent = create_agent_harness(definition.id.clone(), provider);
        coordinator
            .register_agent(definition.id.clone(), agent)
            .await?;
    }

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
    let config_reload_hook: talos_service::ConfigReloadHook = Arc::new(move |trigger| {
        let mut reloaded_config = Config::load()?;
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
        Ok(talos_service::ConfigReloadPlan::new(
            reloaded_config.clone(),
            move || {
                provider_reloader.reload(reloaded_provider);
                *provider_router_config_reloader
                    .write()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) = reloaded_config;
                provider_router_reloader.reload();
                inner_model_reloader.reload(reloaded_inner_model_ref);
            },
        ))
    });
    let service = Arc::new(
        TalosService::new(
            coordinator.clone(),
            session_store,
            session_store_impl.clone(),
            project_store,
            profile_store.clone(),
            file_service,
            event_service.clone(),
            config.clone(),
            memory_service.clone(),
            config_store,
            model_catalog,
            auth_service.clone(),
            user_service,
            config_reload_hook,
        )
        .with_user_workspace_memory(user_workspace_memory, inner_model.clone()),
    );
    if config.agent.kernel.tools.enable_collaboration_tools {
        register_subagent_tools(tool_registry.as_ref());
        tool_registry.set_agent_control(Arc::new(ApplicationAgentControl::new(service.clone())));
    }

    let gateway: Arc<dyn Gateway> = Arc::new(ServiceGateway::new(service.clone()));
    let dream_memory_maintenance = crate::dream_memory_maintenance::DreamMemoryMaintenance::new(
        session_store_impl,
        service.clone(),
        kernel_providers,
        inner_model,
        config.workspaces.clone(),
        config.talos_paths(),
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

    Ok(TalosApplicationRuntime {
        config,
        service,
        event_service,
        auth_service,
        gateway,
        coordinator,
        tool_registry,
        automation,
    })
}

fn sandbox_runtime_paths(config: &Config) -> SandboxRuntimePaths {
    let configured = config
        .agent
        .kernel
        .tools
        .sandbox_policy
        .platform
        .linux_helper_executable
        .clone();
    let linux_helper_executable = configured.or_else(|| {
        let mut candidate = std::env::current_exe().ok()?;
        candidate.set_file_name("talos-linux-sandbox-launcher");
        candidate.is_file().then_some(candidate)
    });
    SandboxRuntimePaths {
        macos_sandbox_executable: None,
        linux_helper_executable,
    }
}
