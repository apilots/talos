use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use agent_pb as pb;
use agent_protocol::{error_pb, harness_pb, node_pb, resources_pb, NodeAuth};
use bytes::Bytes;
use talos_agent::{
    AgentFuture, AgentResult, AttachmentReader, ResolvedAttachment, SessionHistoryReadParams,
    SessionHistoryReader, SessionHistoryWindow, TurnFileScope,
};
use talos_agent_api::SessionHistoryTurn;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_llm_provider::{Attachment, AttachmentKind};
use talos_tools::{AgentControl, AgentControlFuture, ToolImageStore};
use talos_types::{
    AgentCaller, AgentTarget, AgentTreeSnapshot, CollaborationToolResult, FileInfo,
    FollowUpSubagentRequest, InterruptSubagentRequest, ListSubagentsRequest, SendMessageResult,
    SendSubagentMessageRequest, SpawnSubagentRequest, SubagentSnapshot, SubagentSpawnResult,
    WaitSubagentRequest,
};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::transport::{Channel, Endpoint};

use crate::AgentTlsTrust;

/// Remote implementation of the existing data ports consumed by Kernel.
#[derive(Clone)]
pub struct AgentSessionDataClient {
    channel: Channel,
    auth: Arc<RwLock<NodeAuth>>,
    caller: Option<pb::CoordinatorCallContext>,
}

impl AgentSessionDataClient {
    pub async fn connect(
        endpoint: &str,
        trust: AgentTlsTrust,
        auth: NodeAuth,
    ) -> TalosResult<Self> {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
        if !endpoint.starts_with("https://") || auth.node_id.is_empty() || auth.token.is_empty() {
            return Err(invalid_response());
        }
        let endpoint = Endpoint::from_shared(endpoint.to_owned())
            .map_err(|error| {
                TalosError::from_source(
                    ErrorCode::InvalidRequest,
                    "Agent Session data endpoint is invalid",
                    error,
                )
            })?
            .connect_timeout(Duration::from_secs(10))
            .tls_config(trust.into_config())
            .map_err(|error| {
                TalosError::from_source(
                    ErrorCode::InvalidRequest,
                    "Agent Session data TLS trust is invalid",
                    error,
                )
            })?;
        Ok(Self {
            channel: endpoint.connect_lazy(),
            auth: Arc::new(RwLock::new(auth)),
            caller: None,
        })
    }

    pub async fn connect_loopback(
        address: std::net::SocketAddr,
        auth: NodeAuth,
    ) -> TalosResult<Self> {
        Ok(Self {
            channel: crate::tls::loopback_channel(address).await?,
            auth: Arc::new(RwLock::new(auth)),
            caller: None,
        })
    }
    /// Read the current private transport credential for invocation binding.
    /// It must not be logged, exported to callers or synchronized as metadata.
    pub fn current_auth(&self) -> NodeAuth {
        self.auth()
    }
    /// Freeze authorization and routing for this invocation. Concurrent Sessions
    /// never mutate a shared endpoint, callback scope or another invocation's auth.
    pub fn for_invocation(
        &self,
        auth: NodeAuth,
        session: &agent_protocol::AgentSessionContext,
        caller: Option<pb::CoordinatorCallContext>,
    ) -> TalosResult<Self> {
        let scope = agent_protocol::harness_pb::encode_session_context(session.clone());
        let caller = match caller {
            Some(caller)
                if caller.session.as_ref() == Some(&scope)
                    && !caller.connection_id.is_empty()
                    && caller.rpc_call_id != 0 =>
            {
                caller
            }
            Some(_) => {
                return Err(TalosError::new(
                    ErrorCode::PermissionDenied,
                    "Resource callback scope does not match Session",
                ))
            }
            None => pb::CoordinatorCallContext {
                coordinator_id: String::new(),
                connection_id: String::new(),
                rpc_call_id: 0,
                session: Some(scope),
                turn_id: None,
                project_id: None,
            },
        };
        Ok(Self {
            channel: self.channel.clone(),
            auth: Arc::new(RwLock::new(auth)),
            caller: Some(caller),
        })
    }

    pub fn update_auth(&self, auth: NodeAuth) -> TalosResult<()> {
        if auth.node_id.is_empty() || auth.token.is_empty() || auth.node_id != self.auth().node_id {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Agent Session data lease does not match node",
            ));
        }
        *self
            .auth
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = auth;
        Ok(())
    }

    fn auth(&self) -> NodeAuth {
        self.auth
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    async fn read_history(
        &self,
        params: SessionHistoryReadParams,
    ) -> TalosResult<Vec<SessionHistoryTurn>> {
        let completed_event_seq = match params.window {
            SessionHistoryWindow::Latest => None,
            SessionHistoryWindow::OldestAfter {
                completed_event_seq,
            } => Some(completed_event_seq),
        };
        let limit = u32::try_from(params.limit).map_err(|_| {
            TalosError::new(
                ErrorCode::InvalidRequest,
                "Session history limit is too large",
            )
        })?;
        let response =
            pb::agent_session_data_rpc_client::AgentSessionDataRpcClient::new(self.channel.clone())
                .read_session_history(rpc_request(
                    pb::ReadSessionHistoryRequest {
                        caller: self.caller.clone(),
                        auth: Some(node_pb::encode_auth(self.auth())),
                        params: Some(pb::ReadSessionHistoryParams {
                            session_id: params.session_id,
                            completed_event_seq,
                            limit,
                        }),
                    },
                    Duration::from_secs(10),
                ))
                .await
                .map_err(crate::decode_rpc_status)?
                .into_inner();
        match response.result.ok_or_else(invalid_response)? {
            pb::read_session_history_response::Result::Turns(turns) => turns
                .data
                .into_iter()
                .map(resources_pb::decode_history_turn)
                .collect(),
            pb::read_session_history_response::Result::Failure(failure) => {
                Err(error_pb::decode_failure(failure)?)
            }
        }
    }

    async fn resolve_attachments(
        &self,
        scope: &TurnFileScope,
        file_ids: &[String],
    ) -> TalosResult<Vec<ResolvedAttachment>> {
        let mut stream =
            pb::agent_session_data_rpc_client::AgentSessionDataRpcClient::new(self.channel.clone())
                .resolve_attachments(rpc_request(
                    pb::ResolveAttachmentsRequest {
                        caller: self.caller.clone(),
                        auth: Some(node_pb::encode_auth(self.auth())),
                        params: Some(pb::ResolveAttachmentsParams {
                            project_id: scope.project_id.clone(),
                            file_ids: file_ids.to_vec(),
                        }),
                    },
                    Duration::from_secs(10),
                ))
                .await
                .map_err(crate::decode_rpc_status)?
                .into_inner();
        let mut resolved = Vec::new();
        let mut current: Option<(pb::ResolvedAttachmentMetadata, Vec<u8>)> = None;
        while let Some(response) = stream.message().await.map_err(crate::decode_rpc_status)? {
            match response.result.ok_or_else(invalid_response)? {
                pb::resolve_attachments_response::Result::Metadata(metadata) => {
                    finish_attachment(&mut resolved, current.take())?;
                    if metadata.order as usize != resolved.len()
                        || file_ids.get(resolved.len()) != Some(&metadata.file_id)
                    {
                        return Err(invalid_response());
                    }
                    current = Some((metadata, Vec::new()));
                }
                pb::resolve_attachments_response::Result::Chunk(chunk) => {
                    let (metadata, bytes) = current.as_mut().ok_or_else(invalid_response)?;
                    if metadata.order != chunk.order
                        || chunk.data.len() > 64 * 1024
                        || bytes.len().saturating_add(chunk.data.len()) as u64 > metadata.size_bytes
                    {
                        return Err(invalid_response());
                    }
                    bytes.extend_from_slice(&chunk.data);
                }
                pb::resolve_attachments_response::Result::Completed(_) => {
                    finish_attachment(&mut resolved, current.take())?;
                    if resolved.len() != file_ids.len() {
                        return Err(invalid_response());
                    }
                    return Ok(resolved);
                }
                pb::resolve_attachments_response::Result::Failure(failure) => {
                    return Err(error_pb::decode_failure(failure)?);
                }
            }
        }
        Err(invalid_response())
    }

    pub async fn processed_until_event_seq(&self, session_id: &str) -> TalosResult<u64> {
        let response =
            pb::agent_session_data_rpc_client::AgentSessionDataRpcClient::new(self.channel.clone())
                .read_memory_progress(rpc_request(
                    pb::ReadMemoryProgressRequest {
                        caller: self.caller.clone(),
                        auth: Some(node_pb::encode_auth(self.auth())),
                        params: Some(pb::ReadMemoryProgressParams {
                            session_id: session_id.to_owned(),
                        }),
                    },
                    Duration::from_secs(10),
                ))
                .await
                .map_err(crate::decode_rpc_status)?
                .into_inner();
        match response.result.ok_or_else(invalid_response)? {
            pb::read_memory_progress_response::Result::Progress(progress) => {
                Ok(progress.processed_until_event_seq)
            }
            pb::read_memory_progress_response::Result::Failure(failure) => {
                Err(error_pb::decode_failure(failure)?)
            }
        }
    }

    pub async fn advance_processed_until_event_seq(
        &self,
        session_id: &str,
        expected_until_seq: u64,
        next_until_seq: u64,
    ) -> TalosResult<()> {
        let response =
            pb::agent_session_data_rpc_client::AgentSessionDataRpcClient::new(self.channel.clone())
                .advance_memory_progress(rpc_request(
                    pb::AdvanceMemoryProgressRequest {
                        caller: self.caller.clone(),
                        auth: Some(node_pb::encode_auth(self.auth())),
                        params: Some(pb::AdvanceMemoryProgressParams {
                            session_id: session_id.to_owned(),
                            expected_until_seq,
                            next_until_seq,
                        }),
                    },
                    Duration::from_secs(10),
                ))
                .await
                .map_err(crate::decode_rpc_status)?
                .into_inner();
        match response.result.ok_or_else(invalid_response)? {
            pb::advance_memory_progress_response::Result::Advanced(_) => Ok(()),
            pb::advance_memory_progress_response::Result::Failure(failure) => {
                Err(error_pb::decode_failure(failure)?)
            }
        }
    }

    async fn spawn_subagent(
        &self,
        request: SpawnSubagentRequest,
    ) -> TalosResult<SubagentSpawnResult> {
        let response = pb::agent_subagent_control_rpc_client::AgentSubagentControlRpcClient::new(
            self.channel.clone(),
        )
        .spawn_subagent(rpc_request(
            pb::SpawnSubagentRequest {
                caller: self.caller.clone(),
                auth: Some(node_pb::encode_auth(self.auth())),
                params: Some(pb::SpawnSubagentParams {
                    caller: Some(encode_caller(request.caller)),
                    task_name: request.task_name,
                    prompt: request.prompt,
                }),
            },
            Duration::from_secs(10),
        ))
        .await
        .map_err(crate::decode_rpc_status)?
        .into_inner();
        match response.result.ok_or_else(invalid_response)? {
            pb::spawn_subagent_response::Result::Spawned(value) => {
                match decode_collaboration(pb::collaboration_result::Result::SpawnAgent(value))? {
                    CollaborationToolResult::SpawnAgent(value) => Ok(value),
                    _ => Err(invalid_response()),
                }
            }
            pb::spawn_subagent_response::Result::Failure(value) => {
                Err(error_pb::decode_failure(value)?)
            }
        }
    }

    async fn send_subagent_message(
        &self,
        request: SendSubagentMessageRequest,
    ) -> TalosResult<SendMessageResult> {
        let response = pb::agent_subagent_control_rpc_client::AgentSubagentControlRpcClient::new(
            self.channel.clone(),
        )
        .send_subagent_message(rpc_request(
            pb::SendSubagentMessageRequest {
                caller: self.caller.clone(),
                auth: Some(node_pb::encode_auth(self.auth())),
                params: Some(pb::SendSubagentMessageParams {
                    caller: Some(encode_caller(request.caller)),
                    target: Some(encode_target(request.target)),
                    message: request.message,
                }),
            },
            Duration::from_secs(10),
        ))
        .await
        .map_err(crate::decode_rpc_status)?
        .into_inner();
        match response.result.ok_or_else(invalid_response)? {
            pb::send_subagent_message_response::Result::Sent(value) => {
                match decode_collaboration(pb::collaboration_result::Result::SendMessage(value))? {
                    CollaborationToolResult::SendMessage(value) => Ok(value),
                    _ => Err(invalid_response()),
                }
            }
            pb::send_subagent_message_response::Result::Failure(value) => {
                Err(error_pb::decode_failure(value)?)
            }
        }
    }

    async fn wait_subagent(&self, request: WaitSubagentRequest) -> TalosResult<SubagentSnapshot> {
        let deadline =
            Duration::from_millis(request.timeout_ms).saturating_add(Duration::from_secs(10));
        let response = pb::agent_subagent_control_rpc_client::AgentSubagentControlRpcClient::new(
            self.channel.clone(),
        )
        .wait_subagent(rpc_request(
            pb::WaitSubagentRequest {
                caller: self.caller.clone(),
                auth: Some(node_pb::encode_auth(self.auth())),
                params: Some(pb::WaitSubagentParams {
                    caller: Some(encode_caller(request.caller)),
                    target: Some(encode_target(request.target)),
                    timeout_ms: request.timeout_ms,
                }),
            },
            deadline,
        ))
        .await
        .map_err(crate::decode_rpc_status)?
        .into_inner();
        decode_snapshot_response(response)
    }

    async fn interrupt_subagent(
        &self,
        request: InterruptSubagentRequest,
    ) -> TalosResult<SubagentSnapshot> {
        let response = pb::agent_subagent_control_rpc_client::AgentSubagentControlRpcClient::new(
            self.channel.clone(),
        )
        .interrupt_subagent(rpc_request(
            pb::InterruptSubagentRequest {
                caller: self.caller.clone(),
                auth: Some(node_pb::encode_auth(self.auth())),
                params: Some(pb::InterruptSubagentParams {
                    caller: Some(encode_caller(request.caller)),
                    target: Some(encode_target(request.target)),
                }),
            },
            Duration::from_secs(10),
        ))
        .await
        .map_err(crate::decode_rpc_status)?
        .into_inner();
        decode_snapshot_response(response)
    }

    async fn follow_up_subagent(
        &self,
        request: FollowUpSubagentRequest,
    ) -> TalosResult<SubagentSnapshot> {
        let response = pb::agent_subagent_control_rpc_client::AgentSubagentControlRpcClient::new(
            self.channel.clone(),
        )
        .follow_up_subagent(rpc_request(
            pb::FollowUpSubagentRequest {
                caller: self.caller.clone(),
                auth: Some(node_pb::encode_auth(self.auth())),
                params: Some(pb::FollowUpSubagentParams {
                    caller: Some(encode_caller(request.caller)),
                    target: Some(encode_target(request.target)),
                    prompt: request.prompt,
                }),
            },
            Duration::from_secs(10),
        ))
        .await
        .map_err(crate::decode_rpc_status)?
        .into_inner();
        decode_snapshot_response(response)
    }

    async fn list_subagents(
        &self,
        request: ListSubagentsRequest,
    ) -> TalosResult<AgentTreeSnapshot> {
        let response = pb::agent_subagent_control_rpc_client::AgentSubagentControlRpcClient::new(
            self.channel.clone(),
        )
        .list_subagents(rpc_request(
            pb::ListSubagentsRequest {
                caller: self.caller.clone(),
                auth: Some(node_pb::encode_auth(self.auth())),
                params: Some(pb::ListSubagentsParams {
                    caller: Some(encode_caller(request.caller)),
                }),
            },
            Duration::from_secs(10),
        ))
        .await
        .map_err(crate::decode_rpc_status)?
        .into_inner();
        match response.result.ok_or_else(invalid_response)? {
            pb::list_subagents_response::Result::Tree(value) => {
                match decode_collaboration(pb::collaboration_result::Result::ListAgents(value))? {
                    CollaborationToolResult::ListAgents(value) => Ok(value),
                    _ => Err(invalid_response()),
                }
            }
            pb::list_subagents_response::Result::Failure(value) => {
                Err(error_pb::decode_failure(value)?)
            }
        }
    }

    async fn save_tool_image(&self, image: Attachment) -> TalosResult<FileInfo> {
        const CHUNK_BYTES: usize = 64 * 1024;
        if image.kind != AttachmentKind::Image {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Tool image upload requires an image attachment",
            ));
        }
        let (sender, receiver) = mpsc::channel(4);
        let auth = self.auth();
        let filename = image.filename;
        let media_type = image.media_type;
        let size_bytes = image.bytes.len() as u64;
        let bytes = image.bytes;
        let caller = self.caller.clone();
        let upload = tokio::spawn(async move {
            sender
                .send(pb::SaveToolImageRequest {
                    caller: caller.clone(),
                    auth: Some(node_pb::encode_auth(auth.clone())),
                    body: Some(pb::save_tool_image_request::Body::Metadata(
                        pb::SaveToolImageMetadata {
                            filename,
                            media_type,
                            size_bytes,
                        },
                    )),
                })
                .await
                .map_err(|_| transport_closed())?;
            for chunk in bytes.chunks(CHUNK_BYTES) {
                sender
                    .send(pb::SaveToolImageRequest {
                        caller: caller.clone(),
                        auth: Some(node_pb::encode_auth(auth.clone())),
                        body: Some(pb::save_tool_image_request::Body::Chunk(chunk.to_vec())),
                    })
                    .await
                    .map_err(|_| transport_closed())?;
            }
            Ok::<(), TalosError>(())
        });
        let response =
            pb::agent_tool_image_rpc_client::AgentToolImageRpcClient::new(self.channel.clone())
                .save_tool_image(rpc_request(
                    ReceiverStream::new(receiver),
                    Duration::from_secs(10),
                ))
                .await
                .map_err(crate::decode_rpc_status);
        let upload = upload.await.map_err(|_| transport_closed())?;
        let response = response?.into_inner();
        upload?;
        match response.result.ok_or_else(invalid_response)? {
            pb::save_tool_image_response::Result::File(value) => {
                resources_pb::decode_tool_file(value)
            }
            pb::save_tool_image_response::Result::Failure(value) => {
                Err(error_pb::decode_failure(value)?)
            }
        }
    }
}

impl SessionHistoryReader for AgentSessionDataClient {
    fn read(
        &self,
        params: SessionHistoryReadParams,
    ) -> AgentFuture<'_, AgentResult<Vec<SessionHistoryTurn>>> {
        Box::pin(self.read_history(params))
    }
}

impl AttachmentReader for AgentSessionDataClient {
    fn read<'a>(
        &'a self,
        scope: &'a TurnFileScope,
        file_ids: &'a [String],
    ) -> Pin<Box<dyn Future<Output = TalosResult<Vec<ResolvedAttachment>>> + Send + 'a>> {
        Box::pin(self.resolve_attachments(scope, file_ids))
    }
}

impl AgentControl for AgentSessionDataClient {
    fn spawn(&self, request: SpawnSubagentRequest) -> AgentControlFuture<'_, SubagentSpawnResult> {
        Box::pin(self.spawn_subagent(request))
    }

    fn send_message(
        &self,
        request: SendSubagentMessageRequest,
    ) -> AgentControlFuture<'_, SendMessageResult> {
        Box::pin(self.send_subagent_message(request))
    }

    fn wait(&self, request: WaitSubagentRequest) -> AgentControlFuture<'_, SubagentSnapshot> {
        Box::pin(self.wait_subagent(request))
    }

    fn interrupt(
        &self,
        request: InterruptSubagentRequest,
    ) -> AgentControlFuture<'_, SubagentSnapshot> {
        Box::pin(self.interrupt_subagent(request))
    }

    fn follow_up(
        &self,
        request: FollowUpSubagentRequest,
    ) -> AgentControlFuture<'_, SubagentSnapshot> {
        Box::pin(self.follow_up_subagent(request))
    }

    fn list(&self, request: ListSubagentsRequest) -> AgentControlFuture<'_, AgentTreeSnapshot> {
        Box::pin(self.list_subagents(request))
    }
}

impl ToolImageStore for AgentSessionDataClient {
    fn save<'a>(
        &'a self,
        _user_id: &'a str,
        images: Vec<Attachment>,
    ) -> futures_util::future::BoxFuture<'a, TalosResult<Vec<FileInfo>>> {
        Box::pin(async move {
            let mut files = Vec::with_capacity(images.len());
            for image in images {
                files.push(self.save_tool_image(image).await?);
            }
            Ok(files)
        })
    }

    fn read<'a>(
        &'a self,
        _user_id: &'a str,
        file_ids: &'a [String],
    ) -> futures_util::future::BoxFuture<'a, TalosResult<Vec<Attachment>>> {
        Box::pin(async move {
            self.resolve_attachments(
                &TurnFileScope {
                    owner_user_id: String::new(),
                    project_id: None,
                },
                file_ids,
            )
            .await?
            .into_iter()
            .map(|file| {
                if !file.media_type.starts_with("image/") {
                    return Err(TalosError::new(
                        ErrorCode::UnsupportedMediaType,
                        "The tool file is not an image.",
                    ));
                }
                Ok(Attachment::new(
                    AttachmentKind::Image,
                    file.filename,
                    file.media_type,
                    file.bytes,
                ))
            })
            .collect()
        })
    }
}

fn encode_caller(value: AgentCaller) -> pb::AgentCaller {
    pb::AgentCaller {
        session_id: value.session_id,
        turn_id: value.turn_id,
        agent_id: value.agent_id,
    }
}

fn encode_target(value: AgentTarget) -> pb::AgentTarget {
    let target = match value {
        AgentTarget::DirectChild { name } => pb::agent_target::Target::DirectChildName(name),
        AgentTarget::Descendant { path } => {
            pb::agent_target::Target::DescendantPath(path.as_str().to_owned())
        }
    };
    pb::AgentTarget {
        target: Some(target),
    }
}

fn decode_collaboration(
    result: pb::collaboration_result::Result,
) -> TalosResult<CollaborationToolResult> {
    harness_pb::decode_collaboration(pb::CollaborationResult {
        result: Some(result),
    })
}

fn decode_snapshot_response(
    response: pb::SubagentSnapshotResponse,
) -> TalosResult<SubagentSnapshot> {
    match response.result.ok_or_else(invalid_response)? {
        pb::subagent_snapshot_response::Result::Snapshot(value) => {
            match decode_collaboration(pb::collaboration_result::Result::WaitAgent(value))? {
                CollaborationToolResult::WaitAgent(value) => Ok(value),
                _ => Err(invalid_response()),
            }
        }
        pb::subagent_snapshot_response::Result::Failure(value) => {
            Err(error_pb::decode_failure(value)?)
        }
    }
}

fn finish_attachment(
    resolved: &mut Vec<ResolvedAttachment>,
    current: Option<(pb::ResolvedAttachmentMetadata, Vec<u8>)>,
) -> TalosResult<()> {
    if let Some((metadata, bytes)) = current {
        if metadata.size_bytes != bytes.len() as u64 {
            return Err(invalid_response());
        }
        resolved.push(ResolvedAttachment {
            file_id: metadata.file_id,
            filename: metadata.filename,
            media_type: metadata.media_type,
            size_bytes: metadata.size_bytes,
            bytes: Bytes::from(bytes),
        });
    }
    Ok(())
}

fn invalid_response() -> TalosError {
    TalosError::internal("Agent Session data RPC response is malformed")
}

fn transport_closed() -> TalosError {
    TalosError::new(
        ErrorCode::ServiceUnavailable,
        "Agent Session data upload closed before completion",
    )
}

fn rpc_request<T>(body: T, timeout: Duration) -> tonic::Request<T> {
    let mut request = tonic::Request::new(body);
    request.set_timeout(timeout);
    request
}
