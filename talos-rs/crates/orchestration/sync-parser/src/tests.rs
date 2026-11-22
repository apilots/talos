use super::*;
use crate::metadata::{decode_snapshot, encode_snapshot};
use talos_sync_types::SyncScope;

#[test]
fn file_metadata_keys_and_atomic_scope_round_trip() {
    let binding = SyncBinding {
        scope: SyncScope {
            hub_id: "hub".into(),
            account_id: "account".into(),
        },
        peer_id: "peer".into(),
        source_user_id: "account".into(),
        computers: vec![],
    };
    let snapshot = SyncResourceSnapshot::File(serde_json::from_value(serde_json::json!({
        "file": {"fileId":"file-1","originalFilename":"image.png","mimeType":"image/png","sizeBytes":42,
                 "scope":{"type":"personal"},"tags":[],"metadata":{"a/b":"slash","~key":"tilde"},
                 "createdAt":1,"updatedAt":1},
        "storagePeerId":"peer"
    })).unwrap());
    let resource = SyncResourceKey::File {
        file_id: "file-1".into(),
    };
    let (_, fields) = encode_snapshot(&binding, &resource, snapshot.clone()).unwrap();
    assert_eq!(fields["/data/file/metadata/a~1b"], "slash");
    assert_eq!(fields["/data/file/metadata/~0key"], "tilde");
    assert_eq!(
        fields["/data/file/scope"],
        serde_json::json!({"type":"personal"})
    );
    assert_eq!(decode_snapshot(fields).unwrap(), snapshot);
}
