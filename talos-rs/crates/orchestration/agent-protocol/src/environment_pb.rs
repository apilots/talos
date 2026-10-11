//! Projections of the existing environment DTOs; no JSON business envelopes.
pub use crate::environment_config_pb::*;
pub use crate::environment_docs_pb::*;
pub use crate::environment_memory_pb::*;
pub use crate::environment_plugins_pb::*;
pub use crate::environment_skills_pb::*;
use crate::{harness_pb, WorkerEnvironmentContext};
use agent_pb as pb;
use std::path::PathBuf;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_types::*;

pub fn invalid() -> TalosError {
    TalosError::new(
        ErrorCode::InvalidRequest,
        "Worker environment RPC payload is invalid",
    )
}
pub fn path(value: PathBuf) -> TalosResult<String> {
    value.into_os_string().into_string().map_err(|_| invalid())
}
pub fn encode_context(value: WorkerEnvironmentContext) -> pb::WorkerEnvironmentContext {
    pb::WorkerEnvironmentContext {
        user_id: value.user_id,
        username: value.username,
        workspace_id: value.workspace_id,
    }
}
pub fn decode_context(
    value: Option<pb::WorkerEnvironmentContext>,
) -> TalosResult<WorkerEnvironmentContext> {
    let value = value.ok_or_else(invalid)?;
    if value.user_id.trim().is_empty()
        || value
            .workspace_id
            .as_ref()
            .is_some_and(|id| id.trim().is_empty())
    {
        return Err(invalid());
    }
    Ok(WorkerEnvironmentContext {
        user_id: value.user_id,
        username: value.username,
        workspace_id: value.workspace_id,
    })
}
pub fn encode_workspace(value: WorkspaceInfoResponse) -> TalosResult<pb::WorkspaceInfo> {
    Ok(pb::WorkspaceInfo {
        workspace_id: value.workspace_id,
        root_prefix: value.root_prefix,
        user_workspace_dir: value.user_workspace_dir,
    })
}
pub fn decode_workspace(value: pb::WorkspaceInfo) -> TalosResult<WorkspaceInfoResponse> {
    if value.workspace_id.trim().is_empty()
        || value.root_prefix.is_empty()
        || value.user_workspace_dir.is_empty()
    {
        return Err(invalid());
    }
    Ok(WorkspaceInfoResponse {
        workspace_id: value.workspace_id,
        root_prefix: value.root_prefix,
        user_workspace_dir: value.user_workspace_dir,
    })
}
pub fn encode_catalog(value: ModelListResponse) -> TalosResult<pb::ModelCatalog> {
    Ok(pb::ModelCatalog {
        models: value
            .models
            .into_iter()
            .map(harness_pb::encode_model)
            .collect(),
        default_model_selection: Some(harness_pb::encode_model_selection(
            value.default_model_selection,
        )),
    })
}
pub fn decode_catalog(value: pb::ModelCatalog) -> TalosResult<ModelListResponse> {
    Ok(ModelListResponse {
        models: value
            .models
            .into_iter()
            .map(|model| harness_pb::decode_model(Some(model)))
            .collect::<TalosResult<_>>()?,
        default_model_selection: harness_pb::decode_model_selection(value.default_model_selection)?,
    })
}
pub fn encode_read(value: FsDirectoryReadRequest) -> TalosResult<pb::DirectoryReadParams> {
    Ok(pb::DirectoryReadParams {
        path: value.path.map(path).transpose()?,
    })
}
pub fn decode_read(value: Option<pb::DirectoryReadParams>) -> TalosResult<FsDirectoryReadRequest> {
    Ok(FsDirectoryReadRequest {
        node_id: None,
        path: value.ok_or_else(invalid)?.path.map(PathBuf::from),
    })
}
pub fn encode_create(value: FsDirectoryCreateRequest) -> TalosResult<pb::DirectoryCreateParams> {
    Ok(pb::DirectoryCreateParams {
        parent: path(value.parent)?,
        name: value.name,
    })
}
pub fn decode_create(
    value: Option<pb::DirectoryCreateParams>,
) -> TalosResult<FsDirectoryCreateRequest> {
    let value = value.ok_or_else(invalid)?;
    Ok(FsDirectoryCreateRequest {
        node_id: None,
        parent: value.parent.into(),
        name: value.name,
    })
}
pub fn encode_directory(value: FsDirectorySnapshot) -> TalosResult<pb::DirectorySnapshot> {
    Ok(pb::DirectorySnapshot {
        path: path(value.path)?,
        parent: value.parent.map(path).transpose()?,
        readable: value.readable,
        writable: value.writable,
        entries: value
            .entries
            .into_iter()
            .map(|entry| {
                Ok(pb::DirectoryEntry {
                    name: entry.name,
                    path: path(entry.path)?,
                    kind: match entry.kind {
                        FsEntryKind::Directory => pb::FsEntryKind::Directory as i32,
                    },
                    readable: entry.readable,
                    writable: entry.writable,
                    hidden: entry.hidden,
                    is_symlink: entry.is_symlink,
                })
            })
            .collect::<TalosResult<_>>()?,
    })
}
pub fn decode_directory(value: pb::DirectorySnapshot) -> TalosResult<FsDirectorySnapshot> {
    Ok(FsDirectorySnapshot {
        path: value.path.into(),
        parent: value.parent.map(PathBuf::from),
        readable: value.readable,
        writable: value.writable,
        entries: value
            .entries
            .into_iter()
            .map(|entry| {
                Ok(FsDirectoryEntry {
                    name: entry.name,
                    path: entry.path.into(),
                    kind: match pb::FsEntryKind::try_from(entry.kind) {
                        Ok(pb::FsEntryKind::Directory) => FsEntryKind::Directory,
                        _ => return Err(invalid()),
                    },
                    readable: entry.readable,
                    writable: entry.writable,
                    hidden: entry.hidden,
                    is_symlink: entry.is_symlink,
                })
            })
            .collect::<TalosResult<_>>()?,
    })
}
pub fn encode_created(value: FsDirectoryCreateResponse) -> TalosResult<pb::DirectoryCreated> {
    Ok(pb::DirectoryCreated {
        path: path(value.path)?,
    })
}
pub fn decode_created(value: pb::DirectoryCreated) -> TalosResult<FsDirectoryCreateResponse> {
    Ok(FsDirectoryCreateResponse {
        path: value.path.into(),
    })
}
pub fn encode_validated(value: PathBuf) -> TalosResult<pb::ValidatedDirectory> {
    Ok(pb::ValidatedDirectory {
        directory: path(value)?,
    })
}
pub fn decode_validated(value: pb::ValidatedDirectory) -> TalosResult<PathBuf> {
    Ok(value.directory.into())
}
pub fn directory(value: Option<pb::ProjectDirectoryParams>) -> TalosResult<PathBuf> {
    Ok(value.ok_or_else(invalid)?.directory.into())
}
pub fn encode_status(value: ProjectStatus) -> TalosResult<pb::ProjectDirectoryStatus> {
    Ok(pb::ProjectDirectoryStatus {
        state: match value.state {
            ProjectStatusState::Available => pb::ProjectDirectoryState::Available,
            ProjectStatusState::Missing => pb::ProjectDirectoryState::Missing,
            ProjectStatusState::NotDirectory => pb::ProjectDirectoryState::NotDirectory,
            ProjectStatusState::NodeUnavailable => pb::ProjectDirectoryState::NodeUnavailable,
            ProjectStatusState::PermissionDenied => pb::ProjectDirectoryState::PermissionDenied,
        } as i32,
        reason: value.reason.map(|reason| match reason {
            ProjectUnavailableReason::PathNotFound => pb::ProjectDirectoryReason::PathNotFound,
            ProjectUnavailableReason::NotDirectory => pb::ProjectDirectoryReason::NotDirectory,
            ProjectUnavailableReason::NodeUnavailable => {
                pb::ProjectDirectoryReason::NodeUnavailable
            }
            ProjectUnavailableReason::PermissionDenied => {
                pb::ProjectDirectoryReason::PermissionDenied
            }
        } as i32),
    })
}
pub fn decode_status(value: pb::ProjectDirectoryStatus) -> TalosResult<ProjectStatus> {
    Ok(ProjectStatus {
        state: match pb::ProjectDirectoryState::try_from(value.state) {
            Ok(pb::ProjectDirectoryState::NodeUnavailable) => ProjectStatusState::NodeUnavailable,
            Ok(pb::ProjectDirectoryState::Available) => ProjectStatusState::Available,
            Ok(pb::ProjectDirectoryState::Missing) => ProjectStatusState::Missing,
            Ok(pb::ProjectDirectoryState::NotDirectory) => ProjectStatusState::NotDirectory,
            Ok(pb::ProjectDirectoryState::PermissionDenied) => ProjectStatusState::PermissionDenied,
            _ => return Err(invalid()),
        },
        reason: value
            .reason
            .map(
                |reason| match pb::ProjectDirectoryReason::try_from(reason) {
                    Ok(pb::ProjectDirectoryReason::NodeUnavailable) => {
                        Ok(ProjectUnavailableReason::NodeUnavailable)
                    }
                    Ok(pb::ProjectDirectoryReason::PathNotFound) => {
                        Ok(ProjectUnavailableReason::PathNotFound)
                    }
                    Ok(pb::ProjectDirectoryReason::NotDirectory) => {
                        Ok(ProjectUnavailableReason::NotDirectory)
                    }
                    Ok(pb::ProjectDirectoryReason::PermissionDenied) => {
                        Ok(ProjectUnavailableReason::PermissionDenied)
                    }
                    _ => Err(invalid()),
                },
            )
            .transpose()?,
    })
}

pub use super::environment_oauth_pb::*;
