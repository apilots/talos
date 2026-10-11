//! Per-node product admission, independent from Worker execution and node leases.
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
};
use tokio::sync::RwLock;
#[derive(Default)]
pub(crate) struct NodeAdmissionGates(Mutex<HashMap<String, Weak<RwLock<()>>>>);
impl NodeAdmissionGates {
    pub fn for_node(&self, node_id: &str) -> Arc<RwLock<()>> {
        let mut gates = self.0.lock().expect("node admission lock poisoned");
        gates.retain(|_, gate| gate.strong_count() > 0);
        if let Some(gate) = gates.get(node_id).and_then(Weak::upgrade) {
            return gate;
        }
        let gate = Arc::new(RwLock::new(()));
        gates.insert(node_id.to_owned(), Arc::downgrade(&gate));
        gate
    }
}
