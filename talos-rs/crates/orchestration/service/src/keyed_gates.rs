//! Async gates keyed by identity, retained only while a caller uses them.
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
};
use tokio::sync::RwLock;
#[derive(Default)]
pub(crate) struct KeyedGates(Mutex<HashMap<String, Weak<RwLock<()>>>>);
impl KeyedGates {
    pub fn for_node(&self, key: &str) -> Arc<RwLock<()>> {
        let mut gates = self.0.lock().expect("keyed gate lock poisoned");
        gates.retain(|_, gate| gate.strong_count() > 0);
        if let Some(gate) = gates.get(key).and_then(Weak::upgrade) {
            return gate;
        }
        let gate = Arc::new(RwLock::new(()));
        gates.insert(key.to_owned(), Arc::downgrade(&gate));
        gate
    }
}
