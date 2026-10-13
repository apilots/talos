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

pub(super) struct McpRelayRoute {
    actor: Actor,
    node_id: String,
    server: McpDefinitionRef,
    attempt_id: String,
    expires_at: i64,
    environment: Weak<dyn WorkerEnvironment>,
    callback_gate: Arc<tokio::sync::Mutex<()>>,
    _capacity: tokio::sync::OwnedSemaphorePermit,
}

impl TalosService {
    pub async fn start_mcp_authentication(
        &self,
        actor: &Actor,
        mut request: McpAuthenticationStartRequest,
    ) -> TalosResult<McpAuthenticationStartResponse> {
        let node_id = request
            .node_id
            .take()
            .unwrap_or_else(|| "embedded".to_owned());
        let environment = self.worker_environment(actor, Some(&node_id)).await?;
        if self.mode() == talos_config::DeploymentMode::Local && node_id == "embedded" {
            return environment
                .start_mcp_authentication(
                    actor.worker_environment_context(),
                    McpAuthenticationStartParams {
                        request,
                        callback_target: McpOAuthCallbackTarget::Loopback,
                    },
                )
                .await;
        }
        let uri = crate::mcp_relay_callback_uri(&self.oauth_relay_origin)?;
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
                actor.worker_environment_context(),
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
        let current = self.worker_environment(actor, Some(&node_id)).await?;
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
            environment: Arc::downgrade(&environment),
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
            let original = route.environment.upgrade().ok_or_else(unavailable)?;
            let environment = self
                .worker_environment(&route.actor, Some(&route.node_id))
                .await?;
            if !Arc::ptr_eq(&original, &environment) {
                return Err(unavailable());
            }
            let _admission = self
                .execution_gate
                .for_node(&route.node_id)
                .write_owned()
                .await;
            self.sessions
                .reject_nonterminal_turns(&route.node_id)
                .await?;
            environment
                .complete_mcp_authentication(
                    route.actor.worker_environment_context(),
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
            .retain(|_, route| route.expires_at > now() && route.environment.upgrade().is_some());
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
