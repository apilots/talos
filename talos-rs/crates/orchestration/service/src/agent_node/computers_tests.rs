use std::sync::Arc;

use talos_auth::UserRole;
use talos_error::ErrorCode;
use talos_storage::SqliteStorage;
use talos_store::node::DatabaseAgentNodeStore;
use talos_types::{ComputerListRequest, GetNodeRequest};

use super::{AgentNodeService, CreateNodeRequest};
use crate::Actor;

#[tokio::test]
async fn computer_directory_scopes_search_and_pagination_without_hiding_embedded_history() {
    let storage = SqliteStorage::new_with_max_connections(":memory:", 1)
        .await
        .unwrap();
    talos_store::migration::run_migrations(
        storage.pool(),
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../conf/migrations/sqlite"),
    )
    .await
    .unwrap();
    let service = AgentNodeService::new(Arc::new(DatabaseAgentNodeStore::new(
        storage.pool().clone(),
    )));
    service.register_embedded(true).await.unwrap();
    let owner = Actor::regular("owner");
    let other = Actor::regular("other");
    let admin = Actor::regular("admin").with_role(UserRole::Admin);
    let mut own_ids = vec!["embedded".to_string()];
    for (actor, name) in [
        (&owner, "Laptop one"),
        (&owner, "Laptop two"),
        (&owner, "100% host"),
        (&other, "Laptop other"),
        (&admin, "Laptop admin"),
    ] {
        let node = service
            .create_node(
                actor,
                CreateNodeRequest {
                    display_name: name.into(),
                    endpoint: None,
                    owner_user_id: None,
                    credential_expires_at: chrono::Utc::now().timestamp() + 3600,
                },
            )
            .await
            .unwrap();
        if actor == &owner {
            own_ids.push(node.node.node_id);
        }
    }
    let mut listed = Vec::new();
    let mut cursor = None;
    loop {
        let page = service
            .list_computers(
                &owner,
                ComputerListRequest {
                    cursor,
                    limit: Some(1),
                    query: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(page.total, 4);
        assert_eq!(page.data.len(), 1);
        listed.push(page.data[0].node_id.clone());
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    own_ids.sort();
    assert_eq!(listed, own_ids);
    for (query, count) in [("laptop", 2), ("%", 1), ("_", 0)] {
        let page = service
            .list_computers(
                &owner,
                ComputerListRequest {
                    query: Some(query.into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(page.total, count);
        assert_eq!(page.data.len() as u64, count);
    }
    let administrator = service
        .list_computers(&admin, ComputerListRequest::default())
        .await
        .unwrap();
    assert_eq!(administrator.total, 2);
    assert!(administrator
        .data
        .iter()
        .all(|node| node.node_id == "embedded" || node.owner_user_id.as_deref() == Some("admin")));
    let local = Actor::auth_disabled();
    let local_page = service
        .list_computers(&local, ComputerListRequest::default())
        .await
        .unwrap();
    assert_eq!(local_page.total, 1);
    assert_eq!(local_page.data[0].node_id, "embedded");
    for actor in [&owner, &local] {
        assert_eq!(
            service
                .read_computer(
                    actor,
                    GetNodeRequest {
                        node_id: "embedded".into()
                    }
                )
                .await
                .unwrap()
                .node_id,
            "embedded"
        );
    }
    let remote_id = own_ids.iter().find(|id| *id != "embedded").unwrap();
    for actor in [&other, &admin, &local] {
        assert!(service
            .read_computer(
                actor,
                GetNodeRequest {
                    node_id: remote_id.clone()
                }
            )
            .await
            .unwrap_err()
            .is_code(ErrorCode::ResourceNotFound));
    }
}
