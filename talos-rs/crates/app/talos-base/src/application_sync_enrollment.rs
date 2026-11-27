//! Restore durable local capture after confirmed Worker enrollment.
use super::application_host::ApplicationHost;
use talos_auth::RequestIdentity;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_service::Actor;
use talos_sync::{ComputerAlias, SyncBindParams};

impl ApplicationHost {
    /// Explicit enrollment can attach new local work after its previous alias
    /// was retired. Restart the existing account transport so bootstrap captures
    /// facts created while no registered Computer owned the embedded alias.
    pub(crate) async fn refresh_enrolled_capture(&self, actor: &Actor) -> TalosResult<()> {
        let verified = self.verified_actor(actor).await?;
        if self.sync.account_binding(&verified).await?.is_none() {
            return Ok(());
        }
        let pause = self.pause_refresh(&actor.user_id).await;
        let _lifecycle = self.lifecycle.lock().await;
        if !self.account_connections_allowed(&actor.user_id) {
            drop(pause);
            return Ok(());
        }
        let result = self.install_enrolled_capture(actor, &verified).await;
        drop(pause);
        self.schedule_active_refresh(actor);
        result
    }

    /// Caller holds the account lifecycle lock; retirement serializes alias publication.
    pub(super) async fn install_enrolled_capture(
        &self,
        actor: &Actor,
        verified: &RequestIdentity,
    ) -> TalosResult<()> {
        let _retirement = self.registration_retirement.lock().await;
        let enabled = self.config.reload()?.sync.enabled != Some(false)
            && self.sync.enabled(&verified.user_id).await?;
        let Some(binding) = self.sync.account_binding(verified).await? else {
            return Ok(());
        };
        let Some(computer) = self
            .local_computer(actor)
            .await?
            .filter(|id| id != "embedded")
        else {
            return Ok(());
        };
        let alias = ComputerAlias {
            source_node_id: "embedded".into(),
            computer_id: computer,
        };
        let mut computers = binding.computers;
        if computers.contains(&alias) {
            if !enabled
                || self
                    .stores
                    .handles
                    .lock()
                    .await
                    .contains_key(&verified.user_id)
            {
                return Ok(());
            }
        } else {
            if computers
                .iter()
                .any(|existing| existing.source_node_id == alias.source_node_id)
            {
                return Err(TalosError::new(
                    ErrorCode::Conflict,
                    "Synchronization identity cannot be rebound.",
                ));
            }
            computers.push(alias);
        }
        let params = SyncBindParams {
            scope: binding.scope,
            source_user_id: binding.source_user_id,
            computers,
        };
        if !enabled {
            self.verify_computer_aliases(verified, &params.computers)
                .await?;
            let binding = self.sync.bind_account(verified, params).await?;
            self.install_capture(binding)?;
            return Ok(());
        }
        self.stop_account_sync(&verified.user_id).await?;
        self.start_account_sync(verified, params, self.credentials(verified)?)
            .await?;
        Ok(())
    }
}
