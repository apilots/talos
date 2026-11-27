//! Authorized control-plane resources consumed through existing Worker ports.

#[cfg(feature = "server")]
use std::pin::Pin;
use std::sync::Arc;

#[cfg(feature = "server")]
use agent_pb as pb;
#[cfg(feature = "server")]
use agent_protocol::{node_pb, resources_pb};
#[cfg(feature = "server")]
use futures_util::Stream;
#[cfg(feature = "server")]
use talos_agent::{AttachmentReader, TurnFileScope};
use talos_error::{ErrorCode, TalosError, TalosResult};
#[cfg(feature = "server")]
use talos_llm_provider::{Attachment, AttachmentKind};
#[cfg(feature = "server")]
use talos_service::AgentNodeService;
use talos_store::session::{SessionRecord, SessionStore};
#[cfg(feature = "server")]
use talos_tools::{AgentControl, ToolImageStore};
#[cfg(feature = "server")]
use tonic::{Request, Response, Status};

#[cfg(feature = "server")]
use crate::agent_node_rpc::failure;
#[path = "agent_resource_authorization.rs"]
mod authorization;
#[cfg(feature = "server")]
#[path = "agent_resource_automation.rs"]
mod automation;
#[cfg(feature = "server")]
#[path = "agent_resource_images.rs"]
mod images;
#[cfg(feature = "server")]
#[path = "agent_resource_subagents.rs"]
mod subagents;
#[cfg(feature = "server")]
pub use automation::NodeAutomationControlRpc;
#[cfg(feature = "server")]
pub use images::NodeToolImageRpc;
#[cfg(feature = "server")]
pub use subagents::NodeSubagentControlRpc;

/// Serves the control-plane-owned dependencies used by a remote Kernel. The
/// node lease is checked against the persisted Session before any owner runs.
#[derive(Clone)]
pub struct NodeSessionDataRpc {
    #[cfg(feature = "server")]
    nodes: Option<AgentNodeService>,
    sessions: Arc<dyn SessionStore>,
    #[cfg(feature = "server")]
    attachments: Arc<dyn AttachmentReader>,
}

impl NodeSessionDataRpc {
    pub(crate) fn local(
        sessions: Arc<dyn SessionStore>,
        _attachments: Arc<dyn talos_agent::AttachmentReader>,
    ) -> Self {
        Self {
            #[cfg(feature = "server")]
            nodes: None,
            sessions,
            #[cfg(feature = "server")]
            attachments: _attachments,
        }
    }

    #[cfg(feature = "server")]
    pub fn new(
        nodes: AgentNodeService,
        sessions: Arc<dyn SessionStore>,
        attachments: Arc<dyn AttachmentReader>,
    ) -> Self {
        Self {
            nodes: Some(nodes),
            sessions,
            attachments,
        }
    }
}

#[cfg(feature = "server")]
type AttachmentStream =
    Pin<Box<dyn Stream<Item = Result<pb::ResolveAttachmentsResponse, Status>> + Send>>;

#[cfg(feature = "server")]
#[tonic::async_trait]
impl pb::agent_session_data_rpc_server::AgentSessionDataRpc for NodeSessionDataRpc {
    type ResolveAttachmentsStream = AttachmentStream;

    async fn resolve_attachments(
        &self,
        request: Request<pb::ResolveAttachmentsRequest>,
    ) -> Result<Response<Self::ResolveAttachmentsStream>, Status> {
        const CHUNK_BYTES: usize = 64 * 1024;
        let result = async {
            let request = request.into_inner();
            let params = request.params.ok_or_else(invalid_resource_request)?;
            let auth = node_pb::decode_auth(request.auth)?;
            let owner_user_id = self.authorize_owner(&auth).await?;
            if let Some(caller) = request.caller {
                let scope = caller
                    .session
                    .as_ref()
                    .ok_or_else(invalid_resource_request)?;
                let session = self
                    .authorize(Some(node_pb::encode_auth(auth)), &scope.session_id)
                    .await?;
                if caller.project_id != params.project_id || session.project_id != params.project_id
                {
                    return Err(TalosError::authorization_denied());
                }
            }
            self.attachments
                .read(
                    &TurnFileScope {
                        owner_user_id,
                        project_id: params.project_id,
                    },
                    &params.file_ids,
                )
                .await
        }
        .await;
        let mut responses = Vec::new();
        match result {
            Ok(attachments) => {
                for (order, attachment) in attachments.into_iter().enumerate() {
                    let order = order as u32;
                    responses.push(pb::ResolveAttachmentsResponse {
                        result: Some(pb::resolve_attachments_response::Result::Metadata(
                            pb::ResolvedAttachmentMetadata {
                                order,
                                file_id: attachment.file_id,
                                filename: attachment.filename,
                                media_type: attachment.media_type,
                                size_bytes: attachment.size_bytes,
                            },
                        )),
                    });
                    responses.extend(attachment.bytes.chunks(CHUNK_BYTES).map(|chunk| {
                        pb::ResolveAttachmentsResponse {
                            result: Some(pb::resolve_attachments_response::Result::Chunk(
                                pb::ResolvedAttachmentChunk {
                                    order,
                                    data: chunk.to_vec(),
                                },
                            )),
                        }
                    }));
                }
                responses.push(pb::ResolveAttachmentsResponse {
                    result: Some(pb::resolve_attachments_response::Result::Completed(
                        pb::Empty {},
                    )),
                });
            }
            Err(error) => responses.push(pb::ResolveAttachmentsResponse {
                result: Some(pb::resolve_attachments_response::Result::Failure(failure(
                    error,
                ))),
            }),
        }
        Ok(Response::new(Box::pin(futures_util::stream::iter(
            responses.into_iter().map(Ok),
        ))))
    }
}

#[cfg(feature = "server")]
fn invalid_resource_request() -> TalosError {
    TalosError::new(
        ErrorCode::InvalidRequest,
        "Agent Session data RPC request is invalid",
    )
}

#[cfg(feature = "server")]
fn resource_status() -> Status {
    agent_protocol::error_pb::encode_status(&TalosError::new(
        ErrorCode::InvalidRequest,
        "Resource RPC carrier is invalid",
    ))
}

#[cfg(test)]
#[path = "agent_resource_tests.rs"]
pub(crate) mod tests;
