use talos_error::TalosResult;
use talos_service::Actor;
use talos_types::{ProfileReadRequest, ProfileReadResponse};

use crate::ServiceGateway;

impl ServiceGateway {
    pub(crate) async fn read_profile(
        &self,
        actor: &Actor,
        _request: ProfileReadRequest,
    ) -> TalosResult<ProfileReadResponse> {
        self.service.read_profile(actor).await
    }
}
