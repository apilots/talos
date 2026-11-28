//! Source FileService transport through the same account-authenticated Gateway.
use agent_protocol::AgentFuture;
use std::time::Instant;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_service::{file::FilePeerAccess, FileContent};
use talos_types::{
    FileDeleteRequest, FileDeleteResponse, FileReadRequest, FileUpdateRequest, FileUpdateResponse,
};

pub(crate) struct GatewayPeerAccess {
    pub host: std::sync::Weak<super::application_host::ApplicationHost>,
    pub config: talos_config::Config,
}
impl GatewayPeerAccess {
    async fn source_client(
        &self,
        owner: &str,
        peer: &str,
    ) -> TalosResult<talos_sdk::AppServerClient> {
        let ca = talos_service::read_worker_trusted_ca(
            self.config.cloud.agent_trusted_ca_file.as_deref(),
        )?;
        talos_sdk::AppServerClient::connect_source_product_http(
            &self.config.cloud.agent_base_url,
            self.token(owner).await?,
            peer.into(),
            ca.as_deref(),
        )
    }

    async fn gateway(&self) -> TalosResult<agent_client::AgentNodeClient> {
        super::application_sync_computers::gateway_client(&self.config).await
    }
    async fn confirmed_computer(
        &self,
        actor: &talos_service::Actor,
        id: &str,
        info: talos_types::NodeInfo,
        started: Instant,
    ) -> TalosResult<talos_types::NodeInfo> {
        if info.node_id != id {
            return Err(TalosError::authorization_denied());
        }
        // Authorized administrator mutations may target another owner. Their
        // results must not enter this signed-in account's selector cache.
        if info.owner_user_id.as_deref() != Some(&actor.user_id) {
            return if actor.role == talos_auth::UserRole::Admin {
                Ok(info)
            } else {
                Err(TalosError::authorization_denied())
            };
        }
        let host = self.host.upgrade().ok_or_else(|| {
            TalosError::new(
                ErrorCode::ServiceUnavailable,
                "Computer directory host has stopped.",
            )
        })?;
        host.cache_confirmed_computer(actor, id, &info, started)
            .await
    }
    async fn token(&self, owner: &str) -> TalosResult<String> {
        let host = self.host.upgrade().ok_or_else(|| {
            TalosError::new(
                ErrorCode::ServiceUnavailable,
                "Management host has stopped.",
            )
        })?;
        host.token_for_owner(owner)
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
            use talos_sdk::GatewayClient;
            let mut source = self.source_client(owner, peer).await?;
            let metadata = source.read_file(request.clone()).await?;
            if metadata.file.size_bytes > max_bytes {
                return Err(TalosError::new(
                    ErrorCode::InvalidRequest,
                    "File source exceeds the requested byte limit.",
                ));
            }
            let content = source.read_file_content(request).await?;
            if content.data.len() as u64 > max_bytes {
                return Err(TalosError::internal(
                    "File source exceeded its declared size.",
                ));
            }
            Ok(FileContent {
                file: content.file,
                bytes: content.data,
            })
        })
    }
    fn read_range<'a>(
        &'a self,
        owner: &'a str,
        peer: &'a str,
        request: FileReadRequest,
        range: std::ops::Range<u64>,
    ) -> AgentFuture<'a, TalosResult<FileContent>> {
        Box::pin(async move {
            let content = self
                .source_client(owner, peer)
                .await?
                .read_product_file_range(&request.file_id, range)
                .await?;
            Ok(FileContent {
                file: content.file,
                bytes: content.data,
            })
        })
    }
    fn update<'a>(
        &'a self,
        owner: &'a str,
        peer: &'a str,
        request: FileUpdateRequest,
    ) -> AgentFuture<'a, TalosResult<FileUpdateResponse>> {
        Box::pin(async move {
            use talos_sdk::GatewayClient;
            self.source_client(owner, peer)
                .await?
                .update_file(request)
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
            use talos_sdk::GatewayClient;
            self.source_client(owner, peer)
                .await?
                .delete_file(request)
                .await
        })
    }
}

impl talos_service::ComputerManagement for GatewayPeerAccess {
    fn list<'a>(
        &'a self,
        actor: &'a talos_service::Actor,
        request: talos_types::ComputerListRequest,
    ) -> AgentFuture<'a, TalosResult<talos_types::ComputerListResponse>> {
        Box::pin(async move {
            self.host
                .upgrade()
                .ok_or_else(|| {
                    TalosError::new(
                        ErrorCode::ServiceUnavailable,
                        "Computer directory host has stopped.",
                    )
                })?
                .computer_directory
                .list(&actor.user_id, request)
        })
    }
    fn read<'a>(
        &'a self,
        actor: &'a talos_service::Actor,
        request: talos_types::GetNodeRequest,
    ) -> AgentFuture<'a, TalosResult<talos_types::NodeInfo>> {
        Box::pin(async move {
            self.host
                .upgrade()
                .ok_or_else(|| {
                    TalosError::new(
                        ErrorCode::ServiceUnavailable,
                        "Computer directory host has stopped.",
                    )
                })?
                .computer_directory
                .read(&actor.user_id, &request.node_id)
        })
    }
    fn delete<'a>(
        &'a self,
        actor: &'a talos_service::Actor,
        request: talos_types::DeleteComputerRequest,
    ) -> AgentFuture<'a, TalosResult<talos_types::DeleteComputerResponse>> {
        Box::pin(async move {
            let host = self.host.upgrade();
            let local_actor = match &host {
                Some(host)
                    if host
                        .config
                        .reload()?
                        .agent_node
                        .runner
                        .as_ref()
                        .is_some_and(|runner| runner.node_id == request.node_id) =>
                {
                    // G authenticates and authorizes the owner before its deletion
                    // response. Local retirement must not add another Auth outage
                    // after the central operation has committed.
                    Some(talos_auth::RequestIdentity {
                        user_id: actor.user_id.clone(),
                        username: None,
                        role: actor.role,
                        user_type: actor.user_type,
                        auth_disabled: actor.auth_disabled,
                        access_token: actor.access_token.clone(),
                    })
                }
                None if self
                    .config
                    .reload()?
                    .agent_node
                    .runner
                    .as_ref()
                    .is_some_and(|runner| runner.node_id == request.node_id) =>
                {
                    return Err(TalosError::new(
                        ErrorCode::ServiceUnavailable,
                        "Local registration lifecycle is unavailable.",
                    ))
                }
                _ => None,
            };
            let response = self
                .gateway()
                .await?
                .delete_computer(management_token(actor)?, request)
                .await?;
            if let (Some(host), Some(local_actor)) = (&host, local_actor) {
                host.observe_registration_removal(&local_actor, &response.node_id)
                    .await?;
            }
            let host = host.ok_or_else(|| {
                TalosError::new(
                    ErrorCode::ServiceUnavailable,
                    "Computer directory host has stopped.",
                )
            })?;
            host.cache_deleted_computer(actor, &response.node_id)
                .await?;
            Ok(response)
        })
    }

    fn create<'a>(
        &'a self,
        actor: &'a talos_service::Actor,
        request: talos_types::CreateNodeRequest,
    ) -> AgentFuture<'a, TalosResult<talos_types::CreatedNode>> {
        Box::pin(async move {
            let owner = request.owner_user_id.as_deref().unwrap_or(&actor.user_id);
            if owner != actor.user_id && actor.role != talos_auth::UserRole::Admin {
                return Err(TalosError::authorization_denied());
            }
            let started = Instant::now();
            let mut created = self
                .gateway()
                .await?
                .create_node(management_token(actor)?, request.clone())
                .await?;
            // Validate the committed response before treating cache publication as
            // an independent effect. Never deliver a mismatched owner's secret.
            if created.node.node_id == "embedded"
                || created.node.mode != talos_types::NodeMode::Remote
                || created.node.owner_user_id.as_deref() != Some(owner)
                || created.node.display_name != request.display_name
                || created.node.endpoint != request.endpoint
                || created.credential.expires_at != request.credential_expires_at
            {
                return Err(TalosError::authorization_denied());
            }
            match self
                .confirmed_computer(actor, &created.node.node_id, created.node.clone(), started)
                .await
            {
                Ok(current) => created.node = current,
                Err(error) => {
                    // Server has committed and retains only the credential hash.
                    // Report this secondary failure without losing its only delivery
                    // or publishing the older response over newer directory facts.
                    talos_logging::report_error(&error.context(
                        "Publish the committed Computer creation to the local directory.",
                    ));
                    created.node.health = talos_types::NodeHealth::Unknown;
                    created.node.available = false;
                    created.node.lease_expires_at = None;
                    created.node.last_heartbeat_at = None;
                }
            }
            Ok(created)
        })
    }
    fn update<'a>(
        &'a self,
        actor: &'a talos_service::Actor,
        request: talos_types::UpdateNodeRequest,
    ) -> AgentFuture<'a, TalosResult<talos_types::NodeInfo>> {
        Box::pin(async move {
            let id = request.node_id.clone();
            let started = Instant::now();
            let info = self
                .gateway()
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
                .await?;
            self.confirmed_computer(actor, &id, info, started).await
        })
    }
    fn rename<'a>(
        &'a self,
        actor: &'a talos_service::Actor,
        request: talos_types::RenameComputerRequest,
    ) -> AgentFuture<'a, TalosResult<talos_types::NodeInfo>> {
        Box::pin(async move {
            let id = request.node_id.clone();
            let started = Instant::now();
            let info = self
                .gateway()
                .await?
                .rename_computer(management_token(actor)?, request)
                .await?;
            self.confirmed_computer(actor, &id, info, started).await
        })
    }
    fn set_status<'a>(
        &'a self,
        actor: &'a talos_service::Actor,
        request: talos_types::SetComputerStatusRequest,
    ) -> AgentFuture<'a, TalosResult<talos_types::NodeInfo>> {
        Box::pin(async move {
            let id = request.node_id.clone();
            let started = Instant::now();
            let info = self
                .gateway()
                .await?
                .set_computer_status(management_token(actor)?, request)
                .await?;
            self.confirmed_computer(actor, &id, info, started).await
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
