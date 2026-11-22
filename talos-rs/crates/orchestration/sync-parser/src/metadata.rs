//! Shared native metadata encoding and independent field-key selection.
use crate::{
    codec_error,
    header::{invalid_binding, snapshot_header, validate_execution},
    records,
    types::*,
};
use serde_json::{Map, Value};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_sync_types::{SyncChange as WireChange, SyncFieldEdit, SyncFields, SyncHeader};

const ATOMIC_FIELDS: &[&str] = &[
    "/data/conf/modelSelection",
    "/data/session/modelSelection",
    "/data/environment",
    "/data/memoryProcessedThrough",
    "/data/file/scope",
    "/data/binding",
    "/data/treeIdentity",
    "/data/status",
];

pub fn resource(binding: &SyncBinding, key: &SyncResourceKey) -> SyncResourceKey {
    match key {
        SyncResourceKey::Computer {
            computer_id: source,
        } => SyncResourceKey::Computer {
            computer_id: computer_id(binding, source),
        },
        _ => key.clone(),
    }
}

pub fn encode_change(
    binding: &SyncBinding,
    resource: &SyncResourceKey,
    change: SyncChange,
) -> TalosResult<Option<WireChange>> {
    let SyncChange::Metadata { before, after } = change else {
        return records::encode(resource, &change).map(Some);
    };
    let computer = matches!(after.as_ref(), SyncResourceSnapshot::Computer(_));
    let (header, after) = encode_snapshot(binding, resource, *after)?;
    let before = before
        .map(|snapshot| encode_snapshot(binding, resource, *snapshot))
        .transpose()?;
    if let Some((previous, fields)) = &before {
        if previous != &header {
            return Err(invalid_binding());
        }
        if fields.len() == after.len()
            && fields.iter().all(|(key, value)| {
                (computer && key == "/data/updatedAt") || after.get(key) == Some(value)
            })
        {
            return Ok(None);
        }
    }
    Ok(Some(WireChange::fields(
        header,
        before.as_ref().map(|(_, fields)| fields),
        &after,
    )))
}

pub(super) fn encode_snapshot(
    binding: &SyncBinding,
    resource: &SyncResourceKey,
    mut snapshot: SyncResourceSnapshot,
) -> TalosResult<(SyncHeader, SyncFields)> {
    match &mut snapshot {
        SyncResourceSnapshot::Computer(node) => {
            if node
                .owner_user_id
                .as_deref()
                .is_some_and(|owner| owner != binding.scope.account_id)
            {
                return Err(TalosError::new(
                    ErrorCode::PermissionDenied,
                    "Computer belongs to a different account.",
                ));
            }
            if node.owner_user_id.is_none()
                && binding
                    .computers
                    .iter()
                    .any(|alias| alias.source_node_id == node.node_id)
            {
                node.owner_user_id = Some(binding.scope.account_id.clone());
            }
            node.node_id = computer_id(binding, &node.node_id);
        }
        SyncResourceSnapshot::Project(project) => {
            project.node_id = computer_id(binding, &project.node_id);
            project.status = talos_types::ProjectStatus {
                state: talos_types::ProjectStatusState::NodeUnavailable,
                reason: Some(talos_types::ProjectUnavailableReason::NodeUnavailable),
            };
        }
        SyncResourceSnapshot::Session(session) => {
            validate_execution(&session.binding)?;
            session.session.node_id = computer_id(binding, &session.session.node_id);
            session.binding.computer_id = computer_id(binding, &session.binding.computer_id);
        }
        SyncResourceSnapshot::File(_) => {}
    }
    let mut value = serde_json::to_value(&snapshot).map_err(codec_error)?;
    if value["kind"] == "computer" {
        let node = value["data"]
            .as_object_mut()
            .expect("serialized NodeInfo object");
        for field in [
            "health",
            "leaseExpiresAt",
            "lastHeartbeatAt",
            "available",
            "credentialExpiresAt",
            "managedWorkerPhase",
        ] {
            node.remove(field);
        }
    }
    let mut fields = SyncFields::new();
    flatten("", value, &mut fields);
    let header = snapshot_header(resource, &binding.scope.account_id, &snapshot, &fields)?;
    Ok((header, fields))
}

fn computer_id(binding: &SyncBinding, source: &str) -> String {
    binding
        .computers
        .iter()
        .find(|alias| alias.source_node_id == source)
        .map_or_else(|| source.to_owned(), |alias| alias.computer_id.clone())
}

fn flatten(path: &str, value: Value, fields: &mut SyncFields) {
    match value {
        Value::Object(object) if !object.is_empty() && !ATOMIC_FIELDS.contains(&path) => {
            for (key, value) in object {
                let key = key.replace('~', "~0").replace('/', "~1");
                flatten(&format!("{path}/{key}"), value, fields);
            }
        }
        value => {
            fields.insert(path.to_owned(), value);
        }
    }
}

pub fn decode_changes(
    resource: &SyncResourceKey,
    account: &str,
    changes: Vec<WireChange>,
) -> TalosResult<Vec<SyncChange>> {
    changes
        .into_iter()
        .map(|change| match change {
            WireChange::Fields { header, edits } => {
                let fields = edits
                    .into_iter()
                    .map(|edit| match edit {
                        SyncFieldEdit::Put { key, value } => Ok((key, value)),
                        SyncFieldEdit::Delete { .. } => Err(invalid_field_path()),
                    })
                    .collect::<TalosResult<SyncFields>>()?;
                let snapshot = decode_snapshot(fields.clone())?;
                if snapshot_header(resource, account, &snapshot, &fields)? != header {
                    return Err(invalid_binding());
                }
                Ok(SyncChange::Metadata {
                    before: None,
                    after: Box::new(snapshot),
                })
            }
            change => records::decode(resource, change),
        })
        .collect()
}

pub(super) fn decode_snapshot(fields: SyncFields) -> TalosResult<SyncResourceSnapshot> {
    let mut root = Value::Object(Map::new());
    for (path, value) in fields {
        insert_path(&mut root, &path, value)?;
    }
    if root["kind"] == "computer" {
        let node = root["data"]
            .as_object_mut()
            .ok_or_else(invalid_field_path)?;
        node.insert(
            "health".into(),
            serde_json::to_value(talos_types::NodeHealth::Unknown).map_err(codec_error)?,
        );
        node.insert("available".into(), false.into());
    }
    let snapshot = serde_json::from_value(root).map_err(codec_error)?;
    if let SyncResourceSnapshot::Session(session) = &snapshot {
        validate_execution(&session.binding)?;
    }
    Ok(snapshot)
}

fn insert_path(root: &mut Value, path: &str, field: Value) -> TalosResult<()> {
    let path = path.strip_prefix('/').ok_or_else(invalid_field_path)?;
    let mut segments = path.split('/').peekable();
    let mut value = root;
    while let Some(segment) = segments.next() {
        if segment.is_empty() {
            return Err(invalid_field_path());
        }
        let key = segment.replace("~1", "/").replace("~0", "~");
        let object = value.as_object_mut().ok_or_else(invalid_field_path)?;
        if segments.peek().is_none() {
            return if object.insert(key, field).is_none() {
                Ok(())
            } else {
                Err(invalid_field_path())
            };
        }
        value = object
            .entry(key)
            .or_insert_with(|| Value::Object(Map::new()));
    }
    Err(invalid_field_path())
}

fn invalid_field_path() -> TalosError {
    TalosError::new(
        ErrorCode::InvalidRequest,
        "Invalid synchronized metadata path.",
    )
}
