use crate::{Actor, ServiceResult, TalosService};
use talos_types::ModelInfo;

impl TalosService {
    pub async fn list_models(&self, _actor: &Actor) -> ServiceResult<Vec<ModelInfo>> {
        Ok(self.config.list_models().await)
    }
}
