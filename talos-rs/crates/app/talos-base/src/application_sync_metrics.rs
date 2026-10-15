//! Metrics uses the original account verifier without changing synchronization state.
use super::application_sync::ApplicationSyncHost;
use talos_error::{TalosError, TalosResult};
use talos_service::Actor;
use talos_types::SyncMetricsResponse;

impl ApplicationSyncHost {
    pub(crate) async fn read_metrics(&self, actor: &Actor) -> TalosResult<SyncMetricsResponse> {
        let verified = self.verified_actor(actor).await?;
        let config = self.config.reload()?;
        let certificate = config
            .sync
            .trusted_ca_file
            .as_ref()
            .map(std::fs::read)
            .transpose()
            .map_err(|error| {
                TalosError::internal_with_source(
                    "Read synchronization metrics trust certificate; technical details were withheld.",
                    error,
                )
            })?;
        let mut metrics = talos_sync::read_account_metrics(
            &config.sync.hub_endpoint,
            certificate.as_deref(),
            talos_sync_sdk::VerifiedCredential {
                account_id: verified.user_id,
                access_token: verified.access_token.expect("verified actor token"),
            },
        )
        .await?;
        metrics.control_plane_peer_id = config
            .agent_node
            .control_plane
            .as_ref()
            .map(|_| self.sync.peer_id().to_owned());
        Ok(metrics)
    }
}
