use talos_error::TalosResult;
use talos_service::Actor;
use talos_types::{ModelListRequest, ModelListResponse};

use crate::gateway_service::ServiceGateway;

impl ServiceGateway {
    pub(crate) async fn list_models(
        &self,
        actor: &Actor,
        request: ModelListRequest,
    ) -> TalosResult<ModelListResponse> {
        self.service.list_models(actor, request).await
    }
}
