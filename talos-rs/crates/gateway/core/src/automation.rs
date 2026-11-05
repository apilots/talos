//! Thin authenticated automation adaptation through the original Service.
use crate::ServiceGateway;
use talos_error::TalosResult;
use talos_service::Actor;
impl ServiceGateway {
    pub(crate) async fn session_automation(
        &self,
        actor: &Actor,
        request: talos_types::SessionAutomationReadRequest,
    ) -> TalosResult<talos_types::SessionAutomationSnapshot> {
        self.service
            .session_automation(actor, &request.session_id)
            .await
    }
    pub(crate) async fn execute_automation(
        &self,
        actor: &Actor,
        request: talos_types::AutomationRequest,
    ) -> TalosResult<talos_types::AutomationControlResult> {
        self.service.execute_automation(actor, request).await
    }
    pub(crate) async fn list_automation(
        &self,
        actor: &Actor,
        request: talos_types::AutomationListRequest,
    ) -> TalosResult<talos_types::AutomationListResponse> {
        self.service.list_automation(actor, request).await
    }
    pub(crate) async fn automation_command_catalog(
        &self,
        actor: &Actor,
        request: talos_types::AutomationCommandCatalogRequest,
    ) -> TalosResult<talos_types::AutomationCommandCatalogResponse> {
        let _ = (actor, request);
        Ok(talos_types::AutomationCommandCatalogResponse {
            data: talos_types::automation_command_catalog(),
        })
    }
    pub(crate) async fn resolve_automation_command(
        &self,
        actor: &Actor,
        request: talos_types::AutomationCommandResolveRequest,
    ) -> TalosResult<talos_types::AutomationCommandResolveResponse> {
        let _ = actor;
        Ok(talos_types::AutomationCommandResolveResponse {
            command: talos_types::resolve_automation_command(&request.text)?,
        })
    }
    pub(crate) async fn list_builtin_jobs(
        &self,
        actor: &Actor,
        request: talos_types::BuiltinJobListRequest,
    ) -> TalosResult<talos_types::BuiltinJobListResponse> {
        self.service.list_builtin_jobs(actor, request).await
    }
    pub(crate) async fn control_builtin_job(
        &self,
        actor: &Actor,
        request: talos_types::BuiltinJobControlRequest,
    ) -> TalosResult<talos_types::BuiltinJobInfo> {
        self.service.control_builtin_job(actor, request).await
    }
}
