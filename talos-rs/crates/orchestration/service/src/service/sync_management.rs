//! Management-host product controls; business ownership remains in the original Services.
use crate::Actor;
use agent_protocol::AgentFuture;
use std::sync::{Arc, Weak};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_types::{SyncEnableRequest, SyncStatusResponse};

/// Installed by the management host. Pure Worker processes do not implement this port.
/// Credentials are verified by the original AuthService, never by sync metadata.
pub trait ManagementSync: Send + Sync {
    /// Resume the account's owned lifecycle after startup/login authentication.
    /// Snapshot reads must not initialize or repair remote resources.
    fn resume(&self, actor: &Actor);
    /// Explicit login may reuse a credential that was suspended on this host.
    fn resume_login(&self, actor: &Actor);
    fn metrics<'a>(
        &'a self,
        actor: &'a Actor,
    ) -> AgentFuture<'a, TalosResult<talos_types::SyncMetricsResponse>>;
    fn status<'a>(&'a self, actor: &'a Actor) -> AgentFuture<'a, TalosResult<SyncStatusResponse>>;
    fn enable<'a>(
        &'a self,
        actor: &'a Actor,
        request: SyncEnableRequest,
    ) -> AgentFuture<'a, TalosResult<SyncStatusResponse>>;
    fn disable<'a>(&'a self, actor: &'a Actor) -> AgentFuture<'a, TalosResult<SyncStatusResponse>>;
    fn suspend<'a>(&'a self, actor: &'a Actor) -> AgentFuture<'a, TalosResult<()>>;
    fn connect_computer<'a>(
        &'a self,
        actor: &'a Actor,
        computer_id: &'a str,
    ) -> AgentFuture<'a, TalosResult<String>>;
    fn local_computer<'a>(
        &'a self,
        actor: &'a Actor,
    ) -> AgentFuture<'a, TalosResult<Option<String>>>;
    fn resolve_node<'a>(
        &'a self,
        actor: &'a Actor,
        computer_id: &'a str,
    ) -> AgentFuture<'a, TalosResult<String>>;
    fn computer_info<'a>(
        &'a self,
        actor: &'a Actor,
        info: talos_types::NodeInfo,
    ) -> AgentFuture<'a, TalosResult<talos_types::NodeInfo>>;
}

impl super::TalosService {
    /// Authentication/startup entry points resume the host's account lifecycle.
    /// The host verifies candidate credentials before using remote resources.
    pub fn resume_account_sync(&self, actor: &Actor) {
        if let Some(host) = self
            .sync_management
            .read()
            .expect("sync management lock")
            .as_ref()
            .and_then(Weak::upgrade)
        {
            host.resume(actor);
        }
    }
    pub fn resume_account_sync_after_login(&self, actor: &Actor) {
        if let Some(host) = self
            .sync_management
            .read()
            .expect("sync management lock")
            .as_ref()
            .and_then(Weak::upgrade)
        {
            host.resume_login(actor);
        }
    }
    /// A weak port prevents the host's resource callbacks retaining their own Service.
    pub fn set_sync_management(&self, host: Weak<dyn ManagementSync>) {
        *self.sync_management.write().expect("sync management lock") = Some(host);
    }
    fn sync_host(&self) -> TalosResult<Arc<dyn ManagementSync>> {
        self.sync_management
            .read()
            .expect("sync management lock")
            .as_ref()
            .and_then(Weak::upgrade)
            .ok_or_else(|| {
                TalosError::new(
                    ErrorCode::ServiceUnavailable,
                    "Management synchronization is unavailable.",
                )
            })
    }
    /// The host verifies explicit local consent before calling this original Store operation.
    pub async fn claim_local_history(&self, user_id: &str) -> TalosResult<()> {
        let _guard = self.execution_gate.for_node("embedded").write_owned().await;
        self.sessions.claim_local_history(user_id).await
    }
    pub(crate) async fn session_for_client(
        &self,
        actor: &Actor,
        mut info: talos_types::SessionInfo,
    ) -> TalosResult<talos_types::SessionInfo> {
        if info.node_id == "embedded" {
            let host = self
                .sync_management
                .read()
                .expect("sync management lock")
                .as_ref()
                .and_then(Weak::upgrade);
            if let Some(host) = host {
                if let Some(local) = host.local_computer(actor).await? {
                    info.node_id = local;
                }
            }
        }
        Ok(info)
    }
    pub(crate) async fn project_for_client(
        &self,
        actor: &Actor,
        mut info: talos_types::ProjectInfo,
    ) -> TalosResult<talos_types::ProjectInfo> {
        if info.node_id == "embedded" {
            let host = self
                .sync_management
                .read()
                .expect("sync management lock")
                .as_ref()
                .and_then(Weak::upgrade);
            if let Some(host) = host {
                if let Some(local) = host.local_computer(actor).await? {
                    info.node_id = local;
                }
            }
        }
        Ok(info)
    }
    pub async fn sync_metrics(
        &self,
        actor: &Actor,
    ) -> TalosResult<talos_types::SyncMetricsResponse> {
        self.sync_host()?.metrics(actor).await
    }
    pub async fn sync_status(&self, actor: &Actor) -> TalosResult<SyncStatusResponse> {
        self.sync_host()?.status(actor).await
    }
    pub async fn enable_sync(
        &self,
        actor: &Actor,
        request: SyncEnableRequest,
    ) -> TalosResult<SyncStatusResponse> {
        self.sync_host()?.enable(actor, request).await
    }
    pub async fn disable_sync(&self, actor: &Actor) -> TalosResult<SyncStatusResponse> {
        self.sync_host()?.disable(actor).await
    }
    pub async fn list_computers(
        &self,
        actor: &Actor,
        request: talos_types::ComputerListRequest,
    ) -> TalosResult<talos_types::ComputerListResponse> {
        let host = self
            .sync_management
            .read()
            .expect("sync management lock")
            .as_ref()
            .and_then(Weak::upgrade);
        let excluded = match &host {
            Some(host) => host.local_computer(actor).await?,
            None => None,
        };
        let mut page = self
            .nodes
            .list_computers_excluding(actor, request, excluded)
            .await?;
        if let Some(host) = host {
            let mut data = Vec::new();
            for info in page.data {
                match host.computer_info(actor, info).await {
                    Ok(info) => data.push(info),
                    Err(error) if error.is_code(talos_error::ErrorCode::ResourceExpired) => {
                        page.total = page.total.saturating_sub(1);
                    }
                    Err(error) => return Err(error),
                }
            }
            page.data = data;
        }
        Ok(page)
    }
    pub(crate) async fn resolve_computer_node(
        &self,
        actor: &Actor,
        computer_id: &str,
    ) -> TalosResult<String> {
        let host = self
            .sync_management
            .read()
            .expect("sync management lock")
            .as_ref()
            .and_then(Weak::upgrade);
        match host {
            Some(host) => host.resolve_node(actor, computer_id).await,
            None => Ok(computer_id.into()),
        }
    }
    /// Resolve only the host-local alias for retained metadata. Ownership is
    /// checked by the resource Store; offline/removed Workers need no connection.
    pub(crate) async fn resolve_computer_metadata_node(
        &self,
        actor: &Actor,
        computer_id: &str,
    ) -> TalosResult<String> {
        let host = self
            .sync_management
            .read()
            .expect("sync management lock")
            .as_ref()
            .and_then(Weak::upgrade);
        // Directory inspection must work offline and never opens an execution connection.
        let local = match &host {
            Some(host) => host.local_computer(actor).await?,
            None => None,
        };
        if computer_id == "embedded" && host.is_some() && local.is_none() && !actor.auth_disabled {
            return Err(TalosError::new(
                ErrorCode::PermissionDenied,
                "This account does not own the management host's Computer.",
            ));
        }
        Ok(if local.as_deref() == Some(computer_id) {
            "embedded".into()
        } else {
            computer_id.into()
        })
    }
    pub async fn read_computer(
        &self,
        actor: &Actor,
        request: talos_types::GetNodeRequest,
    ) -> TalosResult<talos_types::NodeInfo> {
        let node_id = self
            .resolve_computer_metadata_node(actor, &request.node_id)
            .await?;
        let info = self
            .nodes
            .read_computer(actor, talos_types::GetNodeRequest { node_id })
            .await?;
        let host = self
            .sync_management
            .read()
            .expect("sync management lock")
            .as_ref()
            .and_then(Weak::upgrade);
        match host {
            Some(host) => host.computer_info(actor, info).await,
            None => Ok(info),
        }
    }
    pub async fn suspend_account_sync(&self, actor: &Actor) -> TalosResult<()> {
        let host = self
            .sync_management
            .read()
            .expect("sync management lock")
            .as_ref()
            .and_then(Weak::upgrade);
        if let Some(host) = host {
            host.suspend(actor).await?;
        }
        Ok(())
    }
    pub(crate) async fn connect_selected_computer(
        &self,
        actor: &Actor,
        computer_id: &str,
    ) -> TalosResult<String> {
        let host = self
            .sync_management
            .read()
            .expect("sync management lock")
            .as_ref()
            .and_then(Weak::upgrade);
        if let Some(host) = host {
            return host.connect_computer(actor, computer_id).await;
        }
        Ok(computer_id.into())
    }
}
