use crate::{Actor, ServiceResult, TalosService};
use talos_types::ModelListResponse;

impl TalosService {
    pub async fn list_models(&self, _actor: &Actor) -> ServiceResult<ModelListResponse> {
        self.config.list_models().await
    }
}
