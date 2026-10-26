use std::sync::{Arc, RwLock};

use agent_protocol::{AgentHarness, Deployment};
use talos_agent::{
    create_agent_harness, AgentDefinitionLoader, AgentUserContentKind, AttachmentReader,
    SessionHistoryReader,
};
use talos_agent_provider_kernel::{
    FileKernelSessionArtifactStore, KernelAgentProvider, KernelAgentProviderParams,
    KernelMemoryProgressStore, KernelSessionArtifactStore, SkillRootsResolver,
};
use talos_config::{AgentKernelConfig, AgentProviderConfig, Config};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_llm_provider::{LlmProviderRouter, ModelRef, ReloadableLlmProviderRouter};
use talos_service::{
    McpCredentialStore, MemoryService, PluginManagerService, UserWorkspaceMemoryService,
};
use talos_store::memory::MemoryStore;
use talos_tools::{
    register_core_local_tools, register_network_tools, register_subagent_tools, AgentControl,
    ToolImageStore, ToolRegistry,
};
use talos_tools_sandbox::{SandboxRuntimePaths, ToolSandbox};

use crate::{create_llm_provider, prepare_mcp};

#[derive(Clone)]
pub struct TalosWorkerResources {
    pub session_history_reader: Arc<dyn SessionHistoryReader>,
    pub memory_progress_store: Arc<dyn KernelMemoryProgressStore>,
    pub attachment_reader: Arc<dyn AttachmentReader>,
    pub tool_image_store: Arc<dyn ToolImageStore>,
    pub agent_control: Option<Arc<dyn AgentControl>>,
}

/// One Agent execution runtime shared by embedded and standalone hosts.
#[derive(Clone)]
pub struct TalosWorker {
    pub(crate) resource_bindings: crate::resource_binding::ResourceBindings,
    system_info: talos_types::NodeSystemInfo,
    harnesses: Vec<(String, Arc<dyn AgentHarness>)>,
    config: Arc<RwLock<Config>>,
    provider_router: Arc<ReloadableLlmProviderRouter>,
    tool_registry: Arc<ToolRegistry>,
    plugin_manager: PluginManagerService,
    memory_service: MemoryService,
    user_workspace_memory: Arc<UserWorkspaceMemoryService>,
    mcp: Arc<talos_mcp::TalosMcpRuntime>,
    browser: Arc<talos_browser::BrowserRuntime>,
    mcp_credentials: McpCredentialStore,
    config_service: talos_service::ConfigService,
    background_tasks: Arc<talos_guard_task::GuardTasks>,
}

impl TalosWorker {
    pub async fn build(config: Config, resources: TalosWorkerResources) -> TalosResult<Self> {
        Self::build_with_browser_host(config, resources, talos_types::BrowserHostKind::Native).await
    }

    /// Host capability is selected by the composition root, never model arguments.
    pub async fn build_with_browser_host(
        config: Config,
        resources: TalosWorkerResources,
        host: talos_types::BrowserHostKind,
    ) -> TalosResult<Self> {
        let active_config = Arc::new(RwLock::new(config.clone()));
        let provider_config = active_config.clone();
        let provider_router = Arc::new(ReloadableLlmProviderRouter::new(Arc::new(
            move |model_ref: &ModelRef| {
                let mut config = provider_config
                    .read()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .clone();
                config.llm.provider = model_ref.provider.trim().to_ascii_lowercase();
                config.llm.model = model_ref.id.clone();
                create_llm_provider(&config)
            },
        )));
        let llm_provider_router: Arc<dyn LlmProviderRouter> = provider_router.clone();

        let sandbox = ToolSandbox::with_runtime_paths(sandbox_runtime_paths(&config));
        let tool_registry = Arc::new(ToolRegistry::new_with_sandbox(sandbox));
        tool_registry.set_image_store(resources.tool_image_store);
        register_core_local_tools(tool_registry.as_ref());
        register_network_tools(tool_registry.as_ref());
        let collaboration_tools_enabled = config.agent.kernel.tools.enable_collaboration_tools;
        if collaboration_tools_enabled {
            register_subagent_tools(tool_registry.as_ref());
            if let Some(agent_control) = resources.agent_control {
                tool_registry.set_agent_control(agent_control);
            }
        }

        let manager = talos_plugin::PluginManager::with_state_store(
            config.agent.kernel.plugins.clone(),
            talos_plugin::PluginStateStore::new(config.talos_paths().talos_home.join("plugins")),
        )
        .with_max_skill_md_bytes(config.agent.kernel.skills.max_skill_md_bytes);
        let snapshot = std::env::var_os("TALOS_RESOURCE_DIR")
            .map(std::path::PathBuf::from)
            .map(|root| root.join("plugins/apilots-mcps"))
            .filter(|root| root.join("source.json").is_file());
        match manager.provision_official_browser(snapshot.as_deref()) {
            Ok(talos_plugin::OfficialBrowserProvision::Ready) => {}
            Ok(talos_plugin::OfficialBrowserProvision::NameConflict) => {
                tracing::warn!(
                    "Official Browser unavailable: an existing plugin uses talos_browser."
                );
            }
            Ok(talos_plugin::OfficialBrowserProvision::Unavailable) => {
                tracing::warn!("Official Browser unavailable: install its official source or supply a pinned MCP snapshot.");
            }
            Err(error) => talos_logging::report_error(&error),
        }
        let enabled_manager = manager.clone();
        let browser = talos_browser::BrowserRuntime::new(
            config.talos_paths().talos_home.clone(),
            host,
            Arc::new(move || enabled_manager.official_browser_enabled()),
        )?;
        if host == talos_types::BrowserHostKind::Docker {
            let resources = std::env::var_os("TALOS_RESOURCE_DIR")
                .map(std::path::PathBuf::from)
                .ok_or_else(|| TalosError::internal("Docker Browser resources are missing."))?;
            browser.configure_managed_resources(&resources.join("browser"))?;
        }
        #[cfg(unix)]
        let socket = Some(browser.start_ipc(&config.talos_paths().talos_home).await?);
        #[cfg(not(unix))]
        let socket = None;
        let mcp = Arc::new(talos_mcp::TalosMcpRuntime::with_builtin_browser_socket(
            socket,
        ));
        tool_registry.register_mcp_server(Arc::new(
            talos_tools::McpConnectionManager::from_runtime(mcp.clone())
                .with_browser(browser.clone()),
        ));
        let mcp_credentials =
            McpCredentialStore::new(config.talos_paths().talos_home.join("mcp-credentials"));
        let mcp_for_changes = mcp.clone();
        let credentials_for_changes = mcp_credentials.clone();
        let config_for_changes = active_config.clone();
        let plugin_manager = PluginManagerService::with_manager(manager)
            .with_browser(browser.clone())
            .with_mcp(
                mcp.clone(),
                Arc::new(move |plugins| {
                    let current = config_for_changes
                        .read()
                        .expect("worker config lock poisoned");
                    let prepared = prepare_mcp(
                        &mcp_for_changes,
                        &current.agent.kernel,
                        plugins,
                        &credentials_for_changes,
                    )?;
                    Ok(Box::new(move || prepared.publish()))
                }),
                mcp_credentials.clone(),
            );
        let skill_roots_resolver = Arc::new(WorkerSkillRootsResolver {
            plugins: plugin_manager.clone(),
        });

        let memory_store = Arc::new(
            MemoryStore::open(&config.talos_paths().data_dir.join("memory-index.sqlite")).await?,
        );
        let memory_service = MemoryService::new(config.clone(), memory_store)?
            .with_live_config(active_config.clone());
        memory_service.rebuild_known_scopes().await?;
        tool_registry.set_memory_reader(Arc::new(memory_service.clone()));
        let user_workspace_memory = Arc::new(UserWorkspaceMemoryService::new());
        tool_registry.set_user_workspace_memory_writer(user_workspace_memory.clone());

        let session_artifact_store: Arc<dyn KernelSessionArtifactStore> =
            Arc::new(FileKernelSessionArtifactStore);
        let resource_bindings = crate::resource_binding::ResourceBindings::new();
        let mut harnesses = Vec::new();
        let mut providers = Vec::new();
        for definition in AgentDefinitionLoader::load(&config.agent)? {
            if !definition.enabled {
                continue;
            }
            let provider = match definition.provider {
                AgentProviderConfig::Kernel { kernel } => {
                    crate::output_logs::validate_policy(&config, &kernel)?;
                    KernelAgentProvider::new(KernelAgentProviderParams {
                        agent_id: definition.id.clone(),
                        name: definition.name,
                        description: definition.description,
                        llm_provider_router: llm_provider_router.clone(),
                        tool_registry: tool_registry.clone(),
                        kernel_config: kernel,
                        skill_roots_resolver: skill_roots_resolver.clone(),
                        session_artifact_store: session_artifact_store.clone(),
                        session_history_reader: resources.session_history_reader.clone(),
                        memory_progress_store: resources.memory_progress_store.clone(),
                        dream_memory_store: Arc::new(memory_service.clone()),
                        user_workspace_memory_reader: user_workspace_memory.clone(),
                        attachment_reader: resources.attachment_reader.clone(),
                        rich_input: vec![
                            AgentUserContentKind::SkillSelection,
                            AgentUserContentKind::File,
                        ],
                    })?
                    .with_resource_resolver(resource_bindings.resolver(tool_registry.clone()))
                }
            };
            providers.push((definition.id.clone(), provider.clone()));
            harnesses.push((
                definition.id.clone(),
                Arc::new(crate::harness::WorkerHarness {
                    provider: provider.clone(),
                    execution: create_agent_harness(definition.id, provider),
                    workspace_memory: user_workspace_memory.clone(),
                    execution_gate: plugin_manager.execution_gate(),
                    browser: browser.clone(),
                    config: active_config.clone(),
                }) as Arc<dyn AgentHarness>,
            ));
        }
        if harnesses.is_empty() {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Talos Worker has no enabled Agent deployment",
            ));
        }

        let system_info = talos_system_info::collect().await?;
        let background_tasks = Arc::new(talos_guard_task::GuardTasks::default());
        let output_logs_changed = background_tasks.register(
            talos_guard_task::GuardTaskSpec {
                name: "bash_output_cleanup",
                interval: std::time::Duration::from_secs(
                    config
                        .agent
                        .kernel
                        .tools
                        .bash
                        .output_log
                        .cleanup_interval_secs,
                ),
                run_on_start: true,
            },
            crate::output_logs::OutputLogMaintenance::new(active_config.clone()),
        )?;
        background_tasks.register(
            talos_guard_task::GuardTaskSpec {
                name: "official_browser_bootstrap",
                interval: std::time::Duration::from_secs(300),
                run_on_start: true,
            },
            OfficialBrowserBootstrap(plugin_manager.clone()),
        )?;
        let config_service = crate::config::service(
            active_config.clone(),
            provider_router.clone(),
            plugin_manager.clone(),
            mcp.clone(),
            mcp_credentials.clone(),
            providers,
            output_logs_changed.clone(),
        );
        Ok(Self {
            background_tasks,
            system_info,
            resource_bindings,
            harnesses,
            config: active_config,
            provider_router,
            tool_registry,
            plugin_manager,
            memory_service,
            user_workspace_memory,
            mcp,
            browser,
            mcp_credentials,
            config_service,
        })
    }

    /// The same node-local environment is used by embedded and network hosts.
    pub fn environment(&self) -> Arc<dyn agent_protocol::WorkerEnvironment> {
        Arc::new(
            talos_service::WorkerEnvironmentService::new(
                self.config.clone(),
                std::env::var_os("HOME").map(std::path::PathBuf::from),
            )
            .with_user_workspace_memory(self.user_workspace_memory.clone())
            .with_memory_service(self.memory_service.clone())
            .with_plugin_manager(self.plugin_manager.clone())
            .with_config_service(self.config_service.clone())
            .with_browser(self.browser.clone()),
        )
    }

    pub fn browser(&self) -> Arc<talos_browser::BrowserRuntime> {
        self.browser.clone()
    }

    pub fn config_service(&self) -> talos_service::ConfigService {
        self.config_service.clone()
    }

    pub fn harnesses(&self) -> &[(String, Arc<dyn AgentHarness>)] {
        &self.harnesses
    }

    pub fn system_info(&self) -> &talos_types::NodeSystemInfo {
        &self.system_info
    }

    pub fn deployments(&self) -> Vec<Deployment> {
        self.harnesses
            .iter()
            .map(|(agent_id, _)| Deployment {
                agent_id: agent_id.clone(),
            })
            .collect()
    }

    pub fn config(&self) -> Arc<RwLock<Config>> {
        self.config.clone()
    }

    pub fn provider_router(&self) -> Arc<ReloadableLlmProviderRouter> {
        self.provider_router.clone()
    }

    pub fn tool_registry(&self) -> Arc<ToolRegistry> {
        self.tool_registry.clone()
    }

    pub fn plugin_manager(&self) -> PluginManagerService {
        self.plugin_manager.clone()
    }

    pub fn activate_mcp(&self) -> TalosResult<()> {
        let config = self
            .config
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let prepared = prepare_mcp(
            &self.mcp,
            &config.agent.kernel,
            self.plugin_manager.mcp_servers()?.servers,
            &self.mcp_credentials,
        )?;
        drop(config);
        self.plugin_manager.recover_credentials();
        prepared.publish();
        Ok(())
    }

    pub async fn shutdown(&self) -> TalosResult<()> {
        let mut result = self
            .background_tasks
            .shutdown()
            .await
            .inspect_err(talos_logging::report_error);
        for (_, harness) in &self.harnesses {
            let stopped = harness
                .shutdown(std::time::Duration::from_secs(5))
                .await
                .inspect_err(talos_logging::report_error);
            result = result.and(stopped);
        }
        let browser = self
            .browser
            .shutdown()
            .await
            .inspect_err(talos_logging::report_error);
        result
            .and(browser)
            .and(self.plugin_manager.shutdown_mcp().await)
    }
}

#[derive(Clone)]
struct WorkerSkillRootsResolver {
    plugins: PluginManagerService,
}

impl SkillRootsResolver for WorkerSkillRootsResolver {
    fn skill_roots(&self, config: &AgentKernelConfig) -> TalosResult<talos_skills::SkillRoots> {
        self.plugins.skill_roots(&config.skills)
    }
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

struct OfficialBrowserBootstrap(PluginManagerService);
impl talos_guard_task::GuardTask for OfficialBrowserBootstrap {
    async fn run(&self, context: talos_guard_task::GuardTaskContext) -> TalosResult<()> {
        tokio::select! { _=context.cancelled()=>Ok(()), result=self.0.bootstrap_official_browser()=>result }
    }
}
