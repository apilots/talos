use talos_config::{AgentKernelConfig, PluginsRuntimeConfig, SkillsRuntimeConfig};
use talos_error::TalosResult;
use talos_plugin::PluginManager;
use talos_skills::SkillRoots;
use talos_types::{
    McpServerRuntimeDescriptor, PluginDisableRequest, PluginDisableResponse, PluginEnableRequest,
    PluginEnableResponse, PluginInstallRequest, PluginInstallResponse, PluginListRequest,
    PluginListResponse, PluginReadRequest, PluginReadResponse, PluginSearchRequest,
    PluginSearchResponse, PluginSourceAddRequest, PluginSourceAddResponse, PluginSourceListRequest,
    PluginSourceListResponse, PluginSourceRefreshRequest, PluginSourceRefreshResponse,
    PluginSourceRemoveRequest, PluginSourceRemoveResponse, PluginUninstallRequest,
    PluginUninstallResponse, PluginUpdateRequest, PluginUpdateResponse,
};

#[derive(Clone)]
pub struct PluginManagerService {
    manager: PluginManager,
}

impl PluginManagerService {
    pub fn new(config: PluginsRuntimeConfig, skills: SkillsRuntimeConfig) -> Self {
        Self {
            manager: PluginManager::new(config).with_max_skill_md_bytes(skills.max_skill_md_bytes),
        }
    }

    pub fn with_manager(manager: PluginManager) -> Self {
        Self { manager }
    }

    pub fn list_sources(
        &self,
        request: PluginSourceListRequest,
    ) -> TalosResult<PluginSourceListResponse> {
        self.manager.list_sources(request)
    }

    pub fn add_source(
        &self,
        request: PluginSourceAddRequest,
    ) -> TalosResult<PluginSourceAddResponse> {
        self.manager.add_source(request)
    }

    pub fn refresh_source(
        &self,
        request: PluginSourceRefreshRequest,
    ) -> TalosResult<PluginSourceRefreshResponse> {
        self.manager.refresh_source(request)
    }

    pub fn remove_source(
        &self,
        request: PluginSourceRemoveRequest,
    ) -> TalosResult<PluginSourceRemoveResponse> {
        self.manager.remove_source(request)
    }

    pub fn search(&self, request: PluginSearchRequest) -> TalosResult<PluginSearchResponse> {
        self.manager.search(request)
    }

    pub fn list_plugins(&self, request: PluginListRequest) -> TalosResult<PluginListResponse> {
        self.manager.list_plugins(request)
    }

    pub fn read(&self, request: PluginReadRequest) -> TalosResult<PluginReadResponse> {
        self.manager.read(request)
    }

    pub fn install(&self, request: PluginInstallRequest) -> TalosResult<PluginInstallResponse> {
        self.manager.install(request)
    }

    pub fn update(&self, request: PluginUpdateRequest) -> TalosResult<PluginUpdateResponse> {
        self.manager.update(request)
    }

    pub fn uninstall(
        &self,
        request: PluginUninstallRequest,
    ) -> TalosResult<PluginUninstallResponse> {
        self.manager.uninstall(request)
    }

    pub fn enable(&self, request: PluginEnableRequest) -> TalosResult<PluginEnableResponse> {
        self.manager.enable(request)
    }

    pub fn disable(&self, request: PluginDisableRequest) -> TalosResult<PluginDisableResponse> {
        self.manager.disable(request)
    }

    pub fn skill_roots(&self, config: &SkillsRuntimeConfig) -> TalosResult<SkillRoots> {
        self.manager.skill_roots(config)
    }

    pub fn mcp_servers(&self) -> TalosResult<Vec<McpServerRuntimeDescriptor>> {
        self.manager.mcp_servers()
    }

    pub fn skill_roots_for_kernel_config(
        &self,
        config: &AgentKernelConfig,
    ) -> TalosResult<SkillRoots> {
        self.manager.skill_roots(&config.skills)
    }
}
