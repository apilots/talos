//! Media admission port composed by the host after Computer routing and authorization.
use agent_protocol::AgentFuture;
use talos_error::TalosResult;
use talos_types::DesktopStreamBinding;

use crate::Actor;

/// Reserve and consume existing desktop media transports. The host implements local
/// embedded and registered ingress routing; callers cannot supply a transport address.
pub trait DesktopIngress: Send + Sync {
    fn reserve<'a>(
        &'a self,
        actor: &'a Actor,
        node_id: String,
        binding: DesktopStreamBinding,
    ) -> AgentFuture<'a, TalosResult<agent_client::DesktopReservation>>;

    fn cancel<'a>(
        &'a self,
        actor: &'a Actor,
        node_id: String,
        stream_id: String,
        ticket: String,
    ) -> AgentFuture<'a, TalosResult<()>>;

    fn view<'a>(
        &'a self,
        actor: &'a Actor,
        node_id: String,
        stream_id: String,
        ticket: String,
    ) -> AgentFuture<'a, TalosResult<talos_browser::DesktopTransport>>;
}
