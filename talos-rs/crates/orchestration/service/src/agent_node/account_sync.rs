//! A verified management request lets G observe that account's directory facts.
use super::AgentNodeService;
use crate::{Actor, ManagementSync};
use std::sync::{Arc, Weak};

impl AgentNodeService {
    /// Only the central management host installs this weak product composition hook.
    pub fn set_account_sync(&self, sync: Weak<dyn ManagementSync>) {
        *self.account_sync.write().expect("node account sync lock") = Some(sync);
    }
    pub(super) fn observe_account(&self, actor: &Actor) {
        if actor.auth_disabled || actor.access_token.is_none() {
            return;
        }
        let sync: Option<Arc<dyn ManagementSync>> = self
            .account_sync
            .read()
            .expect("node account sync lock")
            .as_ref()
            .and_then(Weak::upgrade);
        if let Some(sync) = sync {
            // H outage never changes the success or durability of G provisioning.
            sync.resume(actor);
        }
    }
}
