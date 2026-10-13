use talos_error::ErrorCode;
use talos_storage::Storage;
use talos_sync_sdk::{version_equal, SyncReplica};

use super::{capture, fixture, node, options, COMPUTER};
use crate::*;

#[tokio::test]
async fn restart_accepts_equivalent_encoded_versions_but_rejects_a_changed_cut() {
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
    let baseline = fixture
        .sync
        .export_pending(&fixture.binding, resource.clone(), 128)
        .await
        .unwrap()
        .unwrap();
    fixture.sync.acknowledge(&baseline).await.unwrap();
    let mut merged = SyncReplica::new(resource.clone(), 10_000).unwrap();
    merged.import(&[baseline.update]).unwrap();
    let snapshot = merged.snapshot().unwrap();
    for writer in 20_000..20_032 {
        let mut peer = SyncReplica::from_snapshot(resource.clone(), &snapshot).unwrap();
        peer.set_writer_id(writer).unwrap();
        let change = crate::projection::change(
            &fixture.binding,
            SyncChange::Metadata {
                before: Some(Box::new(SyncResourceSnapshot::Computer(node("Baseline")))),
                after: Box::new(SyncResourceSnapshot::Computer(node(&format!(
                    "Peer {writer}"
                )))),
            },
        )
        .unwrap()
        .unwrap();
        merged.import(&[peer.apply(&[change]).unwrap()]).unwrap();
    }
    let version = merged.version().unwrap();
    let mut tx = fixture.storage.begin_transaction().await.unwrap();
    fixture
        .sync
        .import_document(
            &mut *tx,
            &fixture.binding,
            &resource,
            &[merged.snapshot().unwrap()],
            &version,
            None,
        )
        .await
        .unwrap();
    tx.commit().await.unwrap();

    // Official VV bytes preserve a map's iteration order, not a canonical order.
    let decoded = loro::VersionVector::decode(&version.version_vector).unwrap();
    let reordered = (64..1024)
        .find_map(|capacity| {
            let mut candidate = loro::VersionVector::default();
            candidate.reserve(capacity);
            candidate.extend(decoded.iter().map(|(peer, counter)| (*peer, *counter)));
            let bytes = candidate.encode();
            (bytes != version.version_vector).then_some(bytes)
        })
        .expect("different official encodings of the same multi-peer version");
    let equivalent = SyncVersion {
        version_vector: reordered,
        history_generation: version.history_generation,
    };
    assert!(version_equal(&version, &equivalent).unwrap());
    write_vector(&*fixture.storage, &equivalent.version_vector).await;
    let reopened = TalosSync::open(fixture.storage.clone(), options())
        .await
        .unwrap();
    assert!(reopened
        .resource_summary(&fixture.binding, &resource)
        .await
        .unwrap()
        .is_some());

    let mut corrupted = decoded;
    let peer = *corrupted.keys().next().unwrap();
    corrupted.remove(&peer);
    write_vector(&*fixture.storage, &corrupted.encode()).await;
    let error = reopened
        .resource_summary(&fixture.binding, &resource)
        .await
        .unwrap_err();
    assert_eq!(error.error_code(), ErrorCode::InternalError);
}

async fn write_vector(storage: &dyn Storage, bytes: &[u8]) {
    let hex = bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    storage
        .execute(
            "UPDATE sync_documents SET version_vector = unhex(?)",
            &[&hex],
        )
        .await
        .unwrap();
}
