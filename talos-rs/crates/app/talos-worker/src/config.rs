//! Atomic node-local configuration preparation and publication.
use std::sync::{Arc, RwLock};
use talos_config::{Config, ConfigStore, YamlConfigStore};
use talos_error::{ErrorCode, TalosError};
use talos_llm_provider::ReloadableLlmProviderRouter;
use talos_service::{
    ConfigReloadHook, ConfigReloadPlan, ConfigReloadTrigger, ConfigService, McpCredentialStore,
    PluginManagerService,
};
use talos_types::ConfigValue;

pub(crate) fn service(
    active: Arc<RwLock<Config>>,
    router: Arc<ReloadableLlmProviderRouter>,
    plugins: PluginManagerService,
    mcp: Arc<talos_mcp::TalosMcpRuntime>,
    credentials: McpCredentialStore,
    providers: Vec<(String, talos_agent_provider_kernel::KernelAgentProvider)>,
) -> ConfigService {
    let initial = active.read().expect("Worker config lock poisoned").clone();
    let store: Arc<dyn ConfigStore> = Arc::new(YamlConfigStore::with_env_path(
        initial.source_path(),
        initial.talos_paths().env_file,
    ));
    let gate = plugins.execution_gate();
    let hook: ConfigReloadHook = Arc::new(move |trigger| {
        let current = active.read().expect("Worker config lock poisoned").clone();
        let mut candidate = match &trigger {
            ConfigReloadTrigger::McpCredentials(_) => current.clone(),
            _ => current.reload()?,
        };
        match trigger {
            ConfigReloadTrigger::McpCredentials(server) => {
                let target = candidate
                    .agent
                    .kernel
                    .mcp
                    .servers
                    .iter_mut()
                    .find(|entry| entry.name == server.name)
                    .ok_or_else(|| {
                        TalosError::new(
                            ErrorCode::ResourceNotFound,
                            "The configured MCP server was not found.",
                        )
                    })?;
                *target = *server;
            }
            ConfigReloadTrigger::Updated(ConfigValue::Llm(value)) => {
                let mut value = *value;
                talos_config::resolve_llm_update_for_runtime(&current.llm, &mut value);
                candidate.llm = value;
            }
            ConfigReloadTrigger::Updated(ConfigValue::WebSearch(value)) => {
                let mut value = *value;
                talos_config::resolve_web_search_update_for_runtime(
                    &current.agent.kernel.tools.web_search,
                    &mut value,
                );
                candidate.agent.kernel.tools.web_search = value;
            }
            ConfigReloadTrigger::Explicit => {}
        }
        candidate.validate_runtime()?;
        talos_service::MemoryService::validate_config(&candidate)?;
        let definitions = talos_agent::AgentDefinitionLoader::load(&candidate.agent)?;
        let enabled = definitions
            .into_iter()
            .filter(|definition| definition.enabled)
            .collect::<Vec<_>>();
        if enabled.len() != providers.len()
            || enabled
                .iter()
                .any(|definition| !providers.iter().any(|(id, _)| id == &definition.id))
        {
            return Err(TalosError::new(
                ErrorCode::Conflict,
                "Agent deployment changes require restarting the Worker",
            ));
        }
        let kernel_updates = enabled
            .into_iter()
            .map(|definition| {
                let provider = providers
                    .iter()
                    .find(|(id, _)| id == &definition.id)
                    .expect("validated deployment")
                    .1
                    .clone();
                let talos_config::AgentProviderConfig::Kernel { kernel } = definition.provider;
                (provider, kernel)
            })
            .collect::<Vec<_>>();
        crate::create_llm_provider(&candidate)?;
        let update = crate::prepare_mcp(
            &mcp,
            &candidate.agent.kernel,
            plugins.mcp_servers()?.servers,
            &credentials,
        )?;
        let config = active.clone();
        let router = router.clone();
        let plugins = plugins.clone();
        Ok(ConfigReloadPlan::new(move || {
            *config.write().expect("Worker config lock poisoned") = candidate;
            router.reload();
            for (provider, kernel) in kernel_updates {
                provider.reload_kernel_config(kernel);
            }
            update.publish();
            plugins.recover_credentials();
        }))
    });
    ConfigService::new(store, gate, hook)
}
