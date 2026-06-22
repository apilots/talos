use talos_application_service::Actor;
use talos_error::TalosResult;
use talos_types::{ModelListRequest, ModelListResponse};

use crate::application_service_gateway::ApplicationServiceGateway;

impl ApplicationServiceGateway {
    pub(crate) async fn list_models(
        &self,
        actor: &Actor,
        _request: ModelListRequest,
    ) -> TalosResult<ModelListResponse> {
        self.application_service
            .list_models(actor)
            .await
            .map(|models| ModelListResponse { models })
    }
}
