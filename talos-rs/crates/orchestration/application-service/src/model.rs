use crate::{Actor, ApplicationResult, TalosApplicationService};
use talos_types::ModelInfo;

impl TalosApplicationService {
    pub async fn list_models(&self, _actor: &Actor) -> ApplicationResult<Vec<ModelInfo>> {
        Ok(self.config.list_models().await)
    }
}
