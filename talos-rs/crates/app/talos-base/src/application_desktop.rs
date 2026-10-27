//! Compose local and registered desktop media through the same relay contract.
use std::sync::Arc;

use agent_client::{AgentNodeClient, DesktopReservation};
use agent_protocol::AgentFuture;
use talos_auth::AuthService;
use talos_browser::DesktopTransport;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_service::{Actor, DesktopIngress};
use talos_types::DesktopStreamBinding;

use crate::agent_node_rpc::{guard_transport, DesktopRelay};

pub(crate) struct ApplicationDesktopIngress {
    pub(crate) local: DesktopRelay,
    pub(crate) remote: AgentNodeClient,
    pub(crate) auth: Arc<dyn AuthService>,
}

impl DesktopIngress for ApplicationDesktopIngress {
    fn reserve<'a>(
        &'a self,
        actor: &'a Actor,
        node_id: String,
        binding: DesktopStreamBinding,
    ) -> AgentFuture<'a, TalosResult<DesktopReservation>> {
        Box::pin(async move {
            if node_id == "embedded" {
                authorize_local(self.auth.as_ref(), actor)?;
                self.local
                    .reserve(actor.user_id.clone(), node_id, binding)
                    .await
            } else {
                self.remote
                    .reserve_desktop(remote_token(actor)?, node_id, binding)
                    .await
            }
        })
    }

    fn cancel<'a>(
        &'a self,
        actor: &'a Actor,
        node_id: String,
        stream_id: String,
        ticket: String,
    ) -> AgentFuture<'a, TalosResult<()>> {
        Box::pin(async move {
            if node_id == "embedded" {
                self.local
                    .cancel(&actor.user_id, &node_id, &stream_id, &ticket)
                    .await
            } else {
                self.remote
                    .cancel_desktop(remote_token(actor)?, node_id, stream_id, ticket)
                    .await
            }
        })
    }

    fn view<'a>(
        &'a self,
        actor: &'a Actor,
        node_id: String,
        stream_id: String,
        ticket: String,
    ) -> AgentFuture<'a, TalosResult<DesktopTransport>> {
        Box::pin(async move {
            if node_id != "embedded" {
                return self
                    .remote
                    .view_desktop(remote_token(actor)?, node_id, stream_id, ticket)
                    .await;
            }
            authorize_local(self.auth.as_ref(), actor)?;
            let transport = self
                .local
                .viewer(&actor.user_id, &node_id, &stream_id, &ticket)
                .await?;
            if actor.auth_disabled {
                return Ok(transport);
            }
            let auth = self.auth.clone();
            let actor = actor.clone();
            Ok(guard_transport(transport, move || {
                let auth = auth.clone();
                let actor = actor.clone();
                async move { authorize_local(auth.as_ref(), &actor) }
            }))
        })
    }
}

fn authorize_local(auth: &dyn AuthService, actor: &Actor) -> TalosResult<()> {
    if actor.auth_disabled {
        return Ok(());
    }
    let identity = auth.local_identity(remote_token(actor)?)?;
    if identity.user_id != actor.user_id || identity.auth_disabled {
        return Err(denied());
    }
    Ok(())
}

fn remote_token(actor: &Actor) -> TalosResult<&str> {
    if actor.auth_disabled {
        return Err(denied());
    }
    actor.access_token.as_deref().ok_or_else(|| {
        TalosError::new(
            ErrorCode::Unauthenticated,
            "Sign in before opening a remote desktop.",
        )
    })
}

fn denied() -> TalosError {
    TalosError::new(
        ErrorCode::PermissionDenied,
        "Desktop media identity is unavailable.",
    )
}

#[cfg(test)]
#[path = "application_desktop_tests.rs"]
mod tests;
