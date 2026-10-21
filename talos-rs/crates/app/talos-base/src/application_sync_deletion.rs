//! Observe durable removal independently of UI polling and close the local alias.
use super::application_sync::ApplicationSyncHost;
use talos_auth::RequestIdentity;
use talos_config::YamlConfigStore;
use talos_error::TalosResult;
use talos_store::node::AgentNodeStore;

impl ApplicationSyncHost {
    pub(crate) async fn refresh_removed_registration(
        &self,
        actor: &RequestIdentity,
    ) -> TalosResult<()> {
        let store = YamlConfigStore::new(self.config.source_path());
        let Some(runner) = store.load_agent_node()?.runner else {
            return Ok(());
        };
        if store.removed_registration_owner()?.as_deref() == Some(&actor.user_id)
            || self
                .sync_stores
                .nodes
                .deletion_owner(&runner.node_id)
                .await?
                .as_deref()
                == Some(&actor.user_id)
        {
            self.observe_registration_removal(actor, &runner.node_id)
                .await?;
        }
        Ok(())
    }

    pub(crate) fn observe_registration_removal<'a>(
        &'a self,
        actor: &'a RequestIdentity,
        computer: &'a str,
    ) -> agent_protocol::AgentFuture<'a, TalosResult<()>> {
        Box::pin(async move {
            let _retirement = self.registration_retirement.lock().await;
            let store = YamlConfigStore::new(self.config.source_path());
            if store
                .load_agent_node()?
                .runner
                .as_ref()
                .is_none_or(|runner| runner.node_id != computer)
            {
                return Ok(());
            }
            if store.removed_registration_owner()?.as_deref() == Some(&actor.user_id)
                && self
                    .sync_stores
                    .sessions
                    .local_registration_retired(&actor.user_id, computer)
                    .await?
            {
                return Ok(());
            }
            // Snapshot first. New work created after this transaction is never part
            // of the old identity, including if later cleanup has to be retried.
            self.sync_stores
                .sessions
                .record_local_registration_removal(&actor.user_id, computer)
                .await?;
            store.mark_registration_removed(computer, &actor.user_id)?;
            let active_sync = self
                .sync_stores
                .handles
                .lock()
                .await
                .contains_key(&actor.user_id);
            self.retire_removed_local_registration(actor).await?;
            if active_sync {
                if let Some(binding) = self.sync.account_binding(actor).await? {
                    let credentials = self
                        .sync_stores
                        .credentials
                        .read()
                        .expect("Sync credentials lock")
                        .get(&actor.user_id)
                        .cloned();
                    if let Some(credentials) = credentials {
                        self.start_account_sync(
                            actor,
                            talos_sync::SyncBindParams {
                                scope: binding.scope,
                                source_user_id: binding.source_user_id,
                                computers: binding.computers,
                            },
                            credentials,
                        )
                        .await?;
                    }
                }
            }
            Ok(())
        })
    }
}
