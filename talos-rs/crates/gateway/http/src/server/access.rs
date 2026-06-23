use talos_auth::RequestIdentity;
use talos_service::Actor;

pub(super) fn shared_actor(actor: &RequestIdentity) -> Actor {
    let shared = Actor::regular(actor.user_id.clone())
        .with_username(actor.username.clone())
        .with_role(actor.role);
    match actor.access_token.clone() {
        Some(token) => shared.with_access_token(token),
        None => shared,
    }
}
