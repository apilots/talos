use super::*;
use std::sync::{Arc, Mutex};
use talos_storage::SqliteStorage;
use talos_store::node::DatabaseAgentNodeStore;

#[tokio::test]
async fn directory_observation_preserves_authenticated_identity_without_replication() {
    let storage = SqliteStorage::new_with_max_connections(":memory:", 1)
        .await
        .unwrap();
    let service = AgentNodeService::new(Arc::new(DatabaseAgentNodeStore::new(&storage)));
    let observed = Arc::new(Mutex::new(Vec::new()));
    let identities = observed.clone();
    service.set_directory_observer(Arc::new(move |actor| {
        identities.lock().unwrap().push(actor.user_id.clone());
    }));

    service.observe_account(&Actor::auth_disabled());
    service.observe_account(&Actor::regular("unsigned"));
    service.observe_account(&Actor::regular("owner").with_access_token("credential"));

    assert_eq!(*observed.lock().unwrap(), vec!["owner"]);
}
