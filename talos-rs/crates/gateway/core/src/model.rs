use talos_error::TalosResult;
use talos_service::Actor;
use talos_types::{ModelListRequest, ModelListResponse};

use crate::service_gateway::ServiceGateway;

impl ServiceGateway {
    pub(crate) async fn list_models(
        &self,
        actor: &Actor,
        _request: ModelListRequest,
    ) -> TalosResult<ModelListResponse> {
        self.service
            .list_models(actor)
            .await
            .map(|models| ModelListResponse { models })
    }
}
