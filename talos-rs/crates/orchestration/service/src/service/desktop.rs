//! Actor-authorized Docker viewer admission. Tickets are ephemeral and never enter tasks or URLs.
use super::TalosService;
use crate::Actor;
use sha2::{Digest, Sha256};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_types::*;

pub(super) struct ViewerRoute {
    actor: Actor,
    node_id: String,
    binding: DesktopStreamBinding,
    ingress_ticket: String,
    expires: std::time::Instant,
}

impl TalosService {
    pub fn set_desktop_ingress(
        &self,
        ingress: std::sync::Arc<dyn crate::DesktopIngress>,
    ) -> TalosResult<()> {
        let mut current = self
            .desktop_ingress
            .write()
            .map_err(|_| TalosError::internal("Desktop ingress state is poisoned."))?;
        if current.is_some() {
            return Err(TalosError::new(
                ErrorCode::Conflict,
                "Desktop ingress is already configured.",
            ));
        }
        *current = Some(ingress);
        Ok(())
    }
    fn desktop_ingress(&self) -> TalosResult<std::sync::Arc<dyn crate::DesktopIngress>> {
        self.desktop_ingress
            .read()
            .map_err(|_| TalosError::internal("Desktop ingress state is poisoned."))?
            .clone()
            .ok_or_else(|| {
                TalosError::new(
                    ErrorCode::ServiceUnavailable,
                    "The trusted desktop ingress is unavailable.",
                )
            })
    }
    pub async fn open_desktop(
        &self,
        actor: &Actor,
        request: DesktopOpenRequest,
    ) -> TalosResult<DesktopViewGrant> {
        let node_id = self
            .connect_selected_computer(actor, &request.node_id)
            .await?;
        if actor.auth_disabled && node_id != "embedded" {
            return Err(denied());
        }
        let environment = self.access.worker_environment(actor, &node_id).await?;
        let snapshot = environment
            .browser(
                actor.worker_environment_context(),
                BrowserRequest {
                    node_id: None,
                    operation: BrowserOperation::Read,
                },
            )
            .await?;
        if let BrowserDisplayCapability::Unavailable { reason } = snapshot.display {
            return Err(TalosError::localized(
                talos_error::ErrorLocalization::BrowserCapabilityUnavailable {
                    operation: talos_error::BrowserCapabilityOperation::RemoteDesktop,
                    reason: reason.into(),
                },
            ));
        }
        if !snapshot.enabled
            || request.target_id != "managed"
            || snapshot.control.generation != request.generation
        {
            return Err(TalosError::new(
                ErrorCode::Conflict,
                "Desktop state changed. Refresh before opening its viewer.",
            ));
        }
        let binding = DesktopStreamBinding {
            stream_id: uuid::Uuid::new_v4().to_string(),
            target_id: request.target_id,
            viewer_id: request.viewer_id,
            generation: request.generation,
            input_mode: request.input_mode,
            controller_id: request.controller_id,
            input_epoch: request.input_epoch,
        };
        agent_protocol::desktop_pb::validate_binding(&binding)?;
        let ingress = self.desktop_ingress()?;
        let reservation = ingress
            .reserve(actor, node_id.clone(), binding.clone())
            .await?;
        let result = async {
            environment
                .desktop_open(
                    actor.worker_environment_context(),
                    WorkerDesktopOpen {
                        binding: binding.clone(),
                        nonce: reservation.nonce,
                    },
                )
                .await?;
            let ticket = uuid::Uuid::new_v4().to_string();
            let key: [u8; 32] = Sha256::digest(ticket.as_bytes()).into();
            {
                let mut routes = self
                    .desktop_routes
                    .lock()
                    .map_err(|_| TalosError::internal("Desktop ticket state is poisoned."))?;
                routes.retain(|_, route| route.expires > std::time::Instant::now());
                if routes.len() >= 64 {
                    return Err(TalosError::new(
                        ErrorCode::Conflict,
                        "Desktop viewer capacity is exhausted.",
                    ));
                }
                routes.insert(
                    key,
                    ViewerRoute {
                        actor: actor.clone(),
                        node_id: node_id.clone(),
                        binding: binding.clone(),
                        ingress_ticket: reservation.ticket.clone(),
                        expires: std::time::Instant::now() + std::time::Duration::from_secs(30),
                    },
                );
            }
            Ok(DesktopViewGrant {
                ticket,
                stream_id: binding.stream_id.clone(),
                viewer_id: binding.viewer_id.clone(),
                generation: binding.generation.clone(),
                input_mode: binding.input_mode,
                controller_id: binding.controller_id.clone(),
                input_epoch: binding.input_epoch,
                expires_at: reservation.expires_at,
            })
        }
        .await;
        if result.is_err() {
            if let Err(error) = ingress
                .cancel(
                    actor,
                    node_id,
                    binding.stream_id.clone(),
                    reservation.ticket,
                )
                .await
            {
                talos_logging::report_error(&error);
            }
        }
        result
    }
    /// The same-origin WSS handler consumes a ticket once in its first message.
    /// Ingress rechecks the original identity and the selected host's live authority.
    pub async fn consume_desktop_ticket(
        &self,
        actor: &Actor,
        ticket: String,
        expected_stream: Option<&str>,
    ) -> TalosResult<talos_browser::DesktopTransport> {
        if uuid::Uuid::parse_str(&ticket).is_err() {
            return Err(denied());
        }
        let key: [u8; 32] = Sha256::digest(ticket.as_bytes()).into();
        let route = self
            .desktop_routes
            .lock()
            .map_err(|_| TalosError::internal("Desktop ticket state is poisoned."))?
            .remove(&key)
            .ok_or_else(denied)?;
        if route.expires <= std::time::Instant::now()
            || actor.user_id != route.actor.user_id
            || expected_stream.is_some_and(|stream| stream != route.binding.stream_id)
            || actor.auth_disabled != route.actor.auth_disabled
            || (actor.auth_disabled && route.node_id != "embedded")
        {
            return Err(denied());
        }
        let environment = self.worker_environment(actor, Some(&route.node_id)).await?;
        let snapshot = environment
            .browser(
                actor.worker_environment_context(),
                BrowserRequest {
                    node_id: None,
                    operation: BrowserOperation::Read,
                },
            )
            .await?;
        if !snapshot.enabled
            || snapshot.control.generation != route.binding.generation
            || !matches!(
                snapshot.display,
                BrowserDisplayCapability::DockerDesktop { .. }
            )
        {
            return Err(denied());
        }
        self.desktop_ingress()?
            .view(
                actor,
                route.node_id,
                route.binding.stream_id,
                route.ingress_ticket,
            )
            .await
    }
}
fn denied() -> TalosError {
    TalosError::new(
        ErrorCode::PermissionDenied,
        "The desktop ticket is invalid or expired.",
    )
}

#[cfg(test)]
#[path = "desktop_ticket_tests.rs"]
mod tests;
