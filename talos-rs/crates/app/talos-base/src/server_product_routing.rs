//! Cloud entry composition: cloud products are local, personal products relay.
use std::sync::Arc;
use talos_control_plane::AgentNodeService;
use talos_error::TalosResult;
use talos_error::{ErrorCode, TalosError};
use talos_gateway_http::{ProductRequestRouter, ProductRoute, ProductRouteFuture};
#[cfg(feature = "relay")]
use talos_service::McpPersonalCallback;
use talos_service::TalosService;
use talos_types::NodeProductOwner;

pub(crate) struct ServerProductRouter {
    local_host_id: String,
    nodes: AgentNodeService,
    #[cfg(feature = "relay")]
    relay: talos_relay_server::RelayServer,
    #[cfg(feature = "relay")]
    service: Arc<TalosService>,
    #[cfg(feature = "relay")]
    callbacks: Arc<dyn McpPersonalCallback>,
}
impl ServerProductRouter {
    #[cfg(feature = "relay")]
    pub(crate) fn with_relay(
        nodes: AgentNodeService,
        service: Arc<TalosService>,
        local_host_id: String,
        relay: talos_relay_server::RelayServer,
    ) -> Self {
        let callbacks: Arc<dyn McpPersonalCallback> = Arc::new(PersonalCallback {
            relay: relay.clone(),
        });
        Self {
            local_host_id,
            nodes,
            service,
            relay,
            callbacks,
        }
    }
    pub(crate) fn assemble(
        server: talos_gateway_http::HttpServer,
        nodes: AgentNodeService,
        service: Arc<TalosService>,
        local_host_id: String,
        shutdown: tokio_util::sync::CancellationToken,
    ) -> talos_gateway_http::HttpServer {
        #[cfg(feature = "relay")]
        let relay = talos_relay_server::RelayServer::new(nodes.clone());
        #[cfg(feature = "relay")]
        let server = server.with_routes(relay.routes(shutdown));
        #[cfg(not(feature = "relay"))]
        let _ = (shutdown, service);
        #[cfg(feature = "relay")]
        let routing = Self::with_relay(nodes, service, local_host_id, relay);
        #[cfg(not(feature = "relay"))]
        let routing = Self {
            nodes,
            local_host_id,
        };
        server.with_product_router(Arc::new(routing))
    }
}
impl ProductRequestRouter for ServerProductRouter {
    fn resolve_source_peer<'a>(
        &'a self,
        actor: talos_auth::Actor,
        peer_id: String,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = TalosResult<String>> + Send + 'a>> {
        Box::pin(async move {
            if peer_id == self.local_host_id {
                #[cfg(feature = "relay")]
                match self.relay.resolve_source_peer(&actor, &peer_id).await {
                    Err(error) if error.is_code(ErrorCode::ResourceNotFound) => {}
                    Err(error) if !error.is_code(ErrorCode::Conflict) => return Err(error),
                    _ => {
                        return Err(TalosError::new(
                            ErrorCode::Conflict,
                            "Source peer identity is attached to multiple products.",
                        ))
                    }
                }
                return Ok("embedded".into());
            }
            #[cfg(feature = "relay")]
            {
                self.relay.resolve_source_peer(&actor, &peer_id).await
            }
            #[cfg(not(feature = "relay"))]
            {
                let _ = (actor, peer_id);
                Err(TalosError::new(
                    ErrorCode::ServiceUnavailable,
                    "Personal source routing is not assembled.",
                ))
            }
        })
    }
    fn preview<'a>(
        &'a self,
        computer_id: String,
        request: axum::extract::Request,
    ) -> ProductRouteFuture<'a> {
        Box::pin(async move {
            #[cfg(feature = "relay")]
            {
                Ok(ProductRoute::Remote(
                    self.relay.forward_preview(&computer_id, request).await?,
                ))
            }
            #[cfg(not(feature = "relay"))]
            {
                let _ = (computer_id, request);
                Err(TalosError::new(
                    ErrorCode::ServiceUnavailable,
                    "Personal preview relay is not assembled.",
                ))
            }
        })
    }
    fn route<'a>(
        &'a self,
        actor: talos_auth::Actor,
        computer_id: String,
        request: axum::extract::Request,
    ) -> ProductRouteFuture<'a> {
        Box::pin(async move {
            let requested_alias = (computer_id == "talo").then(|| computer_id.clone());
            let computer_id = if computer_id == "talo" {
                self.nodes
                    .read_managed_worker(&actor)
                    .await?
                    .instance
                    .ok_or_else(|| {
                        TalosError::new(
                            ErrorCode::ResourceNotFound,
                            "Managed Worker is unavailable.",
                        )
                    })?
                    .node_id
            } else {
                computer_id
            };
            match self.nodes.product_owner(&actor, &computer_id).await? {
                Some(NodeProductOwner::Server) => {
                    let mut request = request;
                    request
                        .extensions_mut()
                        .insert(talos_gateway_http::SelectedComputer {
                            computer_id,
                            requested_alias,
                        });
                    Ok(ProductRoute::Local(request))
                }
                Some(NodeProductOwner::Computer) => {
                    #[cfg(feature = "relay")]
                    {
                        let mut request = request;
                        request.extensions_mut().insert(
                            talos_app_protocol::relay::RelayCallbackOrigin(
                                self.service.mcp_callback_origin().to_owned(),
                            ),
                        );
                        let mut response =
                            self.relay.forward(&actor, &computer_id, request).await?;
                        // Only the authenticated Host's typed MCP start handler emits this marker.
                        // Both REST and generic application dispatch share that handler behavior.
                        let operation = response.headers_mut().remove("x-talos-product-operation");
                        if response.status().is_success()
                            && operation.as_ref().is_some_and(|value| {
                                value == "mcp-authentication-start"
                                    || value == "file-preview-prepare"
                            })
                        {
                            let (mut parts, body) = response.into_parts();
                            let bytes =
                                axum::body::to_bytes(body, 64 * 1024)
                                    .await
                                    .map_err(|error| {
                                        TalosError::internal_with_source(
                                            "Read bounded product preparation response.",
                                            error,
                                        )
                                    })?;
                            let bytes = if operation
                                .as_ref()
                                .is_some_and(|value| value == "mcp-authentication-start")
                            {
                                let envelope: talos_types::ApiResponse<
                                    talos_types::McpAuthenticationStartResponse,
                                > = serde_json::from_slice(&bytes).map_err(|error| {
                                    TalosError::internal_with_source(
                                        "Decode MCP authorization response.",
                                        error,
                                    )
                                })?;
                                if let talos_types::ApiResponse::Success { data } = envelope {
                                    self.service.remember_personal_mcp_callback(
                                        &actor,
                                        computer_id,
                                        &data,
                                        &self.callbacks,
                                    )?;
                                }
                                bytes
                            } else {
                                let mut envelope: talos_types::ApiResponse<
                                    talos_types::FilePreviewDescriptor,
                                > = serde_json::from_slice(&bytes).map_err(|error| {
                                    TalosError::internal_with_source(
                                        "Decode personal preview descriptor.",
                                        error,
                                    )
                                })?;
                                if let talos_types::ApiResponse::Success { data } = &mut envelope {
                                    if let Some(base) = self.service.file_preview_origin() {
                                        let mut origin =
                                            url::Url::parse(base).map_err(|error| {
                                                TalosError::internal_with_source(
                                                    "Resolve personal preview origin.",
                                                    error,
                                                )
                                            })?;
                                        let host = origin.host_str().ok_or_else(|| {
                                            TalosError::internal("Preview origin has no host.")
                                        })?;
                                        origin
                                            .set_host(Some(&format!(
                                                "{}.{}",
                                                talos_gateway_http::PersonalPreviewRoute::new(
                                                    &computer_id,
                                                    &data.preview_id
                                                )?
                                                .host_label(),
                                                host
                                            )))
                                            .map_err(|error| {
                                                TalosError::internal_with_source(
                                                    "Resolve isolated personal preview host.",
                                                    error,
                                                )
                                            })?;
                                        origin.set_path("/");
                                        data.resource_base_url = Some(origin.to_string());
                                    } else {
                                        data.resource_base_url = None;
                                    }
                                }
                                parts.headers.remove("content-length");
                                parts.headers.remove("etag");
                                serde_json::to_vec(&envelope)
                                    .map_err(|error| {
                                        TalosError::internal_with_source(
                                            "Encode personal preview descriptor.",
                                            error,
                                        )
                                    })?
                                    .into()
                            };
                            response = axum::http::Response::from_parts(
                                parts,
                                axum::body::Body::from(bytes),
                            );
                        }
                        Ok(ProductRoute::Remote(response))
                    }
                    #[cfg(not(feature = "relay"))]
                    {
                        let _ = request;
                        Err(TalosError::new(
                            ErrorCode::ServiceUnavailable,
                            "Personal Computer relay is not assembled in this Server",
                        ))
                    }
                }
                None => Err(TalosError::new(
                    ErrorCode::ServiceUnavailable,
                    "Computer must register its product ownership before remote access",
                )),
            }
        })
    }
}
#[cfg(feature = "relay")]
struct PersonalCallback {
    relay: talos_relay_server::RelayServer,
}
#[cfg(feature = "relay")]
#[async_trait::async_trait]
impl McpPersonalCallback for PersonalCallback {
    async fn complete(
        &self,
        actor: &talos_auth::Actor,
        computer_id: &str,
        state: String,
        issuer: Option<String>,
        outcome: agent_protocol::McpOAuthCallbackOutcome,
    ) -> TalosResult<()> {
        let query = {
            let mut query = url::form_urlencoded::Serializer::new(String::new());
            query.append_pair("state", &state);
            if let Some(issuer) = issuer {
                query.append_pair("iss", &issuer);
            }
            match outcome {
                agent_protocol::McpOAuthCallbackOutcome::AuthorizationCode(value) => {
                    query.append_pair("code", &value);
                }
                agent_protocol::McpOAuthCallbackOutcome::ProviderErrorCode(value) => {
                    query.append_pair("error", &value);
                }
            }
            query.finish()
        };
        let request = axum::http::Request::builder()
            .uri(format!("/v1/mcp/oauth/callback?{query}"))
            .body(axum::body::Body::empty())
            .map_err(|error| {
                TalosError::internal_with_source("Build personal MCP callback.", error)
            })?;
        let response = self.relay.forward(actor, computer_id, request).await?;
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .map_err(|error| {
                TalosError::internal_with_source("Read bounded personal MCP callback.", error)
            })?;
        if status.is_success() {
            return Ok(());
        }
        let envelope: talos_types::ApiResponse<()> =
            serde_json::from_slice(&body).map_err(|error| {
                TalosError::internal_with_source("Decode personal MCP callback error.", error)
            })?;
        match envelope {
            talos_types::ApiResponse::Error { error } => Err(error.into()),
            talos_types::ApiResponse::Success { .. } => Err(TalosError::new(
                ErrorCode::ServiceUnavailable,
                "Personal MCP callback response is invalid.",
            )),
        }
    }
}

#[cfg(all(test, not(feature = "relay")))]
mod local_source_tests {
    use super::*;
    #[tokio::test]
    async fn server_source_peer_does_not_require_relay_assembly() {
        let storage = talos_storage::SqliteStorage::new_with_max_connections(":memory:", 1)
            .await
            .unwrap();
        let nodes = AgentNodeService::new(Arc::new(
            talos_store::node::DatabaseAgentNodeStore::new(&storage),
        ));
        let routing = ServerProductRouter {
            local_host_id: "server-product-host".into(),
            nodes,
        };
        let actor = talos_auth::Actor::regular("owner");
        assert_eq!(
            routing
                .resolve_source_peer(actor.clone(), "server-product-host".into())
                .await
                .unwrap(),
            "embedded"
        );
        assert!(routing
            .resolve_source_peer(actor, "personal-source".into())
            .await
            .unwrap_err()
            .is_code(ErrorCode::ServiceUnavailable));
    }
}
