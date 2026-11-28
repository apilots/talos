//! Short-lived callback routing to the original Worker; no OAuth credentials or product persistence.
use super::TalosService;
use crate::Actor;
use agent_protocol::{
    McpAuthenticationCallbackParams, McpAuthenticationStartParams, McpOAuthCallbackOutcome,
    McpOAuthCallbackTarget, WorkerEnvironment,
};
use sha2::{Digest, Sha256};
use std::sync::{Arc, Weak};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_types::{
    McpAuthenticationStartRequest, McpAuthenticationStartResponse, McpDefinitionRef,
};

/// A transient product callback route. Implementations recheck current Computer ownership.
#[async_trait::async_trait]
pub trait McpPersonalCallback: Send + Sync {
    async fn complete(
        &self,
        actor: &Actor,
        computer_id: &str,
        state: String,
        issuer: Option<String>,
        outcome: McpOAuthCallbackOutcome,
    ) -> TalosResult<()>;
}
enum McpRelayTarget {
    Worker(Weak<dyn WorkerEnvironment>),
    Personal(Weak<dyn McpPersonalCallback>),
}
impl McpRelayTarget {
    fn available(&self) -> bool {
        match self {
            Self::Worker(value) => value.strong_count() > 0,
            Self::Personal(value) => value.strong_count() > 0,
        }
    }
}
pub(super) struct McpRelayRoute {
    actor: Actor,
    node_id: String,
    server: McpDefinitionRef,
    attempt_id: String,
    expires_at: i64,
    target: McpRelayTarget,
    callback_gate: Arc<tokio::sync::Mutex<()>>,
    _capacity: tokio::sync::OwnedSemaphorePermit,
}

impl TalosService {
    pub async fn start_mcp_authentication(
        &self,
        actor: &Actor,
        request: McpAuthenticationStartRequest,
    ) -> TalosResult<McpAuthenticationStartResponse> {
        self.start_mcp_authentication_with_origin(actor, request, None)
            .await
    }

    /// Trusted relay composition supplies the public callback origin; callers cannot override it in JSON.
    pub async fn start_mcp_authentication_with_origin(
        &self,
        actor: &Actor,
        mut request: McpAuthenticationStartRequest,
        callback_origin: Option<&str>,
    ) -> TalosResult<McpAuthenticationStartResponse> {
        let requested_node_id = request
            .node_id
            .take()
            .unwrap_or_else(|| "embedded".to_owned());
        let node_id = self
            .connect_selected_computer(actor, &requested_node_id)
            .await?;
        let environment = self.access.worker_environment(actor, &node_id).await?;
        if callback_origin.is_none()
            && self.product_owner == talos_types::NodeProductOwner::Computer
        {
            return environment
                .start_mcp_authentication(
                    crate::access::worker_environment_context(actor),
                    McpAuthenticationStartParams {
                        request,
                        callback_target: McpOAuthCallbackTarget::Loopback,
                    },
                )
                .await;
        }
        let uri =
            crate::mcp_relay_callback_uri(callback_origin.unwrap_or(&self.oauth_relay_origin))?;
        self.prune_mcp_callbacks();
        self.oauth_routes
            .lock()
            .expect("MCP callback routes lock poisoned")
            .retain(|_, route| route.node_id != node_id || route.server != request.server);
        let capacity = self
            .oauth_capacity
            .clone()
            .try_acquire_owned()
            .map_err(|_| {
                TalosError::new(
                    ErrorCode::Conflict,
                    "Too many pending MCP authorization callbacks.",
                )
            })?;
        let server = request.server.clone();
        let response = environment
            .start_mcp_authentication(
                crate::access::worker_environment_context(actor),
                McpAuthenticationStartParams {
                    request,
                    callback_target: McpOAuthCallbackTarget::ControlPlaneRelay { uri },
                },
            )
            .await?;
        let authorization = url::Url::parse(&response.authorization_url).map_err(|_| invalid())?;
        let states = authorization
            .query_pairs()
            .filter(|(name, _)| name == "state")
            .map(|(_, value)| value.into_owned())
            .collect::<Vec<_>>();
        let [state] = states.as_slice() else {
            return Err(invalid());
        };
        if state.is_empty() || response.expires_at <= now() || response.expires_at > now() + 600 {
            return Err(invalid());
        }
        let current = self.access.worker_environment(actor, &node_id).await?;
        if !Arc::ptr_eq(&current, &environment) {
            return Err(TalosError::new(
                ErrorCode::ServiceUnavailable,
                "The Computer connection changed during authorization.",
            ));
        }
        let mut actor = actor.clone();
        actor.access_token = None;
        let route = Arc::new(McpRelayRoute {
            actor,
            node_id,
            server,
            attempt_id: response.attempt_id.clone(),
            expires_at: response.expires_at,
            target: McpRelayTarget::Worker(Arc::downgrade(&environment)),
            callback_gate: Arc::default(),
            _capacity: capacity,
        });
        let mut routes = self
            .oauth_routes
            .lock()
            .expect("MCP callback routes lock poisoned");
        routes.retain(|_, previous| {
            previous.node_id != route.node_id || previous.server != route.server
        });
        let key = state_hash(state);
        if routes.contains_key(&key) {
            return Err(invalid());
        }
        routes.insert(key, route);
        Ok(response)
    }

    /// Public browser entrypoint. Standard OAuth state selects recorded policy, never a caller identity.
    /// Malformed callbacks preserve the attempt; uncertain transport outcomes are never retried.
    pub async fn complete_mcp_oauth_callback(
        &self,
        state: String,
        issuer: Option<String>,
        outcome: McpOAuthCallbackOutcome,
    ) -> TalosResult<()> {
        self.prune_mcp_callbacks();
        let key = state_hash(&state);
        let route = self
            .oauth_routes
            .lock()
            .expect("MCP callback routes lock poisoned")
            .get(&key)
            .cloned()
            .ok_or_else(|| {
                TalosError::new(
                    ErrorCode::ResourceNotFound,
                    "The MCP authorization callback is unavailable.",
                )
            })?;
        let _callback = route.callback_gate.clone().try_lock_owned().map_err(|_| {
            TalosError::new(
                ErrorCode::Conflict,
                "An MCP callback is already being processed.",
            )
        })?;
        let result = async {
            let McpRelayTarget::Worker(original) = &route.target else {
                let McpRelayTarget::Personal(callback) = &route.target else {
                    unreachable!()
                };
                return callback
                    .upgrade()
                    .ok_or_else(unavailable)?
                    .complete(&route.actor, &route.node_id, state, issuer, outcome)
                    .await;
            };
            let original = original.upgrade().ok_or_else(unavailable)?;
            // Admission already resolved this route. Recheck ownership and the current
            // connection without reopening account sync with a browser callback.
            let environment = self
                .access
                .worker_environment(&route.actor, &route.node_id)
                .await?;
            if !Arc::ptr_eq(&original, &environment) {
                return Err(unavailable());
            }
            environment
                .complete_mcp_authentication(
                    crate::access::worker_environment_context(&route.actor),
                    McpAuthenticationCallbackParams {
                        server: route.server.clone(),
                        attempt_id: route.attempt_id.clone(),
                        state,
                        issuer,
                        outcome,
                    },
                )
                .await
                .map(|_| ())
        }
        .await;
        if !result.as_ref().is_err_and(|error| {
            matches!(
                error.error_code(),
                ErrorCode::InvalidRequest | ErrorCode::Conflict
            )
        }) {
            let mut routes = self
                .oauth_routes
                .lock()
                .expect("MCP callback routes lock poisoned");
            if routes
                .get(&key)
                .is_some_and(|current| Arc::ptr_eq(current, &route))
            {
                routes.remove(&key);
            }
        }
        result
    }

    /// The Server owns only short-lived OAuth state routing, never tokens or product content.
    pub fn remember_personal_mcp_callback(
        &self,
        actor: &Actor,
        node_id: String,
        response: &McpAuthenticationStartResponse,
        callback: &Arc<dyn McpPersonalCallback>,
    ) -> TalosResult<()> {
        self.prune_mcp_callbacks();
        let authorization = url::Url::parse(&response.authorization_url).map_err(|_| invalid())?;
        let states: Vec<_> = authorization
            .query_pairs()
            .filter(|(key, _)| key == "state")
            .map(|(_, value)| value.into_owned())
            .collect();
        let [state] = states.as_slice() else {
            return Err(invalid());
        };
        if state.is_empty()
            || state.len() > 4096
            || response.expires_at <= now()
            || response.expires_at > now() + 600
        {
            return Err(invalid());
        }
        let capacity = self
            .oauth_capacity
            .clone()
            .try_acquire_owned()
            .map_err(|_| {
                TalosError::new(
                    ErrorCode::Conflict,
                    "Too many pending MCP authorization callbacks.",
                )
            })?;
        let mut actor = actor.clone();
        actor.access_token = None;
        let route = Arc::new(McpRelayRoute {
            actor,
            node_id,
            server: response.server.clone(),
            attempt_id: response.attempt_id.clone(),
            expires_at: response.expires_at,
            target: McpRelayTarget::Personal(Arc::downgrade(callback)),
            callback_gate: Arc::default(),
            _capacity: capacity,
        });
        let mut routes = self
            .oauth_routes
            .lock()
            .expect("MCP callback routes lock poisoned");
        routes.retain(|_, previous| {
            previous.node_id != route.node_id || previous.server != route.server
        });
        let key = state_hash(state);
        if routes.contains_key(&key) {
            return Err(invalid());
        }
        routes.insert(key, route);
        Ok(())
    }
    /// Only the trusted product composition may carry this origin across a relay connection.
    pub fn mcp_callback_origin(&self) -> &str {
        &self.oauth_relay_origin
    }

    /// Logout discards pending callback policy; node-local attempts also end on host logout or expiry.
    pub fn invalidate_mcp_callbacks(&self, actor: &Actor) {
        self.oauth_routes
            .lock()
            .expect("MCP callback routes lock poisoned")
            .retain(|_, route| route.actor.user_id != actor.user_id);
    }
    fn prune_mcp_callbacks(&self) {
        self.oauth_routes
            .lock()
            .expect("MCP callback routes lock poisoned")
            .retain(|_, route| route.expires_at > now() && route.target.available());
    }
}
fn state_hash(state: &str) -> [u8; 32] {
    Sha256::digest(state.as_bytes()).into()
}
fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
fn unavailable() -> TalosError {
    TalosError::new(
        ErrorCode::ServiceUnavailable,
        "The original Computer connection is unavailable.",
    )
}
fn invalid() -> TalosError {
    TalosError::new(
        ErrorCode::InvalidRequest,
        "The MCP authorization callback is invalid.",
    )
}
