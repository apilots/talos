//! Source FileService transport through the same account-authenticated Gateway.
use agent_protocol::AgentFuture;
use std::sync::Arc;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_service::{file::FilePeerAccess, FileContent};
use talos_types::{
    FileDeleteRequest, FileDeleteResponse, FileReadRequest, FileUpdateRequest, FileUpdateResponse,
};

pub(crate) struct GatewayPeerAccess {
    pub config: talos_config::Config,
    pub stores: Arc<super::application_sync::ApplicationSyncStores>,
}
impl GatewayPeerAccess {
    async fn gateway(&self) -> TalosResult<agent_client::AgentNodeClient> {
        super::application_sync_computers::gateway_client(&self.config).await
    }
    async fn token(&self, owner: &str) -> TalosResult<String> {
        let callback = self
            .stores
            .credentials
            .read()
            .expect("Sync credentials lock")
            .get(owner)
            .cloned()
            .ok_or_else(|| {
                TalosError::new(
                    ErrorCode::Unauthenticated,
                    "Management account is not signed in.",
                )
            })?;
        let credential = callback().await.map_err(|error| {
            TalosError::from_source(
                match error.code {
                    talos_sync_types::SyncErrorCode::Unauthenticated => ErrorCode::Unauthenticated,
                    talos_sync_types::SyncErrorCode::PermissionDenied => {
                        ErrorCode::PermissionDenied
                    }
                    talos_sync_types::SyncErrorCode::Timeout => ErrorCode::Timeout,
                    talos_sync_types::SyncErrorCode::ServiceUnavailable => {
                        ErrorCode::ServiceUnavailable
                    }
                    _ => ErrorCode::InternalError,
                },
                "Management account credentials are unavailable.",
                error,
            )
        })?;
        if credential.account_id != owner {
            return Err(TalosError::new(
                ErrorCode::PermissionDenied,
                "Management account does not match.",
            ));
        }
        Ok(credential.access_token)
    }
}
impl FilePeerAccess for GatewayPeerAccess {
    fn read<'a>(
        &'a self,
        owner: &'a str,
        peer: &'a str,
        request: FileReadRequest,
        max_bytes: u64,
    ) -> AgentFuture<'a, TalosResult<FileContent>> {
        Box::pin(async move {
            let token = self.token(owner).await?;
            let (file, bytes) = self
                .gateway()
                .await?
                .read_peer_file(&token, peer, request, max_bytes)
                .await?;
            Ok(FileContent { file, bytes })
        })
    }
    fn update<'a>(
        &'a self,
        owner: &'a str,
        peer: &'a str,
        request: FileUpdateRequest,
    ) -> AgentFuture<'a, TalosResult<FileUpdateResponse>> {
        Box::pin(async move {
            self.gateway()
                .await?
                .update_peer_file(&self.token(owner).await?, peer, request)
                .await
        })
    }
    fn delete<'a>(
        &'a self,
        owner: &'a str,
        peer: &'a str,
        request: FileDeleteRequest,
    ) -> AgentFuture<'a, TalosResult<FileDeleteResponse>> {
        Box::pin(async move {
            self.gateway()
                .await?
                .delete_peer_file(&self.token(owner).await?, peer, request)
                .await
        })
    }
}

impl agent_protocol::ActiveSessionCommands for GatewayPeerAccess {
    fn invoke<'a>(
        &'a self,
        owner: &'a str,
        target: agent_protocol::ActiveSessionTarget,
        command: agent_protocol::ActiveSessionCommand,
    ) -> AgentFuture<'a, TalosResult<agent_protocol::ActiveSessionReply>> {
        Box::pin(async move {
            self.gateway()
                .await?
                .invoke_active_command(&self.token(owner).await?, target, command)
                .await
        })
    }
}

impl talos_service::ComputerManagement for GatewayPeerAccess {
    fn delete<'a>(
        &'a self,
        actor: &'a talos_service::Actor,
        request: talos_types::DeleteComputerRequest,
    ) -> AgentFuture<'a, TalosResult<talos_types::DeleteComputerResponse>> {
        Box::pin(async move {
            self.gateway()
                .await?
                .delete_computer(management_token(actor)?, request)
                .await
        })
    }

    fn create<'a>(
        &'a self,
        actor: &'a talos_service::Actor,
        request: talos_types::CreateNodeRequest,
    ) -> AgentFuture<'a, TalosResult<talos_types::CreatedNode>> {
        Box::pin(async move {
            self.gateway()
                .await?
                .create_node(management_token(actor)?, request)
                .await
        })
    }
    fn update<'a>(
        &'a self,
        actor: &'a talos_service::Actor,
        request: talos_types::UpdateNodeRequest,
    ) -> AgentFuture<'a, TalosResult<talos_types::NodeInfo>> {
        Box::pin(async move {
            self.gateway()
                .await?
                .update_node(
                    actor.access_token.as_deref().ok_or_else(|| {
                        TalosError::new(
                            ErrorCode::Unauthenticated,
                            "Sign in before managing Computers.",
                        )
                    })?,
                    request,
                )
                .await
        })
    }
    fn rename<'a>(
        &'a self,
        actor: &'a talos_service::Actor,
        request: talos_types::RenameComputerRequest,
    ) -> AgentFuture<'a, TalosResult<talos_types::NodeInfo>> {
        Box::pin(async move {
            self.gateway()
                .await?
                .rename_computer(management_token(actor)?, request)
                .await
        })
    }
    fn set_status<'a>(
        &'a self,
        actor: &'a talos_service::Actor,
        request: talos_types::SetComputerStatusRequest,
    ) -> AgentFuture<'a, TalosResult<talos_types::NodeInfo>> {
        Box::pin(async move {
            self.gateway()
                .await?
                .set_computer_status(management_token(actor)?, request)
                .await
        })
    }
    fn rotate_credential<'a>(
        &'a self,
        actor: &'a talos_service::Actor,
        request: talos_types::RotateCredentialRequest,
    ) -> AgentFuture<'a, TalosResult<talos_types::NodeCredential>> {
        Box::pin(async move {
            self.gateway()
                .await?
                .rotate_credential(management_token(actor)?, request)
                .await
        })
    }
}
fn management_token(actor: &talos_service::Actor) -> TalosResult<&str> {
    actor.access_token.as_deref().ok_or_else(|| {
        TalosError::new(
            ErrorCode::Unauthenticated,
            "Computer management requires a signed-in account.",
        )
    })
}
