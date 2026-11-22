use super::*;
use std::sync::atomic::Ordering;
fn actor() -> talos_service::Actor {
    talos_service::Actor::regular("account").with_access_token("credential")
}
#[tokio::test]
async fn directory_reads_remain_local_while_control_plane_observations_are_blocked() {
    let mut fixture = Fixture::start().await;
    fixture.block_directory.store(true, Ordering::SeqCst);
    let actor = actor();
    fixture
        .runtime
        .sync
        .set_enabled(&actor.user_id, false)
        .await
        .unwrap();
    let host = fixture.runtime.sync_host.clone();
    let refresh_actor = actor.clone();
    let refresh =
        tokio::spawn(async move { host.refresh_computer_directory(&refresh_actor).await });
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), fixture.entered.recv())
            .await
            .unwrap()
            .unwrap(),
        "directory"
    );
    tokio::time::timeout(
        Duration::from_millis(500),
        fixture
            .runtime
            .service
            .list_computers(&actor, Default::default()),
    )
    .await
    .expect("local directory waited for Server")
    .unwrap();
    fixture.release.add_permits(1);
    refresh.await.unwrap().unwrap();
    let page = fixture
        .runtime
        .service
        .list_computers(&actor, Default::default())
        .await
        .unwrap();
    assert_eq!(page.data.len(), 3);
    let discovered = page
        .data
        .iter()
        .find(|node| node.node_id == "fast")
        .unwrap();
    assert_eq!(discovered.display_name, "Server fast");
    assert!(discovered.available);
    assert_eq!(
        fixture
            .runtime
            .service
            .sync_status(&actor)
            .await
            .unwrap()
            .connection,
        talos_sync::SyncConnectionState::Disabled
    );
    let other = talos_service::Actor::regular("another-account").with_access_token("credential");
    assert!(fixture
        .runtime
        .service
        .list_computers(&other, Default::default())
        .await
        .unwrap()
        .data
        .is_empty());
    fixture.fail_directory.store(true, Ordering::SeqCst);
    assert!(fixture
        .runtime
        .sync_host
        .refresh_computer_directory(&actor)
        .await
        .is_err());
    assert_eq!(
        fixture
            .runtime
            .service
            .list_computers(&actor, Default::default())
            .await
            .unwrap(),
        page
    );
    fixture.finish().await;
}
#[tokio::test]
async fn confirmed_rename_survives_an_older_inflight_directory_pull_with_sync_disabled() {
    let mut fixture = Fixture::start().await;
    let actor = actor();
    fixture
        .runtime
        .sync
        .set_enabled(&actor.user_id, false)
        .await
        .unwrap();
    fixture
        .runtime
        .sync_host
        .refresh_computer_directory(&actor)
        .await
        .unwrap();
    fixture.block_directory.store(true, Ordering::SeqCst);
    let host = fixture.runtime.sync_host.clone();
    let refresh_actor = actor.clone();
    let refresh =
        tokio::spawn(async move { host.refresh_computer_directory(&refresh_actor).await });
    assert_eq!(fixture.entered.recv().await.unwrap(), "directory");
    let renamed = fixture
        .runtime
        .service
        .nodes
        .rename_computer(
            &actor,
            talos_types::RenameComputerRequest {
                node_id: "slow".into(),
                display_name: "Confirmed new name".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(renamed.display_name, "Confirmed new name");
    fixture.release.add_permits(1);
    refresh.await.unwrap().unwrap();
    let stored = fixture
        .runtime
        .service
        .nodes
        .read_computer(
            &actor,
            talos_types::GetNodeRequest {
                node_id: "slow".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(stored.display_name, "Confirmed new name");
    let page = fixture
        .runtime
        .service
        .list_computers(
            &actor,
            talos_types::ComputerListRequest {
                query: Some("Confirmed".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(page.data.len(), 1);
    assert_eq!(page.data[0].display_name, "Confirmed new name");
    assert_eq!(
        fixture
            .runtime
            .service
            .sync_status(&actor)
            .await
            .unwrap()
            .connection,
        talos_sync::SyncConnectionState::Disabled
    );
    fixture.finish().await;
}
#[tokio::test]
async fn only_complete_owner_validated_pages_replace_the_cached_directory() {
    let fixture = Fixture::start().await;
    let actor = actor();
    fixture
        .runtime
        .sync
        .set_enabled(&actor.user_id, false)
        .await
        .unwrap();
    fixture
        .runtime
        .sync_host
        .refresh_computer_directory(&actor)
        .await
        .unwrap();
    let mut foreign = directory_node("foreign-page");
    foreign.owner_user_id = Some("other-account".into());
    fixture.directory.write().unwrap().push(foreign);
    assert!(fixture
        .runtime
        .sync_host
        .refresh_computer_directory(&actor)
        .await
        .unwrap_err()
        .is_code(talos_error::ErrorCode::PermissionDenied));
    let page = fixture
        .runtime
        .service
        .list_computers(&actor, Default::default())
        .await
        .unwrap();
    assert_eq!(page.data.len(), 3);
    fixture
        .directory
        .write()
        .unwrap()
        .retain(|node| node.node_id != "fast" && node.node_id != "foreign-page");
    fixture
        .runtime
        .sync_host
        .refresh_computer_directory(&actor)
        .await
        .unwrap();
    let page = fixture
        .runtime
        .service
        .list_computers(&actor, Default::default())
        .await
        .unwrap();
    assert_eq!(page.data.len(), 2);
    assert!(page.data.iter().all(|node| node.node_id != "fast"));
    fixture.finish().await;
}

#[tokio::test]
async fn refreshed_directory_removes_stale_rows_before_sql_pagination_and_repairs_legacy_sync_markers(
) {
    let fixture = Fixture::start().await;
    let actor = actor();
    for n in 0..35 {
        fixture
            .runtime
            .sync_host
            .sync_stores
            .nodes
            .create(talos_store::node::CreateNodeParams {
                node_id: format!("000-stale-{n:02}"),
                owner_user_id: Some("account".into()),
                display_name: "Stale former registration".into(),
                mode: talos_types::NodeMode::Remote,
                endpoint: None,
                credential_hash: None,
                credential_expires_at: None,
                now_at: 1,
            })
            .await
            .unwrap();
    }
    fixture
        .runtime
        .sync_host
        .sync_stores
        .nodes
        .reconcile_deleted_registration("fast", "account")
        .await
        .unwrap();
    fixture
        .runtime
        .sync_host
        .refresh_computer_directory(&actor)
        .await
        .unwrap();
    let page = fixture
        .runtime
        .service
        .list_computers(
            &actor,
            talos_types::ComputerListRequest {
                limit: Some(20),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(page.total, 3);
    assert_eq!(page.data.len(), 3);
    assert!(page.next_cursor.is_none());
    assert!(page.data.iter().any(|node| node.node_id == "fast"));
    fixture.finish().await;
}
#[tokio::test]
async fn create_and_delete_confirmations_update_cache_during_directory_outage_and_old_pull() {
    let mut fixture = Fixture::start().await;
    let actor = actor();
    fixture
        .runtime
        .sync
        .set_enabled(&actor.user_id, false)
        .await
        .unwrap();
    fixture
        .runtime
        .sync_host
        .refresh_computer_directory(&actor)
        .await
        .unwrap();
    fixture.fail_directory.store(true, Ordering::SeqCst);
    let created = fixture
        .runtime
        .service
        .nodes
        .create_node(
            &actor,
            talos_types::CreateNodeRequest {
                display_name: "Created through Server".into(),
                endpoint: None,
                owner_user_id: None,
                credential_expires_at: chrono::Utc::now().timestamp() + 3600,
            },
        )
        .await
        .unwrap();
    assert_eq!(created.node.node_id, "created");
    assert!(fixture
        .runtime
        .service
        .list_computers(&actor, Default::default())
        .await
        .unwrap()
        .data
        .iter()
        .any(|node| node.node_id == "created"));
    fixture.fail_directory.store(false, Ordering::SeqCst);
    fixture.block_directory.store(true, Ordering::SeqCst);
    let host = fixture.runtime.sync_host.clone();
    let refresh_actor = actor.clone();
    let refresh =
        tokio::spawn(async move { host.refresh_computer_directory(&refresh_actor).await });
    assert_eq!(fixture.entered.recv().await.unwrap(), "directory");
    fixture
        .runtime
        .service
        .nodes
        .delete_computer(
            &actor,
            talos_types::DeleteComputerRequest {
                node_id: "fast".into(),
            },
        )
        .await
        .unwrap();
    fixture.release.add_permits(1);
    refresh.await.unwrap().unwrap();
    fixture.fail_directory.store(true, Ordering::SeqCst);
    assert!(fixture
        .runtime
        .sync_host
        .refresh_computer_directory(&actor)
        .await
        .is_err());
    assert!(fixture
        .runtime
        .service
        .list_computers(&actor, Default::default())
        .await
        .unwrap()
        .data
        .iter()
        .all(|node| node.node_id != "fast"));
    assert!(fixture
        .runtime
        .service
        .nodes
        .read_computer(
            &actor,
            talos_types::GetNodeRequest {
                node_id: "fast".into()
            }
        )
        .await
        .is_err());
    fixture.finish().await;
}

#[tokio::test]
async fn delayed_connection_cannot_overwrite_a_confirmed_directory_mutation() {
    let mut fixture = Fixture::start().await;
    let actor = actor();
    fixture
        .runtime
        .sync_host
        .refresh_computer_directory(&actor)
        .await
        .unwrap();
    let host = fixture.runtime.sync_host.clone();
    let identity = fixture.identity.clone();
    let slow = tokio::spawn(async move { host.connect_computer(&identity, "slow").await });
    assert_eq!(fixture.entered.recv().await.unwrap(), "slow");
    fixture
        .runtime
        .service
        .nodes
        .rename_computer(
            &actor,
            talos_types::RenameComputerRequest {
                node_id: "slow".into(),
                display_name: "Confirmed during connection".into(),
            },
        )
        .await
        .unwrap();
    fixture.release.add_permits(1);
    slow.await.unwrap().unwrap();
    let page = fixture
        .runtime
        .service
        .list_computers(
            &actor,
            talos_types::ComputerListRequest {
                query: Some("Confirmed during connection".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(page.data.len(), 1);
    assert_eq!(page.data[0].display_name, "Confirmed during connection");
    fixture.finish().await;
}

enum Removal {
    ConfirmedDelete,
    DirectoryRefresh,
}
async fn rejects_late_connection(removal: Removal) {
    let mut fixture = Fixture::start().await;
    let actor = actor();
    fixture
        .runtime
        .sync_host
        .refresh_computer_directory(&actor)
        .await
        .unwrap();
    let host = fixture.runtime.sync_host.clone();
    let identity = fixture.identity.clone();
    let slow = tokio::spawn(async move { host.connect_computer(&identity, "slow").await });
    assert_eq!(fixture.entered.recv().await.unwrap(), "slow");
    match removal {
        Removal::ConfirmedDelete => {
            fixture
                .runtime
                .service
                .nodes
                .delete_computer(
                    &actor,
                    talos_types::DeleteComputerRequest {
                        node_id: "slow".into(),
                    },
                )
                .await
                .unwrap();
        }
        Removal::DirectoryRefresh => {
            fixture
                .directory
                .write()
                .unwrap()
                .retain(|node| node.node_id != "slow");
            fixture
                .runtime
                .sync_host
                .refresh_computer_directory(&actor)
                .await
                .unwrap();
        }
    }
    fixture.release.add_permits(1);
    assert!(slow
        .await
        .unwrap()
        .unwrap_err()
        .is_code(talos_error::ErrorCode::ResourceExpired));
    assert!(fixture
        .runtime
        .service
        .list_computers(&actor, Default::default())
        .await
        .unwrap()
        .data
        .iter()
        .all(|node| node.node_id != "slow"));
    fixture.finish().await;
}
#[tokio::test]
async fn delayed_connection_cannot_resurrect_a_confirmed_deletion() {
    rejects_late_connection(Removal::ConfirmedDelete).await;
}
#[tokio::test]
async fn delayed_connection_cannot_resurrect_an_entry_removed_by_a_complete_directory() {
    rejects_late_connection(Removal::DirectoryRefresh).await;
}
