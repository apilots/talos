use crate::{Actor, ServiceResult, TalosService};
use talos_types::{ModelListRequest, ModelListResponse};
impl TalosService {
    pub async fn list_models(
        &self,
        actor: &Actor,
        request: ModelListRequest,
    ) -> ServiceResult<ModelListResponse> {
        self.worker_environment(actor, request.node_id.as_deref())
            .await?
            .list_models(actor.worker_environment_context())
            .await
    }
}
