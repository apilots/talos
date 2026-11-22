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
    let upgraded = upgrade_v1_snapshot(&binding(), &key, &snapshot, &version)
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
    assert!(
        upgrade_v1_snapshot(&binding(), &key, &upgraded, &replica.version().unwrap())
            .unwrap()
            .is_none()
    );
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
        &upgrade_v1_snapshot(&binding(), &key, &old_b, &version_b)
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
    let returning_a = upgrade_v1_snapshot(&binding(), &key, &old_a, &version_a)
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
    let next = upgrade_v1_snapshot(&binding(), &key, &snapshot, &version)
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
    assert!(upgrade_v1_snapshot(&binding(), &key, &snapshot, &wrong).is_err());
}

#[test]
fn released_record_envelopes_preserve_raw_optional_fields_and_native_validation() {
    let key = SyncResourceKey::Session {
        session_id: "released-session".into(),
    };
    let (snapshot, version, expected) = fixture("envelopes");
    let converted = upgrade_v1_snapshot(&binding(), &key, &snapshot, &version)
        .unwrap()
        .unwrap();
    let replica = SyncReplica::from_snapshot(key.wire(), &converted).unwrap();
    let wire = replica.changes().unwrap();
    let command = wire
        .iter()
        .find_map(|change| match change {
            talos_sync_sdk::SyncChange::Record(record)
                if record.value["type"] == "acceptedCommand" =>
            {
                Some(record)
            }
            _ => None,
        })
        .unwrap();
    assert!(
        command.value["data"].get("expiresAt").is_none(),
        "optional defaults must not change original operation values"
    );
    assert_eq!(
        sorted(&decode_changes(&key, "account-1", wire.clone()).unwrap()),
        sorted(&expected)
    );
    let mut invalid = command.clone();
    invalid.stream = "turn-3".into();
    assert!(decode_changes(
        &key,
        "account-1",
        vec![talos_sync_sdk::SyncChange::Record(invalid)]
    )
    .is_err());
}
#[test]
fn deterministic_independent_header_is_identical_across_different_retained_heads() {
    let key = SyncResourceKey::Session {
        session_id: "released-session".into(),
    };
    let migration_peer = super::header_writer(&binding(), &key);
    let mut headers = Vec::new();
    for name in ["source-a", "survivor-b"] {
        let (snapshot, version, _) = fixture(name);
        let converted = upgrade_v1_snapshot(&binding(), &key, &snapshot, &version)
            .unwrap()
            .unwrap();
        let doc = loro::LoroDoc::new();
        doc.import(&converted).unwrap();
        let updates = doc.export_json_updates_without_peer_compression(
            &loro::VersionVector::default(),
            &doc.oplog_vv(),
        );
        let header = updates
            .changes
            .into_iter()
            .find(|change| change.id.peer == migration_peer)
            .unwrap();
        headers.push(serde_json::to_value(header).unwrap());
    }
    assert_eq!(headers[0], headers[1]);
}
#[test]
fn duplicate_converted_tombstone_carries_no_novel_header_operation() {
    let key = SyncResourceKey::Session {
        session_id: "released-deleted".into(),
    };
    let (snapshot, version, _) = fixture("deleted");
    let first = upgrade_v1_snapshot(&binding(), &key, &snapshot, &version)
        .unwrap()
        .unwrap();
    let second = upgrade_v1_snapshot(&binding(), &key, &snapshot, &version)
        .unwrap()
        .unwrap();
    let mut hub = SyncReplica::from_snapshot(key.wire(), &first).unwrap();
    let before = hub.version().unwrap();
    hub.import_from_account(&[second], "account-1").unwrap();
    assert!(talos_sync_sdk::version_equal(&before, &hub.version().unwrap()).unwrap());
}
