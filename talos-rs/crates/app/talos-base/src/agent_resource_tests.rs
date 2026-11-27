//! Local resource scope uses the product owner's persisted Session binding.
use super::*;

#[tokio::test]
async fn local_resource_scope_is_bound_to_the_persisted_session() {
    let storage = Arc::new(
        SqliteStorage::new_with_max_connections(":memory:", 1)
            .await
            .unwrap(),
    );
    talos_store::migration::run_migrations(
        storage.pool(),
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../conf/migrations/sqlite"),
    )
    .await
    .unwrap();
    let nodes = AgentNodeService::new(Arc::new(DatabaseAgentNodeStore::new(&storage)));
    let sessions = Arc::new(DatabaseSessionStore::new(storage).await.unwrap());
    let mut record = session_record();
    record.execution_node_id = "embedded".into();
    sessions.insert_session_record(&record).await.unwrap();
    let data = NodeSessionDataRpc::new(nodes, sessions, Arc::new(TestAttachments));
    let context = agent_protocol::AgentSessionContext {
        agent_id: record.agent_id.clone(),
        session_id: record.id.clone(),
        workspace: talos_config::Config::default()
            .resolve_workspace("default", "user-1")
            .unwrap(),
    };
    data.authorize_local_session(&context).await.unwrap();
    let mut unrelated = context;
    unrelated.session_id = "unrelated-session".into();
    assert!(data.authorize_local_session(&unrelated).await.is_err());
}
