//! Wire projections preserve the existing document/version contract.
use super::environment_pb::invalid;
use agent_pb as pb;
use talos_error::TalosResult;
use talos_types::*;

fn encode_kind(kind: WorkspaceDocKind) -> i32 {
    (match kind {
        WorkspaceDocKind::Soul => pb::WorkspaceDocumentKind::Soul,
        WorkspaceDocKind::Identity => pb::WorkspaceDocumentKind::Identity,
        WorkspaceDocKind::User => pb::WorkspaceDocumentKind::User,
        WorkspaceDocKind::Heartbeat => pb::WorkspaceDocumentKind::Heartbeat,
        WorkspaceDocKind::Memory => pb::WorkspaceDocumentKind::Memory,
    }) as i32
}
fn decode_kind(kind: i32) -> TalosResult<WorkspaceDocKind> {
    Ok(match pb::WorkspaceDocumentKind::try_from(kind) {
        Ok(pb::WorkspaceDocumentKind::Soul) => WorkspaceDocKind::Soul,
        Ok(pb::WorkspaceDocumentKind::Identity) => WorkspaceDocKind::Identity,
        Ok(pb::WorkspaceDocumentKind::User) => WorkspaceDocKind::User,
        Ok(pb::WorkspaceDocumentKind::Heartbeat) => WorkspaceDocKind::Heartbeat,
        Ok(pb::WorkspaceDocumentKind::Memory) => WorkspaceDocKind::Memory,
        _ => return Err(invalid()),
    })
}
pub fn encode_list_workspace_docs_params(
    _: WorkspaceDocListRequest,
) -> TalosResult<pb::WorkspaceDocListParams> {
    Ok(pb::WorkspaceDocListParams {})
}
pub fn decode_list_workspace_docs_params(
    value: Option<pb::WorkspaceDocListParams>,
) -> TalosResult<WorkspaceDocListRequest> {
    value.ok_or_else(invalid)?;
    Ok(WorkspaceDocListRequest::default())
}
pub fn encode_read_workspace_doc_params(
    value: WorkspaceDocReadRequest,
) -> TalosResult<pb::WorkspaceDocReadParams> {
    Ok(pb::WorkspaceDocReadParams {
        kind: encode_kind(value.kind),
    })
}
pub fn decode_read_workspace_doc_params(
    value: Option<pb::WorkspaceDocReadParams>,
) -> TalosResult<WorkspaceDocReadRequest> {
    Ok(WorkspaceDocReadRequest {
        node_id: None,
        kind: decode_kind(value.ok_or_else(invalid)?.kind)?,
    })
}
pub fn encode_write_workspace_doc_params(
    value: WorkspaceDocWriteRequest,
) -> TalosResult<pb::WorkspaceDocWriteParams> {
    Ok(pb::WorkspaceDocWriteParams {
        kind: encode_kind(value.kind),
        content: value.content,
        expected_version: value.expected_version,
    })
}
pub fn decode_write_workspace_doc_params(
    value: Option<pb::WorkspaceDocWriteParams>,
) -> TalosResult<WorkspaceDocWriteRequest> {
    let value = value.ok_or_else(invalid)?;
    Ok(WorkspaceDocWriteRequest {
        node_id: None,
        kind: decode_kind(value.kind)?,
        content: value.content,
        expected_version: value.expected_version,
    })
}
pub fn encode_list_workspace_docs_result(
    value: WorkspaceDocListResponse,
) -> TalosResult<pb::WorkspaceDocuments> {
    Ok(pb::WorkspaceDocuments {
        data: value
            .data
            .into_iter()
            .map(|doc| pb::WorkspaceDocumentSummary {
                kind: encode_kind(doc.kind),
                path: doc.path,
                exists: doc.exists,
                updated_at: doc.updated_at,
                version_token: doc.version_token,
            })
            .collect(),
    })
}
pub fn decode_list_workspace_docs_result(
    value: pb::WorkspaceDocuments,
) -> TalosResult<WorkspaceDocListResponse> {
    Ok(WorkspaceDocListResponse {
        data: value
            .data
            .into_iter()
            .map(|doc| {
                Ok(WorkspaceDocSummary {
                    kind: decode_kind(doc.kind)?,
                    path: doc.path,
                    exists: doc.exists,
                    updated_at: doc.updated_at,
                    version_token: doc.version_token,
                })
            })
            .collect::<TalosResult<_>>()?,
    })
}
fn encode_doc(doc: WorkspaceDocInfo) -> pb::WorkspaceDocument {
    pb::WorkspaceDocument {
        kind: encode_kind(doc.kind),
        path: doc.path,
        exists: doc.exists,
        content: doc.content,
        updated_at: doc.updated_at,
        version_token: doc.version_token,
    }
}
fn decode_doc(doc: pb::WorkspaceDocument) -> TalosResult<WorkspaceDocInfo> {
    Ok(WorkspaceDocInfo {
        kind: decode_kind(doc.kind)?,
        path: doc.path,
        exists: doc.exists,
        content: doc.content,
        updated_at: doc.updated_at,
        version_token: doc.version_token,
    })
}
pub fn encode_read_workspace_doc_result(
    value: WorkspaceDocReadResponse,
) -> TalosResult<pb::WorkspaceDocument> {
    Ok(encode_doc(value.doc))
}
pub fn encode_write_workspace_doc_result(
    value: WorkspaceDocWriteResponse,
) -> TalosResult<pb::WorkspaceDocument> {
    Ok(encode_doc(value.doc))
}
pub fn decode_read_workspace_doc_result(
    value: pb::WorkspaceDocument,
) -> TalosResult<WorkspaceDocReadResponse> {
    Ok(WorkspaceDocReadResponse {
        doc: decode_doc(value)?,
    })
}
pub fn decode_write_workspace_doc_result(
    value: pb::WorkspaceDocument,
) -> TalosResult<WorkspaceDocWriteResponse> {
    Ok(WorkspaceDocWriteResponse {
        doc: decode_doc(value)?,
    })
}
