#[cfg(feature = "product")]
mod service;
mod types;

#[cfg(feature = "product")]
pub(crate) use service::ResourceAccessService;
#[cfg(feature = "product")]
pub(crate) use service::{embedded_access_denied, CreateSessionRequest};
pub use types::Actor;

#[cfg(feature = "product")]
/// Projects the shared authenticated identity into a node environment scope.
pub(crate) fn worker_environment_context(
    actor: &Actor,
) -> agent_protocol::WorkerEnvironmentContext {
    agent_protocol::WorkerEnvironmentContext {
        user_id: actor.user_id.clone(),
        username: actor.username.clone(),
        workspace_id: None,
    }
}
