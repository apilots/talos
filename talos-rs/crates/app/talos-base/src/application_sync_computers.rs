//! Current-host aliases and live Gateway reachability, using original Computer DTOs.
use super::application_sync::ApplicationSyncHost;
use agent_client::{AgentNodeClient, AgentTlsTrust};
use talos_auth::RequestIdentity;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_service::Actor;
use talos_types::NodeInfo;

impl ApplicationSyncHost {
    /// Presence follows current verified enrollment, not immutable capture aliases.
    pub(crate) async fn refresh_presence_computers(
        &self,
        actor: &RequestIdentity,
        handle: &talos_sync::SyncHandle,
    ) -> TalosResult<()> {
        let computers = self
            .local_computer(
                &Actor::regular(&actor.user_id)
                    .with_access_token(actor.access_token.clone().expect("verified token")),
            )
            .await?
            .filter(|computer_id| computer_id != "embedded")
            .map_or_else(Vec::new, |computer_id| {
                vec![talos_sync::ComputerAlias {
                    source_node_id: "embedded".into(),
                    computer_id,
                }]
            });
        self.verify_computer_aliases(actor, &computers).await?;
        handle.set_presence_computers(
            computers
                .into_iter()
                .map(|alias| alias.computer_id)
                .collect(),
        )
    }

    pub(crate) async fn local_computer_for_client(
        &self,
        actor: &Actor,
    ) -> TalosResult<Option<String>> {
        Ok(self
            .local_computer(actor)
            .await?
            .or_else(|| actor.auth_disabled.then(|| "embedded".into())))
    }
    pub(crate) async fn gateway_client(&self) -> TalosResult<AgentNodeClient> {
        gateway_client(&self.config).await
    }
    /// A saved UUID is identity, never proof of ownership for a newly logged-in account.
    pub(crate) async fn verify_computer_aliases(
        &self,
        actor: &RequestIdentity,
        aliases: &[talos_sync::ComputerAlias],
    ) -> TalosResult<()> {
        let previous = self.sync.account_binding(actor).await?;
        for alias in aliases {
            if previous.as_ref().is_some_and(|binding| {
                binding.computers.iter().any(|saved| {
                    saved.source_node_id == alias.source_node_id
                        && saved.computer_id == alias.computer_id
                })
            }) {
                continue;
            }
            let node = self
                .gateway_client()
                .await?
                .get_node(
                    actor.access_token.as_deref().expect("verified token"),
                    &alias.computer_id,
                )
                .await?;
            if node.owner_user_id.as_deref() != Some(actor.user_id.as_str()) {
                return Err(TalosError::new(
                    ErrorCode::PermissionDenied,
                    "This account does not own the registered Computer.",
                ));
            }
        }
        Ok(())
    }
    pub(crate) async fn owns_local_computer(&self, actor: &Actor) -> TalosResult<bool> {
        if actor.auth_disabled {
            return Ok(false);
        }
        if let Some(owner) = talos_config::YamlConfigStore::new(self.config.source_path())
            .removed_registration_owner()?
        {
            return Ok(owner == actor.user_id);
        }
        if self
            .local_owner
            .read()
            .expect("local computer owner lock")
            .as_deref()
            == Some(&actor.user_id)
        {
            return Ok(true);
        }
        let local = self.local_computer_id()?;
        let identity = RequestIdentity {
            user_id: actor.user_id.clone(),
            username: actor.username.clone(),
            role: actor.role,
            auth_disabled: false,
            access_token: actor.access_token.clone(),
        };
        Ok(self
            .sync
            .account_binding(&identity)
            .await?
            .is_some_and(|binding| {
                binding
                    .computers
                    .iter()
                    .any(|alias| alias.computer_id == local && alias.source_node_id == "embedded")
            }))
    }
    pub(crate) async fn local_computer(&self, actor: &Actor) -> TalosResult<Option<String>> {
        self.refresh_saved_registration_removal().await?;
        let local = self.local_computer_id()?;
        Ok(self.owns_local_computer(actor).await?.then_some(local))
    }
    pub(crate) async fn local_node(&self, actor: &Actor, computer_id: &str) -> TalosResult<String> {
        if computer_id == "embedded" {
            if self.local_computer_for_client(actor).await?.is_none() {
                return Err(TalosError::new(
                    ErrorCode::PermissionDenied,
                    "This account does not own the management host's Computer.",
                ));
            }
            return Ok(computer_id.into());
        }
        if self.local_computer(actor).await?.as_deref() == Some(computer_id) {
            Ok("embedded".into())
        } else {
            Ok(computer_id.into())
        }
    }
    pub(crate) async fn resolve_computer(
        &self,
        actor: &Actor,
        computer_id: &str,
    ) -> TalosResult<String> {
        if computer_id == "embedded" || actor.auth_disabled {
            return self.local_node(actor, computer_id).await;
        }
        let local = self.local_computer_id()?;
        if computer_id == local && self.owns_local_computer(actor).await? {
            return Ok("embedded".into());
        }
        let verified = self.verified_actor(actor).await?;
        self.connect_computer(&verified, computer_id).await?;
        if computer_id == local {
            *self.local_owner.write().expect("local computer owner lock") =
                Some(actor.user_id.clone());
            Ok("embedded".into())
        } else {
            Ok(computer_id.into())
        }
    }
    pub(crate) async fn computer_view(
        &self,
        actor: &Actor,
        mut info: NodeInfo,
    ) -> TalosResult<NodeInfo> {
        let local = self.local_computer_id()?;
        let own = self.owns_local_computer(actor).await?;
        if own && info.node_id == "embedded" {
            info.node_id = local.clone();
            info.owner_user_id = Some(actor.user_id.clone());
        }
        if info.node_id == "embedded" {
            info.system_info = Some(self.worker.system_info().clone());
        }
        if info.node_id == "embedded" || actor.auth_disabled {
            return Ok(info);
        }
        let verified = self.verified_actor(actor).await?;
        let result = self
            .gateway_client()
            .await?
            .get_node(
                verified.access_token.as_deref().expect("verified token"),
                &info.node_id,
            )
            .await;
        match result {
            Ok(live) => info = live,
            Err(error)
                if matches!(
                    error.error_code(),
                    ErrorCode::ServiceUnavailable | ErrorCode::Timeout
                ) =>
            {
                info.available = false;
            }
            Err(error)
                if own && info.node_id == local && error.is_code(ErrorCode::ResourceExpired) =>
            {
                self.observe_registration_removal(&verified, &local).await?;
                info.node_id = "embedded".into();
                info.mode = talos_types::NodeMode::Embedded;
                info.owner_user_id = Some(actor.user_id.clone());
                info.available = true;
                info.system_info = Some(self.worker.system_info().clone());
                return Ok(info);
            }
            Err(error) if error.is_code(ErrorCode::ResourceExpired) => {
                self.sync_stores
                    .nodes
                    .reconcile_deleted_registration(&info.node_id, &actor.user_id)
                    .await?;
                return Err(error);
            }
            Err(error) => return Err(error),
        }
        if own && info.node_id == local {
            // Cloud revocation or an offline G never revokes this host's private loopback credential.
            info.available = true;
            info.system_info = Some(self.worker.system_info().clone());
        }
        Ok(info)
    }
}

pub(crate) async fn gateway_client(config: &talos_config::Config) -> TalosResult<AgentNodeClient> {
    let certificate = config
        .cloud
        .agent_trusted_ca_file
        .as_ref()
        .map(std::fs::read)
        .transpose()
        .map_err(|error| {
            TalosError::internal_with_source(
                "Read Agent CA; technical details were withheld.",
                error,
            )
        })?;
    AgentNodeClient::connect(
        &config.cloud.agent_base_url,
        certificate.map_or(AgentTlsTrust::SystemRoots, AgentTlsTrust::Certificate),
    )
    .await
}
