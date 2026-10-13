use talos_store::node::{ListNodesParams, NodeListScope, RenameNodeParams, SetNodeStatusParams};
use talos_types::{
    ComputerListRequest, ComputerListResponse, GetNodeRequest, NodeInfo, NodeMode, NodeStatus,
    RenameComputerRequest, SetComputerStatusRequest,
};

use super::{
    ensure_management_actor, not_found, now, revoke_registration, validate_display_name,
    AgentNodeService,
};
use crate::Actor;
use talos_error::TalosResult;

impl AgentNodeService {
    /// Lists the caller's Computers and the existing embedded business entry.
    /// Administrative directory access remains separate from execution selection.
    pub async fn list_computers(
        &self,
        actor: &Actor,
        request: ComputerListRequest,
    ) -> TalosResult<ComputerListResponse> {
        self.list_computers_excluding(actor, request, None).await
    }
    pub(crate) async fn list_computers_excluding(
        &self,
        actor: &Actor,
        request: ComputerListRequest,
        excluded: Option<String>,
    ) -> TalosResult<ComputerListResponse> {
        if !actor.auth_disabled {
            ensure_management_actor(actor)?;
        }
        // Trusted local unauthenticated access never exposes personally registered nodes.
        let scope = if actor.auth_disabled {
            NodeListScope::Embedded
        } else {
            NodeListScope::Owner {
                user_id: actor.user_id.clone(),
                // G has no shared personal Computer. The private Worker is shown only
                // after the host has mapped its registered UUID for this owner.
                include_embedded: excluded.is_some() || actor.role == talos_auth::UserRole::Admin,
            }
        };
        let query = request
            .query
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());
        let page = self
            .store
            .list(ListNodesParams {
                exclude_node_id: excluded.filter(|id| id != "embedded"),
                scope,
                query,
                cursor: request.cursor,
                limit: request.limit.unwrap_or(50).clamp(1, 200),
            })
            .await?;
        let mut data = Vec::with_capacity(page.data.len());
        for record in page.data {
            data.push(self.node_info(record).await);
        }
        Ok(ComputerListResponse {
            data,
            next_cursor: page.next_cursor,
            total: page.total,
        })
    }

    /// Reads the same scope as the selector, including disabled and offline Computers.
    /// Reading embedded business metadata does not grant instance management permission.
    pub async fn read_computer(
        &self,
        actor: &Actor,
        request: GetNodeRequest,
    ) -> TalosResult<NodeInfo> {
        if !actor.auth_disabled {
            ensure_management_actor(actor)?;
        }
        let record = self
            .store
            .get(&request.node_id)
            .await?
            .ok_or_else(not_found)?;
        if record.mode != NodeMode::Embedded
            && (actor.auth_disabled
                || record.owner_user_id.as_deref() != Some(actor.user_id.as_str()))
        {
            return Err(not_found());
        }
        Ok(self.node_info(record).await)
    }

    /// Changes only the name, preserving concurrent status, endpoint and lease updates.
    pub async fn rename_computer(
        &self,
        actor: &Actor,
        request: RenameComputerRequest,
    ) -> TalosResult<NodeInfo> {
        let remote = self
            .remote_management
            .read()
            .expect("Computer management lock")
            .clone();
        if let Some(remote) = remote {
            return remote.rename(actor, request).await;
        }
        validate_display_name(&request.display_name)?;
        self.owned_node(actor, &request.node_id).await?;
        let record = self
            .store
            .rename(RenameNodeParams {
                node_id: request.node_id,
                display_name: request.display_name,
                now_at: now(),
            })
            .await?;
        Ok(self.node_info(record).await)
    }

    /// Changes status without replacing mutable directory fields. Disabling invalidates the lease.
    pub async fn set_computer_status(
        &self,
        actor: &Actor,
        request: SetComputerStatusRequest,
    ) -> TalosResult<NodeInfo> {
        let remote = self
            .remote_management
            .read()
            .expect("Computer management lock")
            .clone();
        if let Some(remote) = remote {
            return remote.set_status(actor, request).await;
        }
        let _lifecycle = self.lock_lifecycle().await;
        let mut current = self.current.lock().await;
        self.owned_node(actor, &request.node_id).await?;
        let record = self
            .store
            .set_status(SetNodeStatusParams {
                node_id: request.node_id.clone(),
                status: request.status,
                now_at: now(),
            })
            .await?;
        let previous = if request.status == NodeStatus::Disabled {
            current.remove(&request.node_id)
        } else {
            None
        };
        drop(current);
        revoke_registration(previous).await;
        Ok(self.node_info(record).await)
    }
}
