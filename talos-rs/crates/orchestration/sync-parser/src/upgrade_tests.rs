use crate::*;
use talos_sync_sdk::{SyncErrorCode, SyncReplica};
fn binding() -> SyncBinding {
    SyncBinding {
        scope: talos_sync_types::SyncScope {
            hub_id: "old-hub".into(),
            account_id: "account-1".into(),
        },
        source_user_id: "account-1".into(),
        peer_id: "surviving-B".into(),
        computers: vec![],
    }
}
fn fixture(name: &str) -> (Vec<u8>, talos_sync_types::SyncVersion, Vec<SyncChange>) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/released-v1");
    let snapshot = std::fs::read(path.join(format!("{name}.bin"))).unwrap();
    let metadata: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path.join(format!("{name}.json"))).unwrap()).unwrap();
    assert_eq!(metadata["sdk"], "0.1.7");
    (
        snapshot,
        serde_json::from_value(metadata["version"].clone()).unwrap(),
        serde_json::from_value(metadata["changes"].clone()).unwrap(),
    )
}
fn sorted(changes: &[SyncChange]) -> Vec<String> {
    let mut values: Vec<_> = changes
        .iter()
        .map(|value| serde_json::to_string(value).unwrap())
        .collect();
    values.sort();
    values
}
#[test]
fn released_v1_preserves_complete_native_facts_and_original_sources() {
    let key = SyncResourceKey::Session {
        session_id: "released-session".into(),
    };
    let (snapshot, version, expected) = fixture("survivor-b");
    assert_eq!(
        SyncReplica::from_snapshot(key.wire(), &snapshot)
            .err()
            .unwrap()
            .code,
        SyncErrorCode::Conflict
    );
    let upgraded = upgrade_v1_snapshot(&binding(), &key, 202, &snapshot, &version)
        .unwrap()
        .unwrap();
    let replica = SyncReplica::from_snapshot(key.wire(), &upgraded).unwrap();
    let actual = decode_changes(&key, "account-1", replica.changes().unwrap()).unwrap();
    assert_eq!(sorted(&actual), sorted(&expected));
    assert_eq!(
        actual
            .iter()
            .filter(|change| matches!(change, SyncChange::Turn(_)))
            .count(),
        4
    );
    assert!(upgrade_v1_snapshot(
        &binding(),
        &key,
        202,
        &upgraded,
        &replica.version().unwrap()
    )
    .unwrap()
    .is_none());
}
#[test]
fn stale_released_source_rejoins_without_overwriting_a_new_v2_edit() {
    let key = SyncResourceKey::Session {
        session_id: "released-session".into(),
    };
    let (old_b, version_b, changes) = fixture("survivor-b");
    let (old_a, version_a, _) = fixture("source-a");
    let mut b = SyncReplica::from_snapshot(
        key.wire(),
        &upgrade_v1_snapshot(&binding(), &key, 202, &old_b, &version_b)
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    b.set_writer_id(202).unwrap();
    let SyncChange::Metadata { after, .. } = &changes[0] else {
        panic!("metadata")
    };
    let mut next = after.clone();
    let SyncResourceSnapshot::Session(session) = next.as_mut() else {
        panic!("session")
    };
    session.session.title = "New v2 title".into();
    let change = encode_change(
        &binding(),
        &key,
        SyncChange::Metadata {
            before: Some(after.clone()),
            after: next,
        },
    )
    .unwrap()
    .unwrap();
    b.apply(&[change]).unwrap();
    let returning_a = upgrade_v1_snapshot(&binding(), &key, 101, &old_a, &version_a)
        .unwrap()
        .unwrap();
    b.import_from_account(&[returning_a], "account-1").unwrap();
    let actual = decode_changes(&key, "account-1", b.changes().unwrap()).unwrap();
    let SyncChange::Metadata { after, .. } = &actual[0] else {
        panic!("metadata")
    };
    let SyncResourceSnapshot::Session(session) = after.as_ref() else {
        panic!("session")
    };
    assert_eq!(session.session.title, "New v2 title");
    assert_eq!(
        actual
            .iter()
            .filter(|change| matches!(change, SyncChange::Turn(_)))
            .count(),
        4
    );
}
#[test]
fn released_reset_and_tombstone_survive_conversion() {
    let key = SyncResourceKey::Session {
        session_id: "released-deleted".into(),
    };
    let (snapshot, version, expected) = fixture("deleted");
    let next = upgrade_v1_snapshot(&binding(), &key, 303, &snapshot, &version)
        .unwrap()
        .unwrap();
    let replica = SyncReplica::from_snapshot(key.wire(), &next).unwrap();
    assert!(replica.summary().unwrap().deleted);
    assert_eq!(replica.version().unwrap().history_generation, 1);
    assert_eq!(
        sorted(&decode_changes(&key, "account-1", replica.changes().unwrap()).unwrap()),
        sorted(&expected)
    );
    let wrong = talos_sync_types::SyncVersion::default();
    assert!(upgrade_v1_snapshot(&binding(), &key, 303, &snapshot, &wrong).is_err());
}
