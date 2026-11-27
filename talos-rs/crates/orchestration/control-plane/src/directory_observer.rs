//! Optional composition hook for verified directory observations.
use super::AgentNodeService;
use crate::{Actor, NodeDirectoryObserver};

impl AgentNodeService {
    /// The authoritative host attaches an optional lifecycle observer. The callback
    /// must return promptly and cannot change registration success or ownership.
    pub fn set_directory_observer(&self, observer: NodeDirectoryObserver) {
        *self
            .directory_observer
            .write()
            .expect("node directory observer lock") = Some(observer);
    }

    pub(super) fn observe_account(&self, actor: &Actor) {
        if actor.auth_disabled || actor.access_token.is_none() {
            return;
        }
        let observer = self
            .directory_observer
            .read()
            .expect("node directory observer lock")
            .clone();
        if let Some(observer) = observer {
            observer(actor);
        }
    }
}

#[cfg(test)]
#[path = "directory_observer_tests.rs"]
mod tests;
