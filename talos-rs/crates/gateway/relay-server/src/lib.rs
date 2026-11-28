//! Optional personal-product HTTP relay. This library owns ephemeral transport
//! state only; product Stores, scheduling and execution remain with their owners.
mod forward;
mod socket;
mod state;

use axum::{extract::ws::WebSocketUpgrade, routing::get, Router};
use std::{
    collections::HashMap,
    sync::{atomic::AtomicU64, Arc},
};
use talos_auth::Actor;
use talos_control_plane::AgentNodeService;
use talos_error::{ErrorCode, TalosError, TalosResult};
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct RelayServer {
    inner: Arc<Inner>,
}
struct Inner {
    nodes: AgentNodeService,
    connections: RwLock<HashMap<String, Arc<state::Connection>>>,
    next_stream: AtomicU64,
}
impl RelayServer {
    pub fn new(nodes: AgentNodeService) -> Self {
        Self {
            inner: Arc::new(Inner {
                nodes,
                connections: Default::default(),
                next_stream: AtomicU64::new(1),
            }),
        }
    }
    /// Mount beside the existing product HTTP API. The host actively connects
    /// with its current node lease; no personal host listener is required.
    pub fn routes(&self, shutdown: CancellationToken) -> Router {
        let server = self.clone();
        Router::new().route(
            "/talos/product/connect",
            get(move |upgrade: WebSocketUpgrade| {
                let server = server.clone();
                let shutdown = shutdown.clone();
                async move {
                    upgrade
                        .max_message_size(talos_app_protocol::relay::RELAY_MAX_FRAME_BYTES)
                        .max_frame_size(talos_app_protocol::relay::RELAY_MAX_FRAME_BYTES)
                        .on_upgrade(move |socket| async move {
                            if let Err(error) = server.connected(socket, shutdown).await {
                                talos_logging::report_error(&error);
                            }
                        })
                }
            }),
        )
    }
    /// Resolve the existing stable source peer identity against live lease-authenticated hosts.
    /// Duplicate aliases within one account are rejected instead of selecting an arbitrary source.
    pub async fn resolve_source_peer(&self, actor: &Actor, peer_id: &str) -> TalosResult<String> {
        if peer_id.is_empty() || peer_id.len() > 128 {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Source peer identity is invalid.",
            ));
        }
        let candidates: Vec<_> = self
            .inner
            .connections
            .read()
            .await
            .values()
            .filter(|entry| entry.host_id == peer_id && !entry.cancel.is_cancelled())
            .cloned()
            .collect();
        let mut selected = None;
        for connection in candidates {
            if let Err(error) = self.inner.nodes.authenticate_lease(&connection.auth).await {
                if matches!(
                    error.error_code(),
                    ErrorCode::Unauthenticated
                        | ErrorCode::PermissionDenied
                        | ErrorCode::ResourceNotFound
                        | ErrorCode::ResourceExpired
                ) {
                    continue;
                }
                return Err(error);
            }
            if self
                .inner
                .nodes
                .authorize_lease_owner(&connection.auth)
                .await?
                != actor.user_id
            {
                continue;
            }
            self.require_personal(actor, &connection.auth.node_id)
                .await?;
            if selected.replace(connection.auth.node_id.clone()).is_some() {
                return Err(TalosError::new(
                    ErrorCode::Conflict,
                    "Source peer identity is attached to multiple personal Computers.",
                ));
            }
        }
        selected.ok_or_else(|| {
            TalosError::new(
                ErrorCode::ResourceNotFound,
                "Source peer is not attached to this account.",
            )
        })
    }

    async fn require_personal(&self, actor: &Actor, computer: &str) -> TalosResult<()> {
        if self.inner.nodes.product_owner(actor, computer).await?
            != Some(talos_types::NodeProductOwner::Computer)
        {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Cloud products use their Server product API directly.",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "relay_tests.rs"]
mod tests;
