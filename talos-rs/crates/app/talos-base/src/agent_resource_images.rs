use super::*;
#[derive(Clone)]
pub struct NodeToolImageRpc {
    nodes: AgentNodeService,
    images: Arc<dyn ToolImageStore>,
    max_upload_bytes: u64,
    authorization: Option<NodeSessionDataRpc>,
    routes: Option<ResourceRoutes>,
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
            authorization: None,
            routes: None,
        }
    }
}

#[tonic::async_trait]
impl pb::agent_tool_image_rpc_server::AgentToolImageRpc for NodeToolImageRpc {
    async fn save_tool_image(
        &self,
        request: Request<tonic::Streaming<pb::SaveToolImageRequest>>,
    ) -> Result<Response<pb::SaveToolImageResponse>, Status> {
        self.save_stream(request.into_inner()).await
    }
}
impl NodeToolImageRpc {
    pub(in crate::agent_node_rpc) fn with_routes(mut self, routes: ResourceRoutes) -> Self {
        self.routes = Some(routes);
        self
    }
    pub(crate) fn with_authorization(mut self, authorization: NodeSessionDataRpc) -> Self {
        self.authorization = Some(authorization);
        self
    }
    pub(crate) async fn save_stream<S>(
        &self,
        stream: S,
    ) -> Result<Response<pb::SaveToolImageResponse>, Status>
    where
        S: Stream<Item = Result<pb::SaveToolImageRequest, Status>> + Send + 'static,
    {
        use futures_util::StreamExt;
        let mut stream = Box::pin(stream);
        let first = stream
            .next()
            .await
            .transpose()?
            .ok_or_else(resource_status)?;
        if let Some(caller) = first
            .caller
            .clone()
            .filter(|caller| !caller.connection_id.is_empty())
        {
            if self.authorization.is_none() {
                let routes = self.routes.as_ref().ok_or_else(resource_status)?;
                let auth = first.auth.clone();
                let requests = tokio_stream::once(Ok(first)).chain(stream).map(|value| {
                    value.map(|value| pb::worker_rpc_call::Request::SaveToolImage(Box::new(value)))
                });
                let mut response = routes
                    .forward(&self.nodes, auth, caller, Box::pin(requests))
                    .await?;
                let value = response
                    .next()
                    .await
                    .transpose()?
                    .ok_or_else(resource_status)?;
                let pb::worker_rpc_result::Response::SaveToolImage(value) = value else {
                    return Err(resource_status());
                };
                return Ok(Response::new(*value));
            }
        }
        let stream = tokio_stream::once(Ok(first)).chain(stream);
        let result = async {
            let mut stream = Box::pin(stream);
            let first = stream
                .next()
                .await
                .transpose()
                .map_err(|error| {
                    TalosError::from_source(
                        ErrorCode::InvalidRequest,
                        "Tool image upload stream is invalid",
                        error,
                    )
                })?
                .ok_or_else(invalid_resource_request)?;
            let auth = node_pb::decode_auth(first.auth)?;
            let owner = if let Some(authorization) = &self.authorization {
                authorization
                    .authorize_owner(&auth, first.caller.as_ref())
                    .await?
            } else {
                self.nodes.authorize_lease_owner(&auth).await?
            };
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
            while let Some(frame) = stream.next().await.transpose().map_err(|error| {
                TalosError::from_source(
                    ErrorCode::InvalidRequest,
                    "Tool image upload stream is invalid",
                    error,
                )
            })? {
                let frame_auth = node_pb::decode_auth(frame.auth)?;
                if frame_auth.node_id != auth.node_id
                    || frame_auth.token != auth.token
                    || frame.caller != first.caller
                {
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
