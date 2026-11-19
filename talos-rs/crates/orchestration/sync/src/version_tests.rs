use talos_error::ErrorCode;
use talos_storage::Storage;
use talos_sync_sdk::{version_equal, SyncReplica};

use super::{capture, fixture, node, options, COMPUTER};
use crate::*;

#[tokio::test]
async fn large_snapshot_chunks_become_visible_with_the_document_and_reclaim_after_replacement() {
    use crate::store::{self, snapshot::Snapshot};
    let fixture = fixture().await;
    let resource = SyncResourceKey::Computer {
        computer_id: COMPUTER.into(),
    };
    let bytes = vec![42; 600_000];
    let prepared = Snapshot::prepare(fixture.storage.clone(), bytes.clone())
        .await
        .unwrap();
    let chunk_sizes: Vec<_> = fixture
        .storage
        .query(
            "SELECT length(contents) AS bytes FROM sync_snapshot_chunks ORDER BY ordinal",
            &[],
        )
        .await
        .unwrap()
        .iter()
        .map(|row| row.get_i64("bytes").unwrap())
        .collect();
    assert_eq!(chunk_sizes, [262_144, 262_144, 75_712]);
    assert!(fixture
        .sync
        .document_version(&fixture.binding, &resource)
        .await
        .unwrap()
        .is_none());

    let summary = talos_sync_types::SyncResourceSummary {
        resource: resource.clone(),
        computer_id: Some(COMPUTER.into()),
        version: SyncVersion::default(),
        deleted: false,
    };
    let mut writer = fixture
        .storage
        .begin_background_transaction()
        .await
        .unwrap();
    store::save_document(
        &mut *writer,
        &fixture.binding.scope,
        &summary,
        &prepared,
        &summary.version,
        None,
    )
    .await
    .unwrap();
    assert!(fixture
        .sync
        .document_version(&fixture.binding, &resource)
        .await
        .unwrap()
        .is_none());
    writer.commit().await.unwrap();
    let mut read = fixture.storage.begin_read_transaction().await.unwrap();
    let recorded = store::document(&mut *read, &fixture.binding.scope, &resource)
        .await
        .unwrap()
        .unwrap();
    read.commit().await.unwrap();
    assert_eq!(recorded.snapshot, bytes);

    let small = Snapshot::prepare(fixture.storage.clone(), vec![1, 2, 3])
        .await
        .unwrap();
    let mut writer = fixture
        .storage
        .begin_background_transaction()
        .await
        .unwrap();
    store::save_document(
        &mut *writer,
        &fixture.binding.scope,
        &summary,
        &small,
        &summary.version,
        None,
    )
    .await
    .unwrap();
    writer.commit().await.unwrap();
    let mut reclaimed = 0;
    while fixture.sync.reclaim_snapshot().await.unwrap() {
        reclaimed += 1;
    }
    assert_eq!(reclaimed, 4);
    let mut read = fixture.storage.begin_read_transaction().await.unwrap();
    let current = store::document(&mut *read, &fixture.binding.scope, &resource)
        .await
        .unwrap()
        .unwrap();
    read.commit().await.unwrap();
    assert_eq!(current.snapshot, [1, 2, 3]);
}

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
    let (tx, _, _) = fixture
        .sync
        .import_document(
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
    let (_, restored) = reopened
        .flush_cut(&fixture.binding, &resource)
        .await
        .unwrap();
    assert!(version_equal(&restored, &version).unwrap());

    let mut corrupted = decoded;
    let peer = *corrupted.keys().next().unwrap();
    corrupted.remove(&peer);
    write_vector(&*fixture.storage, &corrupted.encode()).await;
    let error = reopened
        .flush_cut(&fixture.binding, &resource)
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
