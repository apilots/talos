use talos_application_service::Actor;
use talos_error::TalosResult;
use talos_types::{
    PluginDisableRequest, PluginDisableResponse, PluginEnableRequest, PluginEnableResponse,
    PluginInstallRequest, PluginInstallResponse, PluginListRequest, PluginListResponse,
    PluginReadRequest, PluginReadResponse, PluginSearchRequest, PluginSearchResponse,
    PluginSourceAddRequest, PluginSourceAddResponse, PluginSourceListRequest,
    PluginSourceListResponse, PluginSourceRefreshRequest, PluginSourceRefreshResponse,
    PluginSourceRemoveRequest, PluginSourceRemoveResponse, PluginUninstallRequest,
    PluginUninstallResponse, PluginUpdateRequest, PluginUpdateResponse,
};

use crate::application_service_gateway::ApplicationServiceGateway;

impl ApplicationServiceGateway {
    pub(crate) async fn list_plugin_sources(
        &self,
        _actor: &Actor,
        request: PluginSourceListRequest,
    ) -> TalosResult<PluginSourceListResponse> {
        self.application_service.plugins.list_sources(request)
    }

    pub(crate) async fn add_plugin_source(
        &self,
        _actor: &Actor,
        request: PluginSourceAddRequest,
    ) -> TalosResult<PluginSourceAddResponse> {
        self.application_service.plugins.add_source(request)
    }

    pub(crate) async fn refresh_plugin_source(
        &self,
        _actor: &Actor,
        request: PluginSourceRefreshRequest,
    ) -> TalosResult<PluginSourceRefreshResponse> {
        self.application_service.plugins.refresh_source(request)
    }

    pub(crate) async fn remove_plugin_source(
        &self,
        _actor: &Actor,
        request: PluginSourceRemoveRequest,
    ) -> TalosResult<PluginSourceRemoveResponse> {
        self.application_service.plugins.remove_source(request)
    }

    pub(crate) async fn search_plugins(
        &self,
        _actor: &Actor,
        request: PluginSearchRequest,
    ) -> TalosResult<PluginSearchResponse> {
        self.application_service.plugins.search(request)
    }

    pub(crate) async fn list_plugins(
        &self,
        _actor: &Actor,
        request: PluginListRequest,
    ) -> TalosResult<PluginListResponse> {
        self.application_service.plugins.list_plugins(request)
    }

    pub(crate) async fn read_plugin(
        &self,
        _actor: &Actor,
        request: PluginReadRequest,
    ) -> TalosResult<PluginReadResponse> {
        self.application_service.plugins.read(request)
    }

    pub(crate) async fn install_plugin(
        &self,
        _actor: &Actor,
        request: PluginInstallRequest,
    ) -> TalosResult<PluginInstallResponse> {
        self.application_service.plugins.install(request)
    }

    pub(crate) async fn update_plugin(
        &self,
        _actor: &Actor,
        request: PluginUpdateRequest,
    ) -> TalosResult<PluginUpdateResponse> {
        self.application_service.plugins.update(request)
    }

    pub(crate) async fn uninstall_plugin(
        &self,
        _actor: &Actor,
        request: PluginUninstallRequest,
    ) -> TalosResult<PluginUninstallResponse> {
        self.application_service.plugins.uninstall(request)
    }

    pub(crate) async fn enable_plugin(
        &self,
        _actor: &Actor,
        request: PluginEnableRequest,
    ) -> TalosResult<PluginEnableResponse> {
        self.application_service.plugins.enable(request)
    }

    pub(crate) async fn disable_plugin(
        &self,
        _actor: &Actor,
        request: PluginDisableRequest,
    ) -> TalosResult<PluginDisableResponse> {
        self.application_service.plugins.disable(request)
    }
}
