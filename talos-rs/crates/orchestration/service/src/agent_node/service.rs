use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use agent_protocol::{
    CreateNodeRequest, CreatedNode, GetNodeRequest, HeartbeatParams, HeartbeatResult,
    LeaseValidity, ListNodesRequest, NodeAuth, NodeCredential, NodeInfo, NodeMode, NodePage,
    NodeStatus, RegisterParams, Registration, RotateCredentialRequest, UpdateNodeRequest,
};
use base64::Engine;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use talos_auth::UserRole;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_store::node::{
    AgentNodeStore, CreateNodeParams, HeartbeatNodeParams, LeaseCheck, ListNodesParams,
    NodeListScope, NodeRecord, RegisterNodeParams, RenewNodeParams, RotateCredentialParams,
    UpdateNodeParams,
};
use talos_store::session::SessionRecord;
use tokio::sync::Mutex;

use super::types::NodePrincipal;
use crate::Actor;

const LEASE_SECONDS: i64 = 90;

/// Withdraws the live publication installed by the transport owner.
/// The callback must release only its own registration, and must not reenter NodeService.
pub type AgentNodeRevocationHook =
    Arc<dyn Fn() -> agent_protocol::AgentFuture<'static, ()> + Send + Sync>;

#[path = "computers.rs"]
mod computers;
#[path = "management.rs"]
mod management;

/// Owns the node directory and its current in-memory registration. Execution
/// state and Session/Turn persistence remain with the existing owners.
#[derive(Clone)]
pub struct AgentNodeService {
    store: Arc<dyn AgentNodeStore>,
    current: Arc<Mutex<HashMap<String, CurrentRegistration>>>,
    lifecycle: Arc<Mutex<()>>,
}

struct CurrentRegistration {
    token_hash: [u8; 32],
    deployments: Vec<agent_protocol::Deployment>,
    revoke: Option<AgentNodeRevocationHook>,
}

impl AgentNodeService {
    pub fn new(store: Arc<dyn AgentNodeStore>) -> Self {
        Self {
            store,
            current: Arc::new(Mutex::new(HashMap::new())),
            lifecycle: Arc::new(Mutex::new(())),
        }
    }

    /// Serialize Register/Unregister together with transport publication.
    /// Directory mutations take this same lock internally. Never hold it during execution.
    pub async fn lock_lifecycle(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.lifecycle.lock().await
    }

    /// Persist the trusted in-process node after composition installs its
    /// Harnesses in Coordinator. No credential or network lease is issued.
    pub async fn register_embedded(&self, has_harnesses: bool) -> TalosResult<NodeInfo> {
        let status = if has_harnesses {
            NodeStatus::Enabled
        } else {
            NodeStatus::Disabled
        };
        self.store.upsert_embedded(status, now()).await.map(to_info)
    }

    /// Authenticate the provisioned credential before entering registration.
    pub async fn authenticate_registration(&self, auth: &NodeAuth) -> TalosResult<NodePrincipal> {
        let node = self
            .authenticated_node(auth, CredentialKind::Registration)
            .await?;
        let credential_hash = node.credential_hash.ok_or_else(unauthenticated)?;
        Ok(NodePrincipal::Registration {
            node_id: node.node_id,
            credential_hash,
        })
    }

    /// Authenticate the current lease before entering a node control method.
    pub async fn authenticate_lease(&self, auth: &NodeAuth) -> TalosResult<NodePrincipal> {
        let node = self.authenticated_node(auth, CredentialKind::Lease).await?;
        let token_hash = node.lease_token_hash.ok_or_else(unauthenticated)?;
        Ok(NodePrincipal::Lease {
            node_id: node.node_id,
            token_hash,
        })
    }

    /// Authorize a node's current lease to read data for its bound Session.
    /// The Session record must come from the control-plane Store, never from
    /// untrusted RPC fields.
    pub async fn authorize_session_lease(
        &self,
        auth: &NodeAuth,
        session: &SessionRecord,
    ) -> TalosResult<()> {
        let node = self.authenticated_node(auth, CredentialKind::Lease).await?;
        if node.node_id != session.execution_node_id
            || node.owner_user_id.as_deref() != Some(session.user_id.as_str())
        {
            return Err(TalosError::new(
                ErrorCode::PermissionDenied,
                "Agent node cannot access this Session",
            ));
        }
        Ok(())
    }

    /// Returns the user represented by a valid current node lease. Remote
    /// user-scoped ports use this identity instead of accepting one from RPC.
    pub async fn authorize_lease_owner(&self, auth: &NodeAuth) -> TalosResult<String> {
        self.authenticated_node(auth, CredentialKind::Lease)
            .await?
            .owner_user_id
            .ok_or_else(unauthenticated)
    }

    /// Marks the current lease executable only after the Coordinator has
    /// installed every advertised remote Harness client.
    pub async fn mark_registration_published(
        &self,
        auth: &NodeAuth,
        revoke: AgentNodeRevocationHook,
    ) -> TalosResult<()> {
        self.authenticate_lease(auth).await?;
        let actual = hash_token(&auth.token);
        let mut current = self.current.lock().await;
        let registration = current.get_mut(&auth.node_id).ok_or_else(unauthenticated)?;
        if !bool::from(registration.token_hash.ct_eq(&actual)) {
            return Err(unauthenticated());
        }
        registration.revoke = Some(revoke);
        Ok(())
    }

    /// Return only the deployments advertised by this current authenticated lease.
    pub async fn outbound_deployments(
        &self,
        auth: &NodeAuth,
    ) -> TalosResult<Vec<agent_protocol::Deployment>> {
        self.authenticate_lease(auth).await?;
        let node = self
            .store
            .get(&auth.node_id)
            .await?
            .ok_or_else(unauthenticated)?;
        if node.endpoint.is_some() {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Direct nodes cannot open outbound execution",
            ));
        }
        let current = self.current.lock().await;
        let registration = current.get(&auth.node_id).ok_or_else(unauthenticated)?;
        if !bool::from(registration.token_hash.ct_eq(&hash_token(&auth.token))) {
            return Err(unauthenticated());
        }
        Ok(registration.deployments.clone())
    }

    /// Withdraw the live publication, retaining the authenticated registration.
    /// Called under lock_lifecycle before replacing clients for the same lease.
    pub async fn revoke_publication(&self, auth: &NodeAuth) -> TalosResult<()> {
        self.authenticate_lease(auth).await?;
        let mut current = self.current.lock().await;
        let registration = current.get_mut(&auth.node_id).ok_or_else(unauthenticated)?;
        if !bool::from(registration.token_hash.ct_eq(&hash_token(&auth.token))) {
            return Err(unauthenticated());
        }
        let revoke = registration.revoke.take();
        drop(current);
        if let Some(revoke) = revoke {
            revoke().await;
        }
        Ok(())
    }

    /// A superseded connection must never withdraw the newer publication.
    pub async fn withdraw_publication(&self, auth: &NodeAuth, expected: &AgentNodeRevocationHook) {
        let mut current = self.current.lock().await;
        let revoke = current.get_mut(&auth.node_id).and_then(|registration| {
            if bool::from(registration.token_hash.ct_eq(&hash_token(&auth.token)))
                && registration
                    .revoke
                    .as_ref()
                    .is_some_and(|revoke| Arc::ptr_eq(revoke, expected))
            {
                registration.revoke.take()
            } else {
                None
            }
        });
        drop(current);
        if let Some(revoke) = revoke {
            revoke().await;
        }
    }

    async fn authenticated_node(
        &self,
        auth: &NodeAuth,
        kind: CredentialKind,
    ) -> TalosResult<NodeRecord> {
        if auth.node_id.trim().is_empty() || auth.token.is_empty() {
            return Err(unauthenticated());
        }
        let node = self
            .store
            .get(&auth.node_id)
            .await?
            .ok_or_else(unauthenticated)?;
        let expected = match kind {
            CredentialKind::Registration => node.credential_hash,
            CredentialKind::Lease => node.lease_token_hash,
        }
        .ok_or_else(unauthenticated)?;
        let actual = hash_token(&auth.token);
        if !bool::from(expected.ct_eq(&actual)) {
            return Err(unauthenticated());
        }
        let now_at = now();
        let expires_at = match kind {
            CredentialKind::Registration => node.credential_expires_at,
            CredentialKind::Lease => node.lease_expires_at,
        };
        if expires_at.is_none_or(|expiry| expiry <= now_at) {
            return Err(TalosError::new(
                ErrorCode::ResourceExpired,
                "Agent node credential or lease has expired",
            ));
        }
        if node.status != NodeStatus::Enabled || node.mode != NodeMode::Remote {
            return Err(TalosError::new(
                ErrorCode::PermissionDenied,
                "Agent node is disabled",
            ));
        }
        Ok(node)
    }

    pub async fn register(
        &self,
        principal: &NodePrincipal,
        params: RegisterParams,
    ) -> TalosResult<Registration> {
        let NodePrincipal::Registration {
            node_id,
            credential_hash,
        } = principal
        else {
            return Err(unauthenticated());
        };
        validate_deployments(&params)?;
        let mut current = self.current.lock().await;
        let now_at = now();
        let node = self.store.get(node_id).await?.ok_or_else(unauthenticated)?;
        let credential_expires_at = node.credential_expires_at.ok_or_else(unauthenticated)?;
        let expires_at = (now_at + LEASE_SECONDS).min(credential_expires_at);
        let token = new_token()?;
        let token_hash = hash_token(&token);
        let registered = self
            .store
            .register(RegisterNodeParams {
                node_id: node_id.clone(),
                expected_credential_hash: *credential_hash,
                lease_token_hash: token_hash,
                lease_expires_at: expires_at,
                now_at,
            })
            .await?;
        let previous = current.insert(
            node_id.clone(),
            CurrentRegistration {
                token_hash,
                deployments: params.deployments,
                revoke: None,
            },
        );
        drop(current);
        revoke_registration(previous).await;
        Ok(Registration {
            lease_token: token,
            validity: LeaseValidity {
                expires_at,
                server_at: now_at,
            },
            owner_user_id: registered
                .owner_user_id
                .ok_or_else(|| TalosError::internal("Remote Agent node has no owner"))?,
        })
    }

    pub async fn renew_lease(&self, principal: &NodePrincipal) -> TalosResult<LeaseValidity> {
        let NodePrincipal::Lease {
            node_id,
            token_hash,
        } = principal
        else {
            return Err(unauthenticated());
        };
        let current = self.current.lock().await;
        let now_at = now();
        let node = self.store.get(node_id).await?.ok_or_else(unauthenticated)?;
        let expires_at =
            (now_at + LEASE_SECONDS).min(node.credential_expires_at.ok_or_else(unauthenticated)?);
        let renewed = self
            .store
            .renew(RenewNodeParams {
                lease: LeaseCheck {
                    node_id: node_id.clone(),
                    token_hash: *token_hash,
                    now_at,
                },
                expires_at,
            })
            .await?;
        drop(current);
        Ok(LeaseValidity {
            expires_at: renewed
                .lease_expires_at
                .ok_or_else(|| TalosError::internal("Renewed Agent node has no lease"))?,
            server_at: now_at,
        })
    }

    pub async fn heartbeat(
        &self,
        principal: &NodePrincipal,
        params: HeartbeatParams,
    ) -> TalosResult<HeartbeatResult> {
        let NodePrincipal::Lease {
            node_id,
            token_hash,
        } = principal
        else {
            return Err(unauthenticated());
        };
        let current = self.current.lock().await;
        let now_at = now();
        self.store
            .heartbeat(HeartbeatNodeParams {
                lease: LeaseCheck {
                    node_id: node_id.clone(),
                    token_hash: *token_hash,
                    now_at,
                },
                health: params.health,
            })
            .await?;
        Ok(HeartbeatResult {
            received_at: now_at,
            registration_required: current
                .get(node_id)
                .is_none_or(|value| value.token_hash != *token_hash),
        })
    }

    pub async fn unregister(&self, principal: &NodePrincipal) -> TalosResult<()> {
        let NodePrincipal::Lease {
            node_id,
            token_hash,
        } = principal
        else {
            return Err(unauthenticated());
        };
        let mut current = self.current.lock().await;
        self.store
            .unregister(LeaseCheck {
                node_id: node_id.clone(),
                token_hash: *token_hash,
                now_at: now(),
            })
            .await?;
        let previous = if current
            .get(node_id)
            .is_some_and(|value| value.token_hash == *token_hash)
        {
            current.remove(node_id)
        } else {
            None
        };
        drop(current);
        revoke_registration(previous).await;
        Ok(())
    }

    async fn node_info(&self, record: NodeRecord) -> NodeInfo {
        let now_at = now();
        let remote_available = if record.mode == NodeMode::Remote
            && record.status == NodeStatus::Enabled
            && record.health == agent_protocol::NodeHealth::Healthy
            && record
                .lease_expires_at
                .is_some_and(|expiry| expiry > now_at)
            && agent_protocol::heartbeat_is_recent(record.last_heartbeat_at, now_at)
        {
            let current = self.current.lock().await;
            current.get(&record.node_id).is_some_and(|registration| {
                registration.revoke.is_some()
                    && record
                        .lease_token_hash
                        .is_some_and(|stored| bool::from(stored.ct_eq(&registration.token_hash)))
            })
        } else {
            false
        };
        let mut info = to_info(record);
        info.available = info.available || remote_available;
        info
    }

    async fn owned_node(&self, actor: &Actor, node_id: &str) -> TalosResult<NodeRecord> {
        ensure_management_actor(actor)?;
        let record = self.store.get(node_id).await?.ok_or_else(not_found)?;
        if record.mode != NodeMode::Remote
            || (actor.role != UserRole::Admin
                && record.owner_user_id.as_deref() != Some(actor.user_id.as_str()))
        {
            return Err(not_found());
        }
        Ok(record)
    }
}

#[derive(Clone, Copy)]
enum CredentialKind {
    Registration,
    Lease,
}

fn to_info(record: NodeRecord) -> NodeInfo {
    NodeInfo {
        node_id: record.node_id,
        display_name: record.display_name,
        mode: record.mode,
        endpoint: record.endpoint,
        status: record.status,
        health: record.health,
        lease_expires_at: record.lease_expires_at,
        last_heartbeat_at: record.last_heartbeat_at,
        created_at: record.created_at,
        updated_at: record.updated_at,
        owner_user_id: record.owner_user_id,
        available: record.mode == NodeMode::Embedded && record.status == NodeStatus::Enabled,
        credential_expires_at: record.credential_expires_at,
    }
}

fn validate_deployments(params: &RegisterParams) -> TalosResult<()> {
    let mut agent_ids = HashSet::new();
    if params.deployments.is_empty()
        || params.deployments.iter().any(|deployment| {
            deployment.agent_id.trim().is_empty() || !agent_ids.insert(&deployment.agent_id)
        })
    {
        return Err(TalosError::new(
            ErrorCode::InvalidRequest,
            "Agent node deployments must have distinct nonempty Agent IDs",
        ));
    }
    Ok(())
}

fn validate_display_name(name: &str) -> TalosResult<()> {
    if name.trim().is_empty() {
        return Err(TalosError::new(
            ErrorCode::InvalidRequest,
            "Agent node display name is required",
        ));
    }
    Ok(())
}

fn validate_endpoint(endpoint: &str) -> TalosResult<()> {
    let url = url::Url::parse(endpoint).map_err(|error| {
        TalosError::from_source(
            ErrorCode::InvalidRequest,
            "Agent node endpoint is invalid",
            error,
        )
    })?;
    if url.scheme() != "https"
        || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(TalosError::new(
            ErrorCode::InvalidRequest,
            "Agent node endpoint must be a TLS origin",
        ));
    }
    Ok(())
}

fn validate_expiry(expires_at: i64, now_at: i64) -> TalosResult<()> {
    if expires_at <= now_at {
        return Err(TalosError::new(
            ErrorCode::InvalidRequest,
            "Agent node credential expiry must be in the future",
        ));
    }
    Ok(())
}

fn ensure_management_actor(actor: &Actor) -> TalosResult<()> {
    if actor.auth_disabled || actor.user_id.trim().is_empty() {
        return Err(unauthenticated());
    }
    Ok(())
}

fn new_token() -> TalosResult<String> {
    let mut bytes = [0_u8; 32];
    getrandom::getrandom(&mut bytes).map_err(|error| {
        TalosError::internal_with_source(
            "Generate Agent node token; technical details were withheld.",
            error,
        )
    })?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
}

fn hash_token(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn unauthenticated() -> TalosError {
    TalosError::new(
        ErrorCode::Unauthenticated,
        "Agent node authentication failed",
    )
}

fn not_found() -> TalosError {
    TalosError::new(ErrorCode::ResourceNotFound, "Agent node not found")
}

async fn revoke_registration(registration: Option<CurrentRegistration>) {
    if let Some(hook) = registration.and_then(|value| value.revoke) {
        hook().await;
    }
}
