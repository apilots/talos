use talos_storage::Storage;
use talos_sync_sdk::SyncReplica;

use super::{capture, fixture, node, COMPUTER};
use crate::*;

async fn seeded() -> (super::Fixture, SyncResourceKey, SyncReplica) {
    let fixture = fixture().await;
    let resource = SyncResourceKey::Computer {
        computer_id: COMPUTER.into(),
    };
    capture(
        &fixture,
        resource.clone(),
        vec![SyncChange::Metadata {
            before: None,
            after: Box::new(SyncResourceSnapshot::Computer(node("Baseline"))),
        }],
    )
    .await;
    let batch = fixture
        .sync
        .export_pending(&fixture.binding, resource.clone(), 128)
        .await
        .unwrap()
        .unwrap();
    let mut hub = SyncReplica::new(resource.clone(), 88).unwrap();
    hub.import(std::slice::from_ref(&batch.update)).unwrap();
    hub.set_writer_id(88).unwrap();
    fixture.sync.acknowledge(&batch).await.unwrap();
    (fixture, resource, hub)
}

#[tokio::test]
async fn confirmed_hub_delete_supersedes_pending_patch_atomically_without_an_upload_ack() {
    let (fixture, resource, mut hub) = seeded().await;
    capture(
        &fixture,
        resource.clone(),
        vec![SyncChange::Metadata {
            before: Some(Box::new(SyncResourceSnapshot::Computer(node("Baseline")))),
            after: Box::new(SyncResourceSnapshot::Computer(node("Offline rename"))),
        }],
    )
    .await;
    let (_, pending_version) = fixture
        .sync
        .flush_cut(&fixture.binding, &resource)
        .await
        .unwrap();
    let delete = hub.apply(&[talos_sync_types::SyncChange::Delete]).unwrap();
    let hub_version = hub.version().unwrap();
    let mut transaction = fixture.storage.begin_transaction().await.unwrap();
    fixture
        .sync
        .import_document(
            &mut *transaction,
            &fixture.binding,
            &resource,
            std::slice::from_ref(&delete),
            &hub_version,
            Some("terminal-cut"),
        )
        .await
        .unwrap();
    transaction.rollback().await.unwrap();
    assert_eq!(
        fixture
            .sync
            .pending_resources(&fixture.binding, 32)
            .await
            .unwrap(),
        vec![resource.clone()]
    );
    assert_eq!(
        fixture
            .sync
            .download_cursor(&fixture.binding)
            .await
            .unwrap(),
        None
    );

    let mut transaction = fixture.storage.begin_transaction().await.unwrap();
    let (_, changes) = fixture
        .sync
        .import_document(
            &mut *transaction,
            &fixture.binding,
            &resource,
            &[delete],
            &hub_version,
            Some("terminal-cut"),
        )
        .await
        .unwrap();
    assert!(matches!(
        changes.last(),
        Some(talos_sync_types::SyncChange::Delete)
    ));
    transaction.commit().await.unwrap();
    assert!(fixture
        .sync
        .pending_resources(&fixture.binding, 32)
        .await
        .unwrap()
        .is_empty());
    let mut transaction = fixture.storage.begin_transaction().await.unwrap();
    let record = store::document(&mut *transaction, &fixture.binding.scope, &resource)
        .await
        .unwrap()
        .unwrap();
    transaction.commit().await.unwrap();
    assert!(talos_sync_sdk::version_includes(&record.acknowledged, &hub_version).unwrap());
    assert!(!talos_sync_sdk::version_includes(&record.acknowledged, &pending_version).unwrap());
    let other = SyncResourceKey::Computer {
        computer_id: uuid::Uuid::new_v4().to_string(),
    };
    let mut metadata = node("Other resource");
    metadata.node_id = other.id().into();
    capture(
        &fixture,
        other.clone(),
        vec![SyncChange::Metadata {
            before: None,
            after: Box::new(SyncResourceSnapshot::Computer(metadata)),
        }],
    )
    .await;
    assert!(fixture
        .sync
        .export_pending(&fixture.binding, other, 128)
        .await
        .unwrap()
        .is_some());
}

#[tokio::test]
async fn local_unacknowledged_delete_survives_a_concurrent_hub_patch_until_its_actual_ack() {
    let (fixture, resource, mut hub) = seeded().await;
    capture(&fixture, resource.clone(), vec![SyncChange::Delete]).await;
    let (_, delete_version) = fixture
        .sync
        .flush_cut(&fixture.binding, &resource)
        .await
        .unwrap();
    let before = match hub.changes().unwrap().remove(0) {
        talos_sync_types::SyncChange::Metadata { after, .. } => after,
        _ => panic!("Computer metadata"),
    };
    let mut after = before.clone();
    let talos_sync_types::SyncResourceSnapshot::Computer(node) = after.as_mut() else {
        panic!("Computer metadata")
    };
    node.display_name = "Changed on hub".into();
    let update = hub
        .apply(&[talos_sync_types::SyncChange::Metadata {
            before: Some(before),
            after,
        }])
        .unwrap();
    let hub_version = hub.version().unwrap();
    let mut transaction = fixture.storage.begin_transaction().await.unwrap();
    let (_, changes) = fixture
        .sync
        .import_document(
            &mut *transaction,
            &fixture.binding,
            &resource,
            &[update],
            &hub_version,
            None,
        )
        .await
        .unwrap();
    assert!(matches!(
        changes.last(),
        Some(talos_sync_types::SyncChange::Delete)
    ));
    transaction.commit().await.unwrap();
    assert_eq!(
        fixture
            .sync
            .pending_resources(&fixture.binding, 32)
            .await
            .unwrap(),
        vec![resource.clone()]
    );
    let batch = fixture
        .sync
        .export_pending(&fixture.binding, resource.clone(), 128)
        .await
        .unwrap()
        .unwrap();
    hub.import_from_peer(
        std::slice::from_ref(&batch.update),
        &fixture.binding.peer_id,
    )
    .unwrap();
    assert!(hub.summary().unwrap().deleted);
    assert!(talos_sync_sdk::version_includes(&hub.version().unwrap(), &delete_version).unwrap());
    fixture.sync.acknowledge(&batch).await.unwrap();
    assert!(fixture
        .sync
        .pending_resources(&fixture.binding, 32)
        .await
        .unwrap()
        .is_empty());
}
