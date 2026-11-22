//! Regenerate golden replicas with the immutable released SDK, never a local patch.
use loro::ToJson;
use serde_json::json;
use std::path::Path;
use talos_sync_sdk::*;
const COMPUTER: &str = "d4383755-33d1-43fd-a6ef-cf14dc2a823d";
fn session(id: &str) -> SyncResourceSnapshot {
    SyncResourceSnapshot::Session(Box::new(serde_json::from_value(json!({
        "session":{"id":id,"nodeId":COMPUTER,"userId":"workspace-owner","title":"Released history","sourceType":"desktop","sourceProvider":"sdk","modelSelection":{"modelRef":{"provider":"original","id":"configured"},"reasoningEffort":null},"configuredDefaultMode":"askForApproval","effectiveMode":"askForApproval","createdAt":1,"lastActivity":11,"messageCount":4,"currentDirectory":"/worker/project","projectId":null},
        "conf":{"modelSelection":null,"userPermissionMode":null},"environment":null,"channelId":null,"threadId":null,
        "binding":{"computerId":COMPUTER,"agentId":"talos","workspaceId":id,"workspaceUserId":"workspace-owner"},
        "treeIdentity":{"type":"primary"},"memoryProcessedThrough":null
    })).unwrap()))
}
fn turn(doc: &mut SyncReplica, session: &str, index: usize, peer: &str, previous: Option<String>) {
    let id = json!({"sessionId":session,"turnId":format!("turn-{index}")});
    let mut changes: Vec<SyncChange> = vec![SyncChange::Turn(
        serde_json::from_value(
            json!({"identity":id,"coordinatorId":peer,"previousTurnId":previous}),
        )
        .unwrap(),
    )];
    let block = json!({"blockId":format!("block-{index}"),"content":{"type":"userContent","data":{"type":"text","text":format!("Released history Turn {index}")}},"startedAt":10,"completedAt":10,"status":"completed"});
    let message = json!({"messageId":format!("message-{index}"),"turnId":format!("turn-{index}"),"role":"user","status":"completed","blocks":[block.clone()],"createdAt":10,"completedAt":10});
    let mut started_message = message.clone();
    started_message["status"] = json!("inProgress");
    started_message["blocks"] = json!([]);
    started_message["completedAt"] = json!(null);
    let mut started_block = block.clone();
    started_block["status"] = json!("running");
    started_block["completedAt"] = json!(null);
    let payloads = [
        json!({"type":"turnStarted","data":{}}),
        json!({"type":"messageStarted","data":{"message":started_message}}),
        json!({"type":"messageBlockStarted","data":{"messageId":format!("message-{index}"),"block":started_block}}),
        json!({"type":"messageBlockCompleted","data":{"messageId":format!("message-{index}"),"block":block}}),
        json!({"type":"messageCompleted","data":message}),
        json!({"type":"turnCompleted","data":{"stopReason":"endTurn"}}),
    ];
    for (offset, payload) in payloads.into_iter().enumerate() {
        let sequence = offset + 1;
        changes.push(SyncChange::Event(serde_json::from_value(json!({"key":{"type":"agent","identity":id,"sequence":sequence},"event":{"type":"agent","data":{"identity":id,"sequence":sequence,"occurredAt":10,"payload":payload}},"persistedAt":11})).unwrap()));
    }
    doc.apply(&changes).unwrap();
}
fn save(out: &Path, name: &str, doc: &SyncReplica) {
    std::fs::write(out.join(format!("{name}.bin")), doc.snapshot().unwrap()).unwrap();
    std::fs::write(out.join(format!("{name}.json")),serde_json::to_vec_pretty(&json!({"sdk":"0.1.7","version":doc.version().unwrap(),"changes":doc.changes().unwrap()})).unwrap()).unwrap();
}
fn envelopes(out: &Path) {
    let key = session("released-session").resource();
    let mut doc = SyncReplica::from_snapshot(
        key.clone(),
        &std::fs::read(out.join("survivor-b.bin")).unwrap(),
    )
    .unwrap();
    doc.set_writer_id(505).unwrap();
    let identity = json!({"sessionId":"released-session","turnId":"turn-4"});
    let last = json!({"type":"agent","identity":identity,"sequence":6});
    doc.apply(&[
        SyncChange::ActiveTurnMetadata(serde_json::from_value(json!({"identity":identity,"state":"waitingForInput","updatedAt":99,"lastEvent":last})).unwrap()),
        SyncChange::AcceptedCommand(serde_json::from_value(json!({"command":"turn_submission","idempotencyKey":"original-key","requestId":"original-request","requestHash":"original-hash","identity":identity,"messageId":"message-4","status":"accepted","committedEvent":last,"expiresAt":null})).unwrap()),
        SyncChange::Event(serde_json::from_value(json!({"key":{"type":"notice","peerId":"original-notice-source","sourceEventId":"source-event-1"},"event":{"type":"systemNotice","data":{"identity":identity,"occurredAt":77,"notice":{"level":"info","content":{"type":"text","data":"Original released notice"}}}},"persistedAt":78})).unwrap()),
    ]).unwrap();
    // Missing optional fields are valid released data. Remove one through Loro
    // and validate the resulting complete replica with the immutable SDK.
    let raw = loro::LoroDoc::new();
    raw.import(&doc.snapshot().unwrap()).unwrap();
    raw.set_peer_id(606).unwrap();
    let commands = raw.get_map("commands:0");
    let deep = commands.get_deep_value().to_json_value();
    let (key_name, encoded) = deep.as_object().unwrap().iter().next().unwrap();
    let mut command: serde_json::Value = serde_json::from_str(encoded.as_str().unwrap()).unwrap();
    command.as_object_mut().unwrap().remove("expiresAt");
    commands
        .insert(key_name, serde_json::to_string(&command).unwrap())
        .unwrap();
    let checked =
        SyncReplica::from_snapshot(key, &raw.export(loro::ExportMode::Snapshot).unwrap()).unwrap();
    save(out, "envelopes", &checked);
}
fn main() {
    let out = std::path::PathBuf::from(std::env::args_os().nth(1).expect("output directory"));
    std::fs::create_dir_all(&out).unwrap();
    if std::env::args().nth(2).as_deref() == Some("envelopes") {
        envelopes(&out);
        return;
    }
    let metadata = session("released-session");
    let mut a = SyncReplica::new(metadata.resource(), 101).unwrap();
    a.apply(&[SyncChange::Metadata {
        before: None,
        after: Box::new(metadata.clone()),
    }])
    .unwrap();
    for n in 1..=2 {
        turn(
            &mut a,
            "released-session",
            n,
            "original-A",
            (n > 1).then(|| format!("turn-{}", n - 1)),
        );
    }
    save(&out, "source-a", &a);
    let mut b = SyncReplica::from_snapshot(metadata.resource(), &a.snapshot().unwrap()).unwrap();
    b.set_writer_id(202).unwrap();
    for n in 3..=4 {
        turn(
            &mut b,
            "released-session",
            n,
            "original-B",
            Some(format!("turn-{}", n - 1)),
        );
    }
    save(&out, "survivor-b", &b);
    // Keep an exported, unacknowledged local edit whose later remote merge wins.
    let mut materialized =
        SyncReplica::from_snapshot(metadata.resource(), &b.snapshot().unwrap()).unwrap();
    materialized.set_writer_id(202).unwrap();
    let SyncResourceSnapshot::Session(mut local) = metadata.clone() else {
        unreachable!()
    };
    local.session.title = "Materialized local title".into();
    let local = SyncResourceSnapshot::Session(local);
    let exported = vec![SyncChange::Metadata {
        before: Some(Box::new(metadata.clone())),
        after: Box::new(local.clone()),
    }];
    materialized.apply(&exported).unwrap();
    let cut = materialized.version().unwrap();
    let mut remote =
        SyncReplica::from_snapshot(local.resource(), &materialized.snapshot().unwrap()).unwrap();
    remote.set_writer_id(404).unwrap();
    let SyncResourceSnapshot::Session(mut next) = local.clone() else {
        unreachable!()
    };
    next.session.title = "Newer merged remote title".into();
    remote
        .apply(&[SyncChange::Metadata {
            before: Some(Box::new(local)),
            after: Box::new(SyncResourceSnapshot::Session(next)),
        }])
        .unwrap();
    materialized.import(&[remote.snapshot().unwrap()]).unwrap();
    save(&out, "materialized-b", &materialized);
    let path = out.join("materialized-b.json");
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    value["exported"] = json!({"version":cut,"changes":exported});
    std::fs::write(path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    let SyncResourceSnapshot::Session(mut renamed) = metadata.clone() else {
        unreachable!()
    };
    renamed.session.title = "Offline pending title".into();
    let pending = vec![SyncChange::Metadata {
        before: Some(Box::new(metadata)),
        after: Box::new(SyncResourceSnapshot::Session(renamed)),
    }];
    std::fs::write(
        out.join("pending.json"),
        serde_json::to_vec_pretty(&pending).unwrap(),
    )
    .unwrap();
    let metadata = session("released-deleted");
    let mut deleted = SyncReplica::new(metadata.resource(), 303).unwrap();
    deleted
        .apply(&[SyncChange::Metadata {
            before: None,
            after: Box::new(metadata),
        }])
        .unwrap();
    turn(&mut deleted, "released-deleted", 1, "original-A", None);
    deleted
        .apply(&[SyncChange::Reset {
            history_generation: 1,
        }])
        .unwrap();
    turn(&mut deleted, "released-deleted", 2, "original-B", None);
    deleted.apply(&[SyncChange::Delete]).unwrap();
    save(&out, "deleted", &deleted);
}
