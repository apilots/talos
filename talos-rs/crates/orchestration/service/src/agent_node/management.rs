//! Authenticated node provisioning and directory mutations, sharing registration revocation.

use super::*;

impl AgentNodeService {
    pub async fn create_node(
        &self,
        actor: &Actor,
        request: CreateNodeRequest,
    ) -> TalosResult<CreatedNode> {
        let remote = self
            .remote_management
            .read()
            .expect("Computer management lock")
            .clone();
        if let Some(remote) = remote {
            return remote.create(actor, request).await;
        }
        ensure_management_actor(actor)?;
        let node_id = uuid::Uuid::new_v4().to_string();
        validate_display_name(&request.display_name)?;
        if let Some(endpoint) = &request.endpoint {
            validate_endpoint(endpoint)?;
        }
        let now_at = now();
        validate_expiry(request.credential_expires_at, now_at)?;
        let owner_user_id = match request.owner_user_id {
            Some(owner) if owner != actor.user_id && actor.role != UserRole::Admin => {
                return Err(TalosError::new(
                    ErrorCode::PermissionDenied,
                    "Only an administrator may create another user's Agent node",
                ));
            }
            Some(owner) => owner,
            None => actor.user_id.clone(),
        };
        if owner_user_id.trim().is_empty() {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Agent node owner is required",
            ));
        }
        let token = new_token()?;
        let record = self
            .store
            .create(CreateNodeParams {
                node_id: node_id.clone(),
                owner_user_id: Some(owner_user_id),
                display_name: request.display_name,
                mode: NodeMode::Remote,
                endpoint: request.endpoint,
                credential_hash: Some(hash_token(&token)),
                credential_expires_at: Some(request.credential_expires_at),
                now_at,
            })
            .await?;
        self.observe_account(actor);
        Ok(CreatedNode {
            node: to_info(record),
            credential: NodeCredential {
                node_id,
                token,
                expires_at: request.credential_expires_at,
            },
        })
    }

    pub async fn get_node(&self, actor: &Actor, request: GetNodeRequest) -> TalosResult<NodeInfo> {
        let record = if actor.role == UserRole::Admin {
            ensure_management_actor(actor)?;
            self.store
                .get(&request.node_id)
                .await?
                .ok_or_else(not_found)?
        } else {
            self.owned_node(actor, &request.node_id).await?
        };
        self.observe_account(actor);
        Ok(self.node_info(record).await)
    }

    pub async fn list_nodes(
        &self,
        actor: &Actor,
        request: ListNodesRequest,
    ) -> TalosResult<NodePage> {
        ensure_management_actor(actor)?;
        let scope = if actor.role == UserRole::Admin {
            NodeListScope::All
        } else {
            NodeListScope::Owner {
                user_id: actor.user_id.clone(),
                include_embedded: false,
            }
        };
        let page = self
            .store
            .list(ListNodesParams {
                exclude_node_id: None,
                scope,
                query: None,
                cursor: request.cursor,
                limit: if request.limit == 0 {
                    50
                } else {
                    request.limit
                },
            })
            .await?;
        let mut data = Vec::with_capacity(page.data.len());
        for record in page.data {
            data.push(self.node_info(record).await);
        }
        Ok(NodePage {
            data,
            next_cursor: page.next_cursor,
        })
    }

    pub async fn update_node(
        &self,
        actor: &Actor,
        request: UpdateNodeRequest,
    ) -> TalosResult<NodeInfo> {
        let remote = self
            .remote_management
            .read()
            .expect("Computer management lock")
            .clone();
        if let Some(remote) = remote {
            return remote.update(actor, request).await;
        }
        validate_display_name(&request.display_name)?;
        if let Some(endpoint) = &request.endpoint {
            validate_endpoint(endpoint)?;
        }
        let _lifecycle = self.lock_lifecycle().await;
        let mut current = self.current.lock().await;
        let old = self.owned_node(actor, &request.node_id).await?;
        let revoked = old.endpoint != request.endpoint || request.status == NodeStatus::Disabled;
        let record = self
            .store
            .update(UpdateNodeParams {
                node_id: request.node_id.clone(),
                display_name: request.display_name,
                endpoint: request.endpoint,
                status: request.status,
                now_at: now(),
            })
            .await?;
        let previous = if revoked {
            current.remove(&request.node_id)
        } else {
            None
        };
        drop(current);
        revoke_registration(previous).await;
        Ok(self.node_info(record).await)
    }

    pub async fn rotate_credential(
        &self,
        actor: &Actor,
        request: RotateCredentialRequest,
    ) -> TalosResult<NodeCredential> {
        let remote = self
            .remote_management
            .read()
            .expect("Computer management lock")
            .clone();
        if let Some(remote) = remote {
            return remote.rotate_credential(actor, request).await;
        }

        let _lifecycle = self.lock_lifecycle().await;
        let mut current = self.current.lock().await;
        self.owned_node(actor, &request.node_id).await?;
        validate_expiry(request.expires_at, now())?;
        let token = new_token()?;
        self.store
            .rotate_credential(RotateCredentialParams {
                node_id: request.node_id.clone(),
                hash: hash_token(&token),
                expires_at: request.expires_at,
                now_at: now(),
            })
            .await?;
        let previous = current.remove(&request.node_id);
        drop(current);
        revoke_registration(previous).await;
        Ok(NodeCredential {
            node_id: request.node_id,
            token,
            expires_at: request.expires_at,
        })
    }
}
