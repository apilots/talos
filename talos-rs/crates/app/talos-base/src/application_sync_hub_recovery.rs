//! Account-owned Hub identity replacement, independent of Computer observations.
use super::application_host::ApplicationHost;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_service::Actor;
use talos_sync::SyncBindParams;

/// Hub discovery/recovery must not delay independent control-plane observations.
pub(crate) async fn maintain_hub_identity(
    host: std::sync::Weak<ApplicationHost>,
    mut actors: tokio::sync::watch::Receiver<Actor>,
) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = interval.tick() => {},
            changed = actors.changed() => { if changed.is_err() { break } },
        }
        let actor = actors.borrow_and_update().clone();
        let Some(host) = host.upgrade() else { break };
        if host.account_connections_allowed(&actor.user_id) {
            tokio::select! {
                result = host.refresh_hub_identity(&actor) => {
                    if let Err(error) = result { talos_logging::report_error(&error); }
                },
                changed = actors.changed() => { if changed.is_err() { break } },
            }
        }
    }
}

impl ApplicationHost {
    /// A verified connection may discover a replacement Hub after its old identity is retired.
    /// Recovery runs in owned maintenance; business reads never wait for this handshake.
    pub(crate) async fn refresh_hub_identity(&self, actor: &Actor) -> TalosResult<()> {
        let current = self
            .stores
            .handles
            .lock()
            .await
            .get(&actor.user_id)
            .cloned();
        let Some(current) = current else {
            return Ok(());
        };
        if !current.is_stopped()
            && !current
                .status()
                .last_error
                .as_ref()
                .is_some_and(|error| error.code == ErrorCode::Conflict)
        {
            return Ok(());
        }
        let verified = self.verified_actor(actor).await?;
        let credentials = self.credentials(&verified)?;
        let scope = self
            .discover_document_scope(&verified, credentials.clone())
            .await?;
        if &scope == current.scope() && !current.is_stopped() {
            return Ok(());
        }
        // Match enrollment's lock order and serialize replacement with settings/logout.
        // Discovery stays outside the lifecycle lock so slow Hub access cannot hold it.
        let _lifecycle = self.lifecycle.lock().await;
        if !self.account_connections_allowed(&actor.user_id)
            || !self.sync.enabled(&actor.user_id).await?
        {
            return Ok(());
        }
        let _retirement = self.registration_retirement.lock().await;
        let binding = self.sync.account_binding(&verified).await?.ok_or_else(|| {
            TalosError::new(
                ErrorCode::Conflict,
                "The previous synchronization binding is unavailable.",
            )
        })?;
        self.start_account_sync(
            &verified,
            SyncBindParams {
                scope,
                source_user_id: binding.source_user_id,
                computers: binding.computers,
            },
            credentials,
        )
        .await?;
        Ok(())
    }
}
