use talos_error::TalosResult;
use talos_service::Actor;
use talos_types::{
    PluginDisableRequest, PluginDisableResponse, PluginEnableRequest, PluginEnableResponse,
    PluginInstallRequest, PluginInstallResponse, PluginListRequest, PluginListResponse,
    PluginReadRequest, PluginReadResponse, PluginSearchRequest, PluginSearchResponse,
    PluginSourceAddRequest, PluginSourceAddResponse, PluginSourceListRequest,
    PluginSourceListResponse, PluginSourceRefreshRequest, PluginSourceRefreshResponse,
    PluginSourceRemoveRequest, PluginSourceRemoveResponse, PluginUninstallRequest,
    PluginUninstallResponse, PluginUpdateRequest, PluginUpdateResponse,
};

use crate::gateway_service::ServiceGateway;

impl ServiceGateway {
    pub(crate) async fn list_plugin_sources(
        &self,
        actor: &Actor,
        request: PluginSourceListRequest,
    ) -> TalosResult<PluginSourceListResponse> {
        self.service.list_plugin_sources(actor, request).await
    }

    pub(crate) async fn add_plugin_source(
        &self,
        actor: &Actor,
        request: PluginSourceAddRequest,
    ) -> TalosResult<PluginSourceAddResponse> {
        self.service.add_plugin_source(actor, request).await
    }

    pub(crate) async fn refresh_plugin_source(
        &self,
        actor: &Actor,
        request: PluginSourceRefreshRequest,
    ) -> TalosResult<PluginSourceRefreshResponse> {
        self.service.refresh_plugin_source(actor, request).await
    }

    pub(crate) async fn remove_plugin_source(
        &self,
        actor: &Actor,
        request: PluginSourceRemoveRequest,
    ) -> TalosResult<PluginSourceRemoveResponse> {
        self.service.remove_plugin_source(actor, request).await
    }

    pub(crate) async fn search_plugins(
        &self,
        actor: &Actor,
        request: PluginSearchRequest,
    ) -> TalosResult<PluginSearchResponse> {
        self.service.search_plugins(actor, request).await
    }

    pub(crate) async fn list_plugins(
        &self,
        actor: &Actor,
        request: PluginListRequest,
    ) -> TalosResult<PluginListResponse> {
        self.service.list_plugins(actor, request).await
    }

    pub(crate) async fn read_plugin(
        &self,
        actor: &Actor,
        request: PluginReadRequest,
    ) -> TalosResult<PluginReadResponse> {
        self.service.read_plugin(actor, request).await
    }

    pub(crate) async fn install_plugin(
        &self,
        actor: &Actor,
        request: PluginInstallRequest,
    ) -> TalosResult<PluginInstallResponse> {
        self.service.install_plugin(actor, request).await
    }

    pub(crate) async fn update_plugin(
        &self,
        actor: &Actor,
        request: PluginUpdateRequest,
    ) -> TalosResult<PluginUpdateResponse> {
        self.service.update_plugin(actor, request).await
    }

    pub(crate) async fn uninstall_plugin(
        &self,
        actor: &Actor,
        request: PluginUninstallRequest,
    ) -> TalosResult<PluginUninstallResponse> {
        self.service.uninstall_plugin(actor, request).await
    }

    pub(crate) async fn enable_plugin(
        &self,
        actor: &Actor,
        request: PluginEnableRequest,
    ) -> TalosResult<PluginEnableResponse> {
        self.service.enable_plugin(actor, request).await
    }

    pub(crate) async fn disable_plugin(
        &self,
        actor: &Actor,
        request: PluginDisableRequest,
    ) -> TalosResult<PluginDisableResponse> {
        self.service.disable_plugin(actor, request).await
    }
}
