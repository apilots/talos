//! Thin product capability adapter. Registration and managed state exist only in Server.
use super::ComputerManagement;
use crate::Actor;
use agent_protocol::NodeAdmissionGates;
use std::sync::{Arc, RwLock};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_types::*;

#[derive(Clone)]
pub struct ProductNodeAccess {
    kind: ProductNodes,
    admission: Arc<NodeAdmissionGates>,
}
#[derive(Clone)]
enum ProductNodes {
    Computer {
        embedded: Arc<NodeInfo>,
        management: Arc<RwLock<Option<Arc<dyn ComputerManagement>>>>,
    },
    #[cfg(feature = "control-plane")]
    Server(talos_control_plane::AgentNodeService),
}
pub(crate) use agent_protocol::ManagedDispatch;

pub(crate) struct ManagedConversationBinding<'a> {
    pub session_id: &'a str,
    pub project_id: Option<&'a str>,
    pub node_id: Option<&'a str>,
}

impl ProductNodeAccess {
    /// Assemble local product access without allocating a registration or managed lifecycle owner.
    pub fn computer(embedded: NodeInfo, admission: Arc<NodeAdmissionGates>) -> Self {
        Self {
            kind: ProductNodes::Computer {
                embedded: Arc::new(embedded),
                management: Default::default(),
            },
            admission,
        }
    }
    pub fn admission_gates(&self) -> Arc<NodeAdmissionGates> {
        self.admission.clone()
    }
    pub fn set_remote_management(&self, remote: Arc<dyn ComputerManagement>) {
        match &self.kind {
            ProductNodes::Computer { management, .. } => {
                *management.write().expect("Computer management lock") = Some(remote);
            }
            #[cfg(feature = "control-plane")]
            ProductNodes::Server(_) => {}
        }
    }
    fn management(&self) -> TalosResult<Arc<dyn ComputerManagement>> {
        match &self.kind {
            ProductNodes::Computer { management, .. } => management
                .read()
                .expect("Computer management lock")
                .clone()
                .ok_or_else(unavailable),
            #[cfg(feature = "control-plane")]
            ProductNodes::Server(_) => Err(unavailable()),
        }
    }
    #[cfg(feature = "control-plane")]
    pub fn server(&self) -> TalosResult<&talos_control_plane::AgentNodeService> {
        match &self.kind {
            ProductNodes::Server(server) => Ok(server),
            ProductNodes::Computer { .. } => Err(unavailable()),
        }
    }
    pub async fn list_computers_excluding(
        &self,
        actor: &Actor,
        request: ComputerListRequest,
        excluded: Option<String>,
    ) -> TalosResult<ComputerListResponse> {
        let _ = &excluded;
        match &self.kind {
            #[cfg(feature = "control-plane")]
            ProductNodes::Server(server) => {
                server
                    .list_computers_excluding(actor, request, excluded)
                    .await
            }
            ProductNodes::Computer { embedded, .. } => {
                if !actor.auth_disabled {
                    return self.management()?.list(actor, request).await;
                }
                let matches = request.query.as_deref().is_none_or(|q| {
                    embedded
                        .display_name
                        .to_lowercase()
                        .contains(&q.trim().to_lowercase())
                });
                let data = if request
                    .cursor
                    .as_deref()
                    .is_none_or(|cursor| cursor < embedded.node_id.as_str())
                    && matches
                {
                    vec![embedded.as_ref().clone()]
                } else {
                    vec![]
                };
                Ok(ComputerListResponse {
                    total: u64::from(matches),
                    data,
                    next_cursor: None,
                })
            }
        }
    }
    pub async fn list_computers(
        &self,
        actor: &Actor,
        request: ComputerListRequest,
    ) -> TalosResult<ComputerListResponse> {
        self.list_computers_excluding(actor, request, None).await
    }
    pub async fn read_computer(
        &self,
        actor: &Actor,
        request: GetNodeRequest,
    ) -> TalosResult<NodeInfo> {
        match &self.kind {
            #[cfg(feature = "control-plane")]
            ProductNodes::Server(server) => server.read_computer(actor, request).await,
            ProductNodes::Computer { embedded, .. } => {
                if request.node_id == "embedded" {
                    if !actor.auth_disabled && actor.user_id.trim().is_empty() {
                        return Err(TalosError::new(
                            ErrorCode::Unauthenticated,
                            "Computer access requires an authenticated actor.",
                        ));
                    }
                    return Ok(embedded.as_ref().clone());
                }
                if actor.auth_disabled {
                    return Err(denied());
                }
                self.management()?.read(actor, request).await
            }
        }
    }
    pub async fn get_node(&self, actor: &Actor, request: GetNodeRequest) -> TalosResult<NodeInfo> {
        #[cfg(feature = "control-plane")]
        if let ProductNodes::Server(server) = &self.kind {
            return server.get_node(actor, request).await;
        }
        self.read_computer(actor, request).await
    }
    pub async fn create_node(
        &self,
        actor: &Actor,
        request: CreateNodeRequest,
    ) -> TalosResult<CreatedNode> {
        #[cfg(feature = "control-plane")]
        if let ProductNodes::Server(server) = &self.kind {
            return server.create_node(actor, request).await;
        }
        self.management()?.create(actor, request).await
    }
    pub async fn update_node(
        &self,
        actor: &Actor,
        request: UpdateNodeRequest,
    ) -> TalosResult<NodeInfo> {
        #[cfg(feature = "control-plane")]
        if let ProductNodes::Server(server) = &self.kind {
            return server.update_node(actor, request).await;
        }
        self.management()?.update(actor, request).await
    }
    pub async fn rename_computer(
        &self,
        actor: &Actor,
        request: RenameComputerRequest,
    ) -> TalosResult<NodeInfo> {
        #[cfg(feature = "control-plane")]
        if let ProductNodes::Server(server) = &self.kind {
            return server.rename_computer(actor, request).await;
        }
        self.management()?.rename(actor, request).await
    }
    pub async fn set_computer_status(
        &self,
        actor: &Actor,
        request: SetComputerStatusRequest,
    ) -> TalosResult<NodeInfo> {
        #[cfg(feature = "control-plane")]
        if let ProductNodes::Server(server) = &self.kind {
            return server.set_computer_status(actor, request).await;
        }
        self.management()?.set_status(actor, request).await
    }
    pub async fn rotate_credential(
        &self,
        actor: &Actor,
        request: RotateCredentialRequest,
    ) -> TalosResult<NodeCredential> {
        #[cfg(feature = "control-plane")]
        if let ProductNodes::Server(server) = &self.kind {
            return server.rotate_credential(actor, request).await;
        }
        self.management()?.rotate_credential(actor, request).await
    }
    pub async fn delete_computer(
        &self,
        actor: &Actor,
        request: DeleteComputerRequest,
    ) -> TalosResult<DeleteComputerResponse> {
        #[cfg(feature = "control-plane")]
        if let ProductNodes::Server(server) = &self.kind {
            return server.delete_computer(actor, request).await;
        }
        self.management()?.delete(actor, request).await
    }
    pub async fn product_owner(
        &self,
        actor: &Actor,
        node_id: &str,
    ) -> TalosResult<Option<NodeProductOwner>> {
        #[cfg(feature = "control-plane")]
        if let ProductNodes::Server(server) = &self.kind {
            return server.product_owner(actor, node_id).await;
        }
        let _ = (actor, node_id);
        Err(denied())
    }
    pub async fn ensure_execution_product_owner(
        &self,
        node_id: &str,
        user_id: &str,
        expected: NodeProductOwner,
    ) -> TalosResult<()> {
        #[cfg(feature = "control-plane")]
        if let ProductNodes::Server(server) = &self.kind {
            return server
                .ensure_execution_product_owner(node_id, user_id, expected)
                .await;
        }
        let _ = (user_id, expected);
        local_only(node_id)
    }
    pub async fn execution_node_permanently_removed(
        &self,
        node_id: &str,
        user_id: &str,
    ) -> TalosResult<bool> {
        #[cfg(feature = "control-plane")]
        if let ProductNodes::Server(server) = &self.kind {
            return server
                .execution_node_permanently_removed(node_id, user_id)
                .await;
        }
        let _ = (node_id, user_id);
        Ok(false)
    }
    pub(crate) async fn managed_conversation_node(
        &self,
        actor: &Actor,
        request: ManagedConversationBinding<'_>,
    ) -> TalosResult<Option<String>> {
        #[cfg(feature = "control-plane")]
        if let ProductNodes::Server(server) = &self.kind {
            return server
                .managed_conversation_node(
                    actor,
                    talos_control_plane::ManagedConversationBinding {
                        session_id: request.session_id,
                        project_id: request.project_id,
                        node_id: request.node_id,
                    },
                )
                .await;
        }
        let _ = (
            actor,
            request.session_id,
            request.project_id,
            request.node_id,
        );
        Ok(None)
    }
    pub fn managed_worker_enabled(&self) -> bool {
        #[cfg(feature = "control-plane")]
        if let ProductNodes::Server(server) = &self.kind {
            return server.managed_worker_enabled();
        }
        false
    }
    pub async fn ensure_managed_node_access(
        &self,
        actor: &Actor,
        node_id: &str,
    ) -> TalosResult<()> {
        #[cfg(feature = "control-plane")]
        if let ProductNodes::Server(server) = &self.kind {
            return server.ensure_managed_node_access(actor, node_id).await;
        }
        let _ = actor;
        local_only(node_id)
    }
    pub async fn authorize_session_dispatch(
        &self,
        owner: &str,
        node_id: &str,
    ) -> TalosResult<SessionKind> {
        #[cfg(feature = "control-plane")]
        if let ProductNodes::Server(server) = &self.kind {
            return server.authorize_session_dispatch(owner, node_id).await;
        }
        let _ = owner;
        local_only(node_id)?;
        Ok(SessionKind::Standard)
    }
    pub async fn is_managed_worker_node(&self, node_id: &str) -> TalosResult<bool> {
        #[cfg(feature = "control-plane")]
        if let ProductNodes::Server(server) = &self.kind {
            return server.is_managed_worker_node(node_id).await;
        }
        let _ = node_id;
        Ok(false)
    }
    pub async fn managed_request_wait(&self, node_id: &str) -> TalosResult<std::time::Duration> {
        #[cfg(feature = "control-plane")]
        if let ProductNodes::Server(server) = &self.kind {
            return server.managed_request_wait(node_id).await;
        }
        local_only(node_id)?;
        Ok(std::time::Duration::ZERO)
    }
    pub async fn acquire_managed_worker(
        &self,
        actor: &Actor,
        node_id: &str,
    ) -> TalosResult<Option<tokio::sync::OwnedRwLockReadGuard<()>>> {
        #[cfg(feature = "control-plane")]
        if let ProductNodes::Server(server) = &self.kind {
            return server.acquire_managed_worker(actor, node_id).await;
        }
        let _ = actor;
        local_only(node_id)?;
        Ok(None)
    }
    /// Acquire the existing lifecycle permit for already qualified background maintenance.
    pub async fn acquire_managed_dispatch(
        &self,
        owner: &str,
        node_id: &str,
    ) -> TalosResult<Option<tokio::sync::OwnedRwLockReadGuard<()>>> {
        #[cfg(feature = "control-plane")]
        if let ProductNodes::Server(server) = &self.kind {
            return server.acquire_managed_dispatch(owner, node_id).await;
        }
        let _ = owner;
        local_only(node_id)?;
        Ok(None)
    }
    pub async fn wait_managed_dispatch(&self, owner: &str, node_id: &str) -> TalosResult<()> {
        #[cfg(feature = "control-plane")]
        if let ProductNodes::Server(server) = &self.kind {
            return server.wait_managed_dispatch(owner, node_id).await;
        }
        let _ = owner;
        local_only(node_id)
    }
    pub async fn record_managed_activity(&self, node_id: &str) -> TalosResult<()> {
        #[cfg(feature = "control-plane")]
        if let ProductNodes::Server(server) = &self.kind {
            return server.record_managed_activity(node_id).await;
        }
        local_only(node_id)
    }
    pub(crate) async fn wake_for_managed_dispatch(
        &self,
        owner: &str,
        node_id: &str,
    ) -> TalosResult<ManagedDispatch> {
        #[cfg(feature = "control-plane")]
        if let ProductNodes::Server(server) = &self.kind {
            return server.wake_for_managed_dispatch(owner, node_id).await;
        }
        let _ = owner;
        local_only(node_id)?;
        Ok(ManagedDispatch::Proceed)
    }
    pub async fn read_managed_worker(&self, actor: &Actor) -> TalosResult<ManagedWorkerResponse> {
        #[cfg(feature = "control-plane")]
        if let ProductNodes::Server(server) = &self.kind {
            return server.read_managed_worker(actor).await;
        }
        talos_auth::ensure_managed_worker_access(actor)?;
        Ok(ManagedWorkerResponse {
            enabled: false,
            instance: None,
        })
    }
    pub async fn create_managed_worker(&self, actor: &Actor) -> TalosResult<ManagedWorkerInfo> {
        #[cfg(feature = "control-plane")]
        if let ProductNodes::Server(server) = &self.kind {
            return server.create_managed_worker(actor).await;
        }
        let _ = actor;
        Err(unavailable())
    }
    pub async fn start_managed_worker(
        &self,
        actor: &Actor,
        request: ManagedWorkerActionRequest,
    ) -> TalosResult<ManagedWorkerInfo> {
        #[cfg(feature = "control-plane")]
        if let ProductNodes::Server(server) = &self.kind {
            return server.start_managed_worker(actor, request).await;
        }
        let _ = (actor, request);
        Err(unavailable())
    }
    pub async fn stop_managed_worker(
        &self,
        actor: &Actor,
        request: ManagedWorkerActionRequest,
    ) -> TalosResult<ManagedWorkerInfo> {
        #[cfg(feature = "control-plane")]
        if let ProductNodes::Server(server) = &self.kind {
            return server.stop_managed_worker(actor, request).await;
        }
        let _ = (actor, request);
        Err(unavailable())
    }
    pub async fn reset_managed_worker(
        &self,
        actor: &Actor,
        request: ManagedWorkerActionRequest,
    ) -> TalosResult<ManagedWorkerInfo> {
        #[cfg(feature = "control-plane")]
        if let ProductNodes::Server(server) = &self.kind {
            return server.reset_managed_worker(actor, request).await;
        }
        let _ = (actor, request);
        Err(unavailable())
    }
    pub async fn delete_managed_worker(
        &self,
        actor: &Actor,
        request: DeleteManagedWorkerRequest,
    ) -> TalosResult<ManagedWorkerInfo> {
        #[cfg(feature = "control-plane")]
        if let ProductNodes::Server(server) = &self.kind {
            return server.delete_managed_worker(actor, request).await;
        }
        let _ = (actor, request);
        Err(unavailable())
    }
}
#[cfg(feature = "control-plane")]
impl From<talos_control_plane::AgentNodeService> for ProductNodeAccess {
    fn from(server: talos_control_plane::AgentNodeService) -> Self {
        Self {
            admission: server.admission_gates(),
            kind: ProductNodes::Server(server),
        }
    }
}
fn unavailable() -> TalosError {
    TalosError::new(
        ErrorCode::ServiceUnavailable,
        "This operation requires the Talos Server control plane.",
    )
}
fn denied() -> TalosError {
    TalosError::new(
        ErrorCode::PermissionDenied,
        "Personal products operate only their embedded Worker.",
    )
}
fn local_only(node_id: &str) -> TalosResult<()> {
    if node_id == "embedded" {
        Ok(())
    } else {
        Err(denied())
    }
}
