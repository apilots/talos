//! One header contract for native identity, association and immutable bindings.
use crate::types::*;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_sync_types::{SyncFields, SyncHeader};

pub(super) fn snapshot_header(
    resource: &SyncResourceKey,
    account: &str,
    snapshot: &SyncResourceSnapshot,
    fields: &SyncFields,
) -> TalosResult<SyncHeader> {
    let mut header = SyncHeader::default();
    let fixed: &[&str] = match (resource, snapshot) {
        (SyncResourceKey::Computer { computer_id }, SyncResourceSnapshot::Computer(node))
            if computer_id == &node.node_id =>
        {
            if node.owner_user_id.as_deref() != Some(account) {
                return Err(TalosError::new(
                    ErrorCode::PermissionDenied,
                    "Computer belongs to a different account.",
                ));
            }
            header.owner_account = Some(account.to_owned());
            header.related = Some(resource.wire());
            &["/data/ownerUserId"]
        }
        (SyncResourceKey::Project { project_id }, SyncResourceSnapshot::Project(project))
            if project_id == &project.project_id =>
        {
            header.related = Some(
                SyncResourceKey::Computer {
                    computer_id: project.node_id.clone(),
                }
                .wire(),
            );
            &["/data/nodeId", "/data/directory"]
        }
        (SyncResourceKey::Session { session_id }, SyncResourceSnapshot::Session(session))
            if session_id == &session.session.id
                && session.session.node_id == session.binding.computer_id =>
        {
            header.related = Some(
                SyncResourceKey::Computer {
                    computer_id: session.binding.computer_id.clone(),
                }
                .wire(),
            );
            &[
                "/data/session/nodeId",
                "/data/binding",
                "/data/treeIdentity",
            ]
        }
        (SyncResourceKey::File { file_id }, SyncResourceSnapshot::File(file))
            if file_id == &file.file.file_id =>
        {
            &["/data/storagePeerId"]
        }
        _ => return Err(invalid_binding()),
    };
    header.immutable.extend(
        fixed
            .iter()
            .filter_map(|key| fields.get(*key).map(|value| ((*key).into(), value.clone()))),
    );
    Ok(header)
}

pub(super) fn invalid_binding() -> TalosError {
    TalosError::new(
        ErrorCode::Conflict,
        "The synchronized Talos identity or binding differs.",
    )
}

pub(super) fn validate_execution(binding: &SessionExecutionBinding) -> TalosResult<()> {
    if binding.agent_id.is_empty()
        || binding.workspace_id.is_empty()
        || binding.workspace_user_id.is_empty()
    {
        return Err(TalosError::new(
            ErrorCode::Conflict,
            "Synchronized Session execution binding is incomplete.",
        ));
    }
    Ok(())
}
