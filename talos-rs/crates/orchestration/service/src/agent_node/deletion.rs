use super::*;
use talos_types::{DeleteComputerRequest, DeleteComputerResponse};

impl AgentNodeService {
    /// Attach the idle-revocation operation to the current live publication.
    pub async fn set_registration_deletion_hook(
        &self,
        auth: &NodeAuth,
        hook: crate::agent_node::AgentNodeDeletionHook,
    ) -> TalosResult<()> {
        self.authenticate_lease(auth).await?;
        let mut current = self.current.lock().await;
        let registration = current.get_mut(&auth.node_id).ok_or_else(unauthenticated)?;
        if registration.token_hash != hash_token(&auth.token) {
            return Err(unauthenticated());
        }
        registration.prepare_deletion = Some(hook);
        Ok(())
    }

    /// Owner-only, retryable deletion. Holding the current registration lock prevents
    /// renewal/reinstallation while Worker admission is revoked and the Store commits.
    pub async fn delete_computer(
        &self,
        actor: &Actor,
        request: DeleteComputerRequest,
    ) -> TalosResult<DeleteComputerResponse> {
        let remote = self
            .remote_management
            .read()
            .expect("Computer management lock")
            .clone();
        if let Some(remote) = remote {
            return remote.delete(actor, request).await;
        }
        let service = self.clone();
        let actor = actor.clone();
        tokio::spawn(async move { service.delete_owned_registration(&actor, request).await })
            .await
            .map_err(|error| {
                TalosError::internal_with_source("Computer deletion task failed", error)
            })?
    }

    async fn delete_owned_registration(
        &self,
        actor: &Actor,
        request: DeleteComputerRequest,
    ) -> TalosResult<DeleteComputerResponse> {
        ensure_management_actor(actor)?;
        let _lifecycle = self.lock_lifecycle().await;
        let mut current = self.current.lock().await;
        let deleted_owner = self.store.deletion_owner(&request.node_id).await?;
        if deleted_owner.as_deref() != Some(&actor.user_id) {
            let node = self
                .store
                .get(&request.node_id)
                .await?
                .ok_or_else(not_found)?;
            if node.mode != NodeMode::Remote
                || node.owner_user_id.as_deref() != Some(&actor.user_id)
            {
                return Err(not_found());
            }
            let prepare = current
                .get(&request.node_id)
                .and_then(|registration| registration.prepare_deletion.clone());
            if let Some(prepare) = &prepare {
                if let Err(error) = prepare(/* blocked */ true).await {
                    // An expired, unreachable publication cannot admit remote work.
                    // Offline removal therefore does not require its source to reconnect.
                    let expired = node.lease_expires_at.is_none_or(|expires| expires <= now());
                    let unavailable = matches!(
                        error.error_code(),
                        ErrorCode::Unauthenticated
                            | ErrorCode::ResourceExpired
                            | ErrorCode::ServiceUnavailable
                            | ErrorCode::Timeout
                    );
                    if !(expired && unavailable) {
                        if let Err(cleanup) = prepare(/* blocked */ false).await {
                            talos_logging::report_error(&cleanup);
                        }
                        return Err(if unavailable {
                            TalosError::localized(
                                talos_error::ErrorLocalization::ComputerDeletionUnverified,
                            )
                            .with_source(error)
                        } else {
                            error
                        });
                    }
                }
            } else if node.lease_expires_at.is_some_and(|expires| expires > now()) {
                return Err(TalosError::localized(
                    talos_error::ErrorLocalization::ComputerDeletionUnverified,
                ));
            }
            if let Err(error) = self
                .store
                .delete_owned(&request.node_id, &actor.user_id, now())
                .await
            {
                if let Some(prepare) = prepare {
                    if let Err(cleanup) = prepare(/* blocked */ false).await {
                        talos_logging::report_error(&cleanup);
                    }
                }
                return Err(error);
            }
        }
        let previous = current.remove(&request.node_id);
        drop(current);
        revoke_registration(previous).await;
        Ok(DeleteComputerResponse {
            node_id: request.node_id,
        })
    }
}
