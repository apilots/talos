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

use super::failure;

/// Serves the control-plane-owned dependencies used by a remote Kernel. The
/// node lease is checked against the persisted Session before any owner runs.
#[derive(Clone)]
pub struct NodeSessionDataRpc {
    nodes: AgentNodeService,
    sessions: Arc<dyn SessionStore>,
    history: Arc<dyn SessionHistoryReader>,
    attachments: Arc<dyn AttachmentReader>,
    memory_progress: Arc<dyn KernelMemoryProgressStore>,
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
        }
    }

    async fn authorize(
        &self,
        auth: Option<pb::NodeAuth>,
        session_id: &str,
    ) -> TalosResult<SessionRecord> {
        let auth = node_pb::decode_auth(auth)?;
        // Reject invalid leases before looking up caller-selected resources.
        self.nodes.authenticate_lease(&auth).await?;
        let session = self
            .sessions
            .get_session_record(session_id)
            .await?
            .ok_or_else(|| TalosError::new(ErrorCode::SessionNotFound, "Session was not found"))?;
        self.nodes.authorize_session_lease(&auth, &session).await?;
        Ok(session)
    }
}

type AttachmentStream =
    Pin<Box<dyn Stream<Item = Result<pb::ResolveAttachmentsResponse, Status>> + Send>>;

#[tonic::async_trait]
impl pb::agent_session_data_rpc_server::AgentSessionDataRpc for NodeSessionDataRpc {
    async fn read_session_history(
        &self,
        request: Request<pb::ReadSessionHistoryRequest>,
    ) -> Result<Response<pb::ReadSessionHistoryResponse>, Status> {
        let result = async {
            let request = request.into_inner();
            let params = request.params.ok_or_else(invalid_resource_request)?;
            self.authorize(request.auth, &params.session_id).await?;
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
        const CHUNK_BYTES: usize = 64 * 1024;
        let result = async {
            let request = request.into_inner();
            let params = request.params.ok_or_else(invalid_resource_request)?;
            let auth = node_pb::decode_auth(request.auth)?;
            let owner_user_id = self.nodes.authorize_lease_owner(&auth).await?;
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
        let result = async {
            let request = request.into_inner();
            let params = request.params.ok_or_else(invalid_resource_request)?;
            self.authorize(request.auth, &params.session_id).await?;
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
        let result = async {
            let request = request.into_inner();
            let params = request.params.ok_or_else(invalid_resource_request)?;
            self.authorize(request.auth, &params.session_id).await?;
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

#[derive(Clone)]
pub struct NodeToolImageRpc {
    nodes: AgentNodeService,
    images: Arc<dyn ToolImageStore>,
    max_upload_bytes: u64,
}

impl NodeToolImageRpc {
    pub fn new(
        nodes: AgentNodeService,
        images: Arc<dyn ToolImageStore>,
        max_upload_bytes: u64,
    ) -> Self {
        Self {
            nodes,
            images,
            max_upload_bytes,
        }
    }
}

#[tonic::async_trait]
impl pb::agent_tool_image_rpc_server::AgentToolImageRpc for NodeToolImageRpc {
    async fn save_tool_image(
        &self,
        request: Request<tonic::Streaming<pb::SaveToolImageRequest>>,
    ) -> Result<Response<pb::SaveToolImageResponse>, Status> {
        let result = async {
            let mut stream = request.into_inner();
            let first = stream
                .message()
                .await
                .map_err(|error| {
                    TalosError::from_source(
                        ErrorCode::InvalidRequest,
                        "Tool image upload stream is invalid",
                        error,
                    )
                })?
                .ok_or_else(invalid_resource_request)?;
            let auth = node_pb::decode_auth(first.auth)?;
            let owner = self.nodes.authorize_lease_owner(&auth).await?;
            let metadata = match first.body.ok_or_else(invalid_resource_request)? {
                pb::save_tool_image_request::Body::Metadata(value) => value,
                pb::save_tool_image_request::Body::Chunk(_) => {
                    return Err(invalid_resource_request())
                }
            };
            if metadata.size_bytes > self.max_upload_bytes {
                return Err(TalosError::new(
                    ErrorCode::UploadTooLarge,
                    "Tool image exceeds the upload byte limit",
                ));
            }
            if metadata.filename.is_empty() || !metadata.media_type.starts_with("image/") {
                return Err(TalosError::new(
                    ErrorCode::InvalidRequest,
                    "Tool image metadata is invalid",
                ));
            }
            let capacity =
                usize::try_from(metadata.size_bytes).map_err(|_| invalid_resource_request())?;
            let mut bytes = Vec::with_capacity(capacity);
            while let Some(frame) = stream.message().await.map_err(|error| {
                TalosError::from_source(
                    ErrorCode::InvalidRequest,
                    "Tool image upload stream is invalid",
                    error,
                )
            })? {
                let frame_auth = node_pb::decode_auth(frame.auth)?;
                if frame_auth.node_id != auth.node_id || frame_auth.token != auth.token {
                    return Err(TalosError::new(
                        ErrorCode::Unauthenticated,
                        "Agent node authentication changed during upload",
                    ));
                }
                let chunk = match frame.body.ok_or_else(invalid_resource_request)? {
                    pb::save_tool_image_request::Body::Chunk(value) => value,
                    pb::save_tool_image_request::Body::Metadata(_) => {
                        return Err(invalid_resource_request())
                    }
                };
                let next_len = bytes
                    .len()
                    .checked_add(chunk.len())
                    .ok_or_else(invalid_resource_request)?;
                if next_len as u64 > metadata.size_bytes {
                    return Err(TalosError::new(
                        ErrorCode::UploadTooLarge,
                        "Tool image exceeds its declared byte length",
                    ));
                }
                bytes.extend_from_slice(&chunk);
            }
            if bytes.len() as u64 != metadata.size_bytes {
                return Err(TalosError::new(
                    ErrorCode::InvalidRequest,
                    "Tool image byte length does not match metadata",
                ));
            }
            let mut files = self
                .images
                .save(
                    &owner,
                    vec![Attachment::new(
                        AttachmentKind::Image,
                        metadata.filename,
                        metadata.media_type,
                        bytes.into(),
                    )],
                )
                .await?;
            if files.len() != 1 {
                return Err(TalosError::internal(
                    "Tool image Store returned an invalid result count",
                ));
            }
            resources_pb::encode_tool_file(files.remove(0))
        }
        .await;
        Ok(Response::new(pb::SaveToolImageResponse {
            result: Some(match result {
                Ok(value) => pb::save_tool_image_response::Result::File(value),
                Err(error) => pb::save_tool_image_response::Result::Failure(failure(error)),
            }),
        }))
    }
}

#[derive(Clone)]
pub struct NodeSubagentControlRpc {
    session_data: NodeSessionDataRpc,
    control: Arc<dyn AgentControl>,
}

impl NodeSubagentControlRpc {
    pub fn new(session_data: NodeSessionDataRpc, control: Arc<dyn AgentControl>) -> Self {
        Self {
            session_data,
            control,
        }
    }

    async fn authorize(&self, auth: Option<pb::NodeAuth>, session_id: &str) -> TalosResult<()> {
        self.session_data
            .authorize(auth, session_id)
            .await
            .map(drop)
    }
}

#[tonic::async_trait]
impl pb::agent_subagent_control_rpc_server::AgentSubagentControlRpc for NodeSubagentControlRpc {
    async fn spawn_subagent(
        &self,
        request: Request<pb::SpawnSubagentRequest>,
    ) -> Result<Response<pb::SpawnSubagentResponse>, Status> {
        let result = async {
            let request = request.into_inner();
            let params = resources_pb::decode_spawn_params(request.params)?;
            self.authorize(request.auth, &params.caller.session_id)
                .await?;
            self.control.spawn(params).await
        }
        .await;
        Ok(Response::new(pb::SpawnSubagentResponse {
            result: Some(match result {
                Ok(value) => pb::spawn_subagent_response::Result::Spawned(
                    resources_pb::encode_spawn_result(value),
                ),
                Err(error) => pb::spawn_subagent_response::Result::Failure(failure(error)),
            }),
        }))
    }

    async fn send_subagent_message(
        &self,
        request: Request<pb::SendSubagentMessageRequest>,
    ) -> Result<Response<pb::SendSubagentMessageResponse>, Status> {
        let result = async {
            let request = request.into_inner();
            let params = resources_pb::decode_send_params(request.params)?;
            self.authorize(request.auth, &params.caller.session_id)
                .await?;
            self.control.send_message(params).await
        }
        .await;
        Ok(Response::new(pb::SendSubagentMessageResponse {
            result: Some(match result {
                Ok(value) => pb::send_subagent_message_response::Result::Sent(
                    resources_pb::encode_send_result(value),
                ),
                Err(error) => pb::send_subagent_message_response::Result::Failure(failure(error)),
            }),
        }))
    }

    async fn wait_subagent(
        &self,
        request: Request<pb::WaitSubagentRequest>,
    ) -> Result<Response<pb::SubagentSnapshotResponse>, Status> {
        let result = async {
            let request = request.into_inner();
            let params = resources_pb::decode_wait_params(request.params)?;
            self.authorize(request.auth, &params.caller.session_id)
                .await?;
            self.control.wait(params).await
        }
        .await;
        Ok(Response::new(snapshot_response(result)))
    }

    async fn interrupt_subagent(
        &self,
        request: Request<pb::InterruptSubagentRequest>,
    ) -> Result<Response<pb::SubagentSnapshotResponse>, Status> {
        let result = async {
            let request = request.into_inner();
            let params = resources_pb::decode_interrupt_params(request.params)?;
            self.authorize(request.auth, &params.caller.session_id)
                .await?;
            self.control.interrupt(params).await
        }
        .await;
        Ok(Response::new(snapshot_response(result)))
    }

    async fn follow_up_subagent(
        &self,
        request: Request<pb::FollowUpSubagentRequest>,
    ) -> Result<Response<pb::SubagentSnapshotResponse>, Status> {
        let result = async {
            let request = request.into_inner();
            let params = resources_pb::decode_follow_up_params(request.params)?;
            self.authorize(request.auth, &params.caller.session_id)
                .await?;
            self.control.follow_up(params).await
        }
        .await;
        Ok(Response::new(snapshot_response(result)))
    }

    async fn list_subagents(
        &self,
        request: Request<pb::ListSubagentsRequest>,
    ) -> Result<Response<pb::ListSubagentsResponse>, Status> {
        let result = async {
            let request = request.into_inner();
            let params = resources_pb::decode_list_params(request.params)?;
            self.authorize(request.auth, &params.caller.session_id)
                .await?;
            self.control.list(params).await
        }
        .await;
        Ok(Response::new(pb::ListSubagentsResponse {
            result: Some(match result {
                Ok(value) => {
                    pb::list_subagents_response::Result::Tree(resources_pb::encode_tree(value))
                }
                Err(error) => pb::list_subagents_response::Result::Failure(failure(error)),
            }),
        }))
    }
}

fn snapshot_response(
    result: TalosResult<talos_types::SubagentSnapshot>,
) -> pb::SubagentSnapshotResponse {
    pb::SubagentSnapshotResponse {
        result: Some(match result {
            Ok(value) => pb::subagent_snapshot_response::Result::Snapshot(
                resources_pb::encode_snapshot(value),
            ),
            Err(error) => pb::subagent_snapshot_response::Result::Failure(failure(error)),
        }),
    }
}
