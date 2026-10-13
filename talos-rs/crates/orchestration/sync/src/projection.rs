use serde::{de::DeserializeOwned, Serialize};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_sync_types::{generated, SyncChange as WireChange, SyncResourceSnapshot as WireSnapshot};

use crate::{SyncBinding, SyncChange, SyncResourceKey, SyncResourceSnapshot};

pub(crate) fn resource(binding: &SyncBinding, key: &SyncResourceKey) -> SyncResourceKey {
    match key {
        SyncResourceKey::Computer {
            computer_id: source,
        } => SyncResourceKey::Computer {
            computer_id: computer_id(binding, source),
        },
        _ => key.clone(),
    }
}

pub(crate) fn change(binding: &SyncBinding, change: SyncChange) -> TalosResult<Option<WireChange>> {
    if let SyncChange::Metadata { before, after } = change {
        let after = Box::new(snapshot(binding, *after)?);
        let before = before
            .map(|value| snapshot(binding, *value).map(Box::new))
            .transpose()?;
        // Registration heartbeats change timestamps but are not metadata facts.
        let mut compared_before = before.clone();
        if let Some(old) = &mut compared_before {
            if let (WireSnapshot::Computer(old), WireSnapshot::Computer(new)) =
                (old.as_mut(), after.as_ref())
            {
                old.updated_at = new.updated_at;
            }
        }
        if compared_before.as_ref() == Some(&after) {
            return Ok(None);
        }
        return Ok(Some(WireChange::Metadata { before, after }));
    }
    transcode(&change).map(Some)
}

fn snapshot(binding: &SyncBinding, snapshot: SyncResourceSnapshot) -> TalosResult<WireSnapshot> {
    Ok(match snapshot {
        SyncResourceSnapshot::Computer(mut node) => {
            match node.owner_user_id.as_deref() {
                Some(owner) if owner != binding.scope.account_id => {
                    return Err(TalosError::new(
                        ErrorCode::PermissionDenied,
                        "Computer belongs to a different account.",
                    ));
                }
                None if binding
                    .computers
                    .iter()
                    .any(|alias| alias.source_node_id == node.node_id) =>
                {
                    // The authenticated host explicitly claimed this alias at
                    // bind_account. This projection does not change local Auth.
                    node.owner_user_id = Some(binding.scope.account_id.clone());
                }
                _ => {}
            }
            node.node_id = computer_id(binding, &node.node_id);
            let mut value = serde_json::to_value(node).map_err(codec_error)?;
            for field in [
                "health",
                "leaseExpiresAt",
                "lastHeartbeatAt",
                "available",
                "credentialExpiresAt",
            ] {
                value
                    .as_object_mut()
                    .expect("canonical NodeInfo is an object")
                    .remove(field);
            }
            WireSnapshot::Computer(
                serde_json::from_value::<generated::NodeInfo>(value).map_err(codec_error)?,
            )
        }
        SyncResourceSnapshot::Project(mut project) => {
            project.node_id = computer_id(binding, &project.node_id);
            project.status = talos_types::ProjectStatus {
                state: talos_types::ProjectStatusState::NodeUnavailable,
                reason: Some(talos_types::ProjectUnavailableReason::NodeUnavailable),
            };
            WireSnapshot::Project(transcode(&project)?)
        }
        SyncResourceSnapshot::Session(mut session) => {
            session.session.node_id = computer_id(binding, &session.session.node_id);
            session.binding.computer_id = computer_id(binding, &session.binding.computer_id);
            WireSnapshot::Session(Box::new(transcode(&session)?))
        }

        SyncResourceSnapshot::File(file) => WireSnapshot::File(transcode(&file)?),
    })
}

fn computer_id(binding: &SyncBinding, source: &str) -> String {
    binding
        .computers
        .iter()
        .find(|alias| alias.source_node_id == source)
        .map_or_else(|| source.to_owned(), |alias| alias.computer_id.clone())
}

pub(crate) fn transcode<T: Serialize, U: DeserializeOwned>(source: &T) -> TalosResult<U> {
    serde_json::from_value(serde_json::to_value(source).map_err(codec_error)?).map_err(codec_error)
}

pub(crate) fn codec_error(error: serde_json::Error) -> TalosError {
    TalosError::internal_with_source(
        "Synchronize canonical data; technical details were withheld.",
        error,
    )
}
