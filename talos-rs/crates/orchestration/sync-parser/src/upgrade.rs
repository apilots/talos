//! One-time conversion of the released SDK 0.1.7 replica format.
//! Remap map operations without replacing their IDs, dependencies or Lamport clocks.
use crate::{
    codec_error, header::snapshot_header, metadata::decode_snapshot, records, SyncBinding,
    SyncChange, SyncResourceKey, SyncTimelineEntry, SyncTurnMetadata,
};
use loro::{
    ContainerID, ContainerType, ExportMode, JsonMapOp, JsonOpContent, LoroDoc, LoroValue, ToJson,
    VersionVector,
};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_sync_types::SyncChange as WireChange;

pub fn upgrade_v1_snapshot(
    binding: &SyncBinding,
    resource: &SyncResourceKey,
    writer: u64,
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
    next.set_peer_id(writer).map_err(invalid)?;
    next.get_map("identity")
        .insert(
            "header",
            serde_json::to_string(&header).map_err(codec_error)?,
        )
        .map_err(invalid)?;
    next.export(ExportMode::Snapshot).map(Some).map_err(invalid)
}
fn remap_records(
    resource: &SyncResourceKey,
    name: &str,
    content: &mut JsonMapOp,
) -> TalosResult<String> {
    let (kind, generation) = name.split_once(':').ok_or_else(conflict)?;
    let parsed: u64 = generation.parse().map_err(invalid)?;
    if generation != parsed.to_string() {
        return Err(conflict());
    }
    let JsonMapOp::Insert { key, value } = content else {
        return Err(conflict());
    };
    let native = match kind {
        "turns" => {
            let turn: SyncTurnMetadata = decode(value)?;
            if key != &turn.identity.turn_id {
                return Err(conflict());
            }
            SyncChange::Turn(turn)
        }
        "timeline" => {
            let entry: SyncTimelineEntry = decode(value)?;
            if key != &serde_json::to_string(&entry.key).map_err(codec_error)? {
                return Err(conflict());
            }
            SyncChange::Event(entry)
        }
        "active" => {
            let active: crate::SyncActiveTurnMetadata = decode(value)?;
            if key != &active.identity.turn_id {
                return Err(conflict());
            }
            SyncChange::ActiveTurnMetadata(active)
        }
        "commands" => {
            let command: crate::SyncAcceptedCommand = decode(value)?;
            if key
                != &serde_json::to_string(&(
                    &command.command,
                    &command.idempotency_key,
                    &command.request_id,
                ))
                .map_err(codec_error)?
            {
                return Err(conflict());
            }
            SyncChange::AcceptedCommand(command)
        }
        _ => return Err(conflict()),
    };
    match records::encode(resource, &native)? {
        WireChange::Stream(stream) => {
            *key = stream.id.clone();
            *value = encoded(&stream)?;
            Ok(format!("streams:{generation}"))
        }
        WireChange::Record(record) => {
            *key = record.key.clone();
            *value = encoded(&record)?;
            Ok(format!("records:{generation}"))
        }
        _ => Err(conflict()),
    }
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
