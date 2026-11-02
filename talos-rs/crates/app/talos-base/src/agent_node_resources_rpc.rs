//! Authorized control-plane resources consumed through existing Worker ports.

use std::pin::Pin;
use std::sync::Arc;

use agent_pb as pb;
use agent_protocol::{node_pb, resources_pb};
use futures_util::Stream;
use talos_agent::{
    AttachmentReader, SessionHistoryReadParams, SessionHistoryReader, SessionHistoryWindow,
    TurnFileScope,
};
use talos_agent_provider_kernel::KernelMemoryProgressStore;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_llm_provider::{Attachment, AttachmentKind};
use talos_service::AgentNodeService;
use talos_store::session::{SessionRecord, SessionStore};
use talos_tools::{AgentControl, ToolImageStore};
use tonic::{Request, Response, Status};

use super::{failure, ResourceRoutes};
#[path = "agent_resource_authorization.rs"]
mod authorization;
#[path = "agent_resource_history_append.rs"]
mod history_append;
use authorization::ResourceAuthority;
#[path = "agent_resource_images.rs"]
mod images;
#[path = "agent_resource_subagents.rs"]
mod subagents;
pub use images::NodeToolImageRpc;
pub use subagents::NodeSubagentControlRpc;

/// Serves the control-plane-owned dependencies used by a remote Kernel. The
/// node lease is checked against the persisted Session before any owner runs.
#[derive(Clone)]
pub struct NodeSessionDataRpc {
    nodes: AgentNodeService,
    sessions: Arc<dyn SessionStore>,
    history: Arc<dyn SessionHistoryReader>,
    attachments: Arc<dyn AttachmentReader>,
    memory_progress: Arc<dyn KernelMemoryProgressStore>,
    authority: ResourceAuthority,
    routes: Option<ResourceRoutes>,
}

impl NodeSessionDataRpc {
    pub fn new(
        nodes: AgentNodeService,
        sessions: Arc<dyn SessionStore>,
        history: Arc<dyn SessionHistoryReader>,
        attachments: Arc<dyn AttachmentReader>,
        memory_progress: Arc<dyn KernelMemoryProgressStore>,
    ) -> Self {
        Self {
            nodes,
            sessions,
            history,
            attachments,
            memory_progress,
            authority: ResourceAuthority::Lease,
            routes: None,
        }
    }
}

type AttachmentStream =
    Pin<Box<dyn Stream<Item = Result<pb::ResolveAttachmentsResponse, Status>> + Send>>;

#[tonic::async_trait]
impl pb::agent_session_data_rpc_server::AgentSessionDataRpc for NodeSessionDataRpc {
    async fn append_session_history_supplement(
        &self,
        request: Request<tonic::Streaming<pb::AppendSessionHistorySupplementRequest>>,
    ) -> Result<Response<pb::AppendSessionHistorySupplementResponse>, Status> {
        self.append_history_stream(request.into_inner()).await
    }

    async fn read_session_history(
        &self,
        request: Request<pb::ReadSessionHistoryRequest>,
    ) -> Result<Response<pb::ReadSessionHistoryResponse>, Status> {
        if let Some(caller) = request
            .get_ref()
            .caller
            .clone()
            .filter(|caller| !caller.connection_id.is_empty())
        {
            if self.should_route() {
                let routes = self.routes.as_ref().ok_or_else(resource_status)?;
                let auth = request.get_ref().auth.clone();
                let response = routes
                    .forward(
                        &self.nodes,
                        auth,
                        caller,
                        Box::pin(tokio_stream::once(Ok(
                            pb::worker_rpc_call::Request::ReadSessionHistory(Box::new(
                                request.into_inner(),
                            )),
                        ))),
                    )
                    .await?;
                let mut response = response;
                use futures_util::StreamExt;
                let value = response
                    .next()
                    .await
                    .transpose()?
                    .ok_or_else(resource_status)?;
                let pb::worker_rpc_result::Response::ReadSessionHistory(value) = value else {
                    return Err(resource_status());
                };
                return Ok(Response::new(*value));
            }
        }
        let result = async {
            let request = request.into_inner();
            let params = request.params.ok_or_else(invalid_resource_request)?;
            self.authorize(request.auth, &params.session_id, request.caller.as_ref())
                .await?;
            let window = params.completed_event_seq.map_or(
                SessionHistoryWindow::Latest,
                |completed_event_seq| SessionHistoryWindow::OldestAfter {
                    completed_event_seq,
                },
            );
            let turns = self
                .history
                .read(SessionHistoryReadParams {
                    session_id: params.session_id,
                    window,
                    limit: params.limit as usize,
                })
                .await?;
            turns
                .into_iter()
                .map(resources_pb::encode_history_turn)
                .collect::<TalosResult<Vec<_>>>()
        }
        .await;
        Ok(Response::new(pb::ReadSessionHistoryResponse {
            result: Some(match result {
                Ok(data) => {
                    pb::read_session_history_response::Result::Turns(pb::SessionHistoryTurns {
                        data,
                    })
                }
                Err(error) => pb::read_session_history_response::Result::Failure(failure(error)),
            }),
        }))
    }

    type ResolveAttachmentsStream = AttachmentStream;

    async fn resolve_attachments(
        &self,
        request: Request<pb::ResolveAttachmentsRequest>,
    ) -> Result<Response<Self::ResolveAttachmentsStream>, Status> {
        if let Some(caller) = request
            .get_ref()
            .caller
            .clone()
            .filter(|caller| !caller.connection_id.is_empty())
        {
            if self.should_route() {
                let routes = self.routes.as_ref().ok_or_else(resource_status)?;
                let auth = request.get_ref().auth.clone();
                let response = routes
                    .forward(
                        &self.nodes,
                        auth,
                        caller,
                        Box::pin(tokio_stream::once(Ok(
                            pb::worker_rpc_call::Request::ResolveAttachments(Box::new(
                                request.into_inner(),
                            )),
                        ))),
                    )
                    .await?;
                let response = futures_util::StreamExt::map(response, |value| match value? {
                    pb::worker_rpc_result::Response::ResolveAttachments(value) => Ok(*value),
                    _ => Err(resource_status()),
                });
                return Ok(Response::new(Box::pin(response)));
            }
        }
        const CHUNK_BYTES: usize = 64 * 1024;
        let result = async {
            let request = request.into_inner();
            let params = request.params.ok_or_else(invalid_resource_request)?;
            let auth = node_pb::decode_auth(request.auth)?;
            let owner_user_id = self.authorize_owner(&auth, request.caller.as_ref()).await?;
            if let ResourceAuthority::Invocation { caller, .. } = &self.authority {
                if caller.project_id != params.project_id {
                    return Err(TalosError::new(
                        ErrorCode::PermissionDenied,
                        "Attachment request belongs to another invocation",
                    ));
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

    async fn read_memory_progress(
        &self,
        request: Request<pb::ReadMemoryProgressRequest>,
    ) -> Result<Response<pb::ReadMemoryProgressResponse>, Status> {
        if let Some(caller) = request
            .get_ref()
            .caller
            .clone()
            .filter(|caller| !caller.connection_id.is_empty())
        {
            if self.should_route() {
                let routes = self.routes.as_ref().ok_or_else(resource_status)?;
                let auth = request.get_ref().auth.clone();
                let response = routes
                    .forward(
                        &self.nodes,
                        auth,
                        caller,
                        Box::pin(tokio_stream::once(Ok(
                            pb::worker_rpc_call::Request::ReadMemoryProgress(Box::new(
                                request.into_inner(),
                            )),
                        ))),
                    )
                    .await?;
                let mut response = response;
                use futures_util::StreamExt;
                let value = response
                    .next()
                    .await
                    .transpose()?
                    .ok_or_else(resource_status)?;
                let pb::worker_rpc_result::Response::ReadMemoryProgress(value) = value else {
                    return Err(resource_status());
                };
                return Ok(Response::new(*value));
            }
        }
        let result = async {
            let request = request.into_inner();
            let params = request.params.ok_or_else(invalid_resource_request)?;
            self.authorize(request.auth, &params.session_id, request.caller.as_ref())
                .await?;
            self.memory_progress
                .processed_until_event_seq(&params.session_id)
                .await
        }
        .await;
        Ok(Response::new(pb::ReadMemoryProgressResponse {
            result: Some(match result {
                Ok(processed_until_event_seq) => {
                    pb::read_memory_progress_response::Result::Progress(pb::MemoryProgress {
                        processed_until_event_seq,
                    })
                }
                Err(error) => pb::read_memory_progress_response::Result::Failure(failure(error)),
            }),
        }))
    }

    async fn advance_memory_progress(
        &self,
        request: Request<pb::AdvanceMemoryProgressRequest>,
    ) -> Result<Response<pb::AdvanceMemoryProgressResponse>, Status> {
        if let Some(caller) = request
            .get_ref()
            .caller
            .clone()
            .filter(|caller| !caller.connection_id.is_empty())
        {
            if self.should_route() {
                let routes = self.routes.as_ref().ok_or_else(resource_status)?;
                let auth = request.get_ref().auth.clone();
                let response = routes
                    .forward(
                        &self.nodes,
                        auth,
                        caller,
                        Box::pin(tokio_stream::once(Ok(
                            pb::worker_rpc_call::Request::AdvanceMemoryProgress(Box::new(
                                request.into_inner(),
                            )),
                        ))),
                    )
                    .await?;
                let mut response = response;
                use futures_util::StreamExt;
                let value = response
                    .next()
                    .await
                    .transpose()?
                    .ok_or_else(resource_status)?;
                let pb::worker_rpc_result::Response::AdvanceMemoryProgress(value) = value else {
                    return Err(resource_status());
                };
                return Ok(Response::new(*value));
            }
        }
        let result = async {
            let request = request.into_inner();
            let params = request.params.ok_or_else(invalid_resource_request)?;
            self.authorize(request.auth, &params.session_id, request.caller.as_ref())
                .await?;
            self.memory_progress
                .advance_processed_until_event_seq(
                    &params.session_id,
                    params.expected_until_seq,
                    params.next_until_seq,
                )
                .await
        }
        .await;
        Ok(Response::new(pb::AdvanceMemoryProgressResponse {
            result: Some(match result {
                Ok(()) => pb::advance_memory_progress_response::Result::Advanced(pb::Empty {}),
                Err(error) => pb::advance_memory_progress_response::Result::Failure(failure(error)),
            }),
        }))
    }
}

fn invalid_resource_request() -> TalosError {
    TalosError::new(
        ErrorCode::InvalidRequest,
        "Agent Session data RPC request is invalid",
    )
}

fn resource_status() -> Status {
    agent_protocol::error_pb::encode_status(&TalosError::new(
        ErrorCode::InvalidRequest,
        "Resource RPC carrier is invalid",
    ))
}
