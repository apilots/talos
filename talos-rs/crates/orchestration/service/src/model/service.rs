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
            .list_models(crate::access::worker_environment_context(actor))
            .await
    }
}
