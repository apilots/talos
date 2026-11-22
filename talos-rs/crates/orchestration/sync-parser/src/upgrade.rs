//! One-time conversion of the released SDK 0.1.7 replica format.
//! Remap map operations without replacing their IDs, dependencies or Lamport clocks.
use crate::{
    codec_error, header::snapshot_header, metadata::decode_snapshot, SyncBinding, SyncResourceKey,
};
use loro::{
    ContainerID, ContainerType, ExportMode, JsonMapOp, JsonOpContent, LoroDoc, LoroValue, ToJson,
    VersionVector,
};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_sync_types::{SyncRecord, SyncStream};

pub fn upgrade_v1_snapshot(
    binding: &SyncBinding,
    resource: &SyncResourceKey,
    snapshot: &[u8],
    expected: &talos_sync_types::SyncVersion,
) -> TalosResult<Option<Vec<u8>>> {
    let old = LoroDoc::new();
    let imported = old.import(snapshot).map_err(invalid)?;
    if imported.pending.is_some() {
        return Err(conflict());
    }
    let schema: u64 = value(&old, "identity", "schema")?;
    if schema == 2 {
        return Ok(None);
    }
    if schema != 1 {
        return Err(conflict());
    }
    if old.oplog_vv() != VersionVector::decode(&expected.version_vector).map_err(invalid)? {
        return Err(conflict());
    }
    if value::<SyncResourceKey>(&old, "identity", "resource")? != *resource {
        return Err(conflict());
    }
    let metadata = old.get_map("metadata").get_deep_value().to_json_value();
    let fields: talos_sync_types::SyncFields = metadata
        .as_object()
        .ok_or_else(conflict)?
        .iter()
        .map(|(key, value)| {
            Ok((
                key.clone(),
                serde_json::from_str(value.as_str().ok_or_else(conflict)?).map_err(codec_error)?,
            ))
        })
        .collect::<TalosResult<_>>()?;
    let native = decode_snapshot(fields.clone())?;
    let header = snapshot_header(resource, &binding.scope.account_id, &native, &fields)?;
    let mut updates = old
        .export_json_updates_without_peer_compression(&VersionVector::default(), &old.oplog_vv());
    for change in &mut updates.changes {
        for op in &mut change.ops {
            let ContainerID::Root {
                name,
                container_type: ContainerType::Map,
            } = &op.container
            else {
                return Err(conflict());
            };
            let name = name.to_string();
            let JsonOpContent::Map(content) = &mut op.content else {
                return Err(conflict());
            };
            let next = match name.as_str() {
                "identity" => {
                    let JsonMapOp::Insert { key, value } = content else {
                        return Err(conflict());
                    };
                    match key.as_str() {
                        "schema" => {
                            if decode::<u64>(value)? != 1 {
                                return Err(conflict());
                            }
                            *value = encoded(&2_u64)?;
                        }
                        "resource" => {
                            if decode::<SyncResourceKey>(value)? != *resource {
                                return Err(conflict());
                            }
                            *value = encoded(&resource.wire())?;
                        }
                        "deleted" => {
                            if !decode::<bool>(value)? {
                                return Err(conflict());
                            }
                        }
                        _ => return Err(conflict()),
                    }
                    name
                }
                "metadata" => "fields".into(),
                "generations" => name,
                _ => remap_records(resource, &name, content)?,
            };
            op.container = ContainerID::new_root(&next, ContainerType::Map);
        }
    }
    let next = LoroDoc::new();
    if next
        .import_json_updates(updates)
        .map_err(invalid)?
        .pending
        .is_some()
    {
        return Err(conflict());
    }
    let writer = header_writer(binding, resource);
    if old.oplog_vv().iter().any(|(peer, _)| *peer == writer) {
        return Err(conflict());
    }
    // An independent operation has identical ID, dependencies and Lamport clock
    // on Hub and local replicas even when their unacknowledged histories differ.
    let header_doc = LoroDoc::new();
    header_doc.set_record_timestamp(false);
    header_doc.set_peer_id(writer).map_err(invalid)?;
    header_doc
        .get_map("identity")
        .insert(
            "header",
            serde_json::to_string(&header).map_err(codec_error)?,
        )
        .map_err(invalid)?;
    let status = next
        .import(&header_doc.export(ExportMode::Snapshot).map_err(invalid)?)
        .map_err(invalid)?;
    if status.pending.is_some() {
        return Err(conflict());
    }
    next.export(ExportMode::Snapshot).map(Some).map_err(invalid)
}
fn header_writer(binding: &SyncBinding, resource: &SyncResourceKey) -> u64 {
    let mut hash = Sha256::new();
    hash.update(b"talos-sync:replica-upgrade:v1-to-v2:header");
    let key = resource.wire();
    for value in [
        &binding.scope.hub_id,
        &binding.scope.account_id,
        &key.namespace,
        &key.id,
    ] {
        hash.update((value.len() as u64).to_be_bytes());
        hash.update(value.as_bytes());
    }
    u64::from_be_bytes(
        hash.finalize()[..8]
            .try_into()
            .expect("eight-byte hash prefix"),
    )
}
fn remap_records(
    resource: &SyncResourceKey,
    name: &str,
    content: &mut JsonMapOp,
) -> TalosResult<String> {
    if !matches!(resource, SyncResourceKey::Session { .. }) {
        return Err(conflict());
    }
    let (kind, generation) = name.split_once(':').ok_or_else(conflict)?;
    let parsed: u64 = generation.parse().map_err(invalid)?;
    if generation != parsed.to_string() {
        return Err(conflict());
    }
    let JsonMapOp::Insert { key, value } = content else {
        return Err(conflict());
    };
    let raw: Value = decode(value)?;
    let identity = |value: &Value| -> TalosResult<String> {
        if value.get("sessionId").and_then(Value::as_str) != Some(resource.id()) {
            return Err(conflict());
        }
        text(value, "turnId")
    };
    if kind == "turns" {
        let id = identity(&raw["identity"])?;
        if key != &id {
            return Err(conflict());
        }
        let stream = SyncStream {
            id,
            owner: text(&raw, "coordinatorId")?,
            previous: raw
                .get("previousTurnId")
                .filter(|value| !value.is_null())
                .map(|value| value.as_str().map(str::to_owned).ok_or_else(conflict))
                .transpose()?,
        };
        *value = encoded(&stream)?;
        return Ok(format!("streams:{generation}"));
    }
    let (label, stream, publisher, sequence, reference, replaceable) = match kind {
        "timeline" => {
            let event_key = &raw["key"];
            let stable: crate::SyncEventKey =
                serde_json::from_value(event_key.clone()).map_err(codec_error)?;
            if key != &serde_json::to_string(&stable).map_err(codec_error)? {
                return Err(conflict());
            }
            let event = &raw["event"];
            let stream = identity(&event["data"]["identity"])?;
            let (publisher, sequence) = match event_key["type"].as_str() {
                Some("agent") if event["type"] == "agent" => {
                    if identity(&event_key["identity"])? != stream
                        || event_key["sequence"] != event["data"]["sequence"]
                    {
                        return Err(conflict());
                    }
                    (
                        None,
                        Some(event_key["sequence"].as_u64().ok_or_else(conflict)?),
                    )
                }
                Some("notice") if event["type"] == "systemNotice" => {
                    if text(event_key, "sourceEventId")?.is_empty() {
                        return Err(conflict());
                    }
                    (Some(text(event_key, "peerId")?), None)
                }
                _ => return Err(conflict()),
            };
            *key = protocol_key("event", &stable)?;
            ("event", stream, publisher, sequence, None, false)
        }
        "active" => {
            let stream = identity(&raw["identity"])?;
            if key != &stream {
                return Err(conflict());
            }
            *key = protocol_key("active", &stream)?;
            (
                "activeTurnMetadata",
                stream,
                None,
                None,
                Some(event_reference(&raw["lastEvent"])?),
                true,
            )
        }
        "commands" => {
            let tuple =
                serde_json::json!([raw["command"], raw["idempotencyKey"], raw["requestId"]]);
            if key != &serde_json::to_string(&tuple).map_err(codec_error)? {
                return Err(conflict());
            }
            *key = protocol_key("command", &tuple)?;
            (
                "acceptedCommand",
                identity(&raw["identity"])?,
                None,
                None,
                Some(event_reference(&raw["committedEvent"])?),
                false,
            )
        }
        _ => return Err(conflict()),
    };
    *value = encoded(&SyncRecord {
        key: key.clone(),
        stream,
        publisher,
        sequence,
        reference,
        replaceable,
        // Preserve opaque historical JSON; current DTOs can add defaults when decoded.
        value: serde_json::json!({"type":label,"data":raw}),
    })?;
    Ok(format!("records:{generation}"))
}
fn protocol_key(kind: &str, value: &impl Serialize) -> TalosResult<String> {
    serde_json::to_string(&(kind, value)).map_err(codec_error)
}
fn event_reference(value: &Value) -> TalosResult<String> {
    let key: crate::SyncEventKey = serde_json::from_value(value.clone()).map_err(codec_error)?;
    protocol_key("event", &key)
}
fn text(value: &Value, key: &str) -> TalosResult<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(conflict)
}

fn value<T: DeserializeOwned>(doc: &LoroDoc, map: &str, key: &str) -> TalosResult<T> {
    let values = doc.get_map(map).get_deep_value().to_json_value();
    serde_json::from_str(
        values
            .get(key)
            .and_then(Value::as_str)
            .ok_or_else(conflict)?,
    )
    .map_err(codec_error)
}
fn decode<T: DeserializeOwned>(value: &LoroValue) -> TalosResult<T> {
    let LoroValue::String(value) = value else {
        return Err(conflict());
    };
    serde_json::from_str(value).map_err(codec_error)
}
fn encoded(value: &impl Serialize) -> TalosResult<LoroValue> {
    Ok(serde_json::to_string(value).map_err(codec_error)?.into())
}
fn invalid(error: impl std::error::Error + Send + Sync + 'static) -> TalosError {
    TalosError::from_source(
        ErrorCode::InvalidRequest,
        "Upgrade a retained synchronization replica.",
        error,
    )
}
fn conflict() -> TalosError {
    TalosError::new(
        ErrorCode::Conflict,
        "The retained synchronization replica cannot be upgraded safely.",
    )
}

#[cfg(test)]
#[path = "upgrade_tests.rs"]
mod tests;
