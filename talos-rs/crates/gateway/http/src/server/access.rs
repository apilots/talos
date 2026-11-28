use talos_auth::RequestIdentity;
use talos_service::Actor;

pub(super) fn shared_actor(actor: &RequestIdentity) -> Actor {
    let shared = Actor::regular(actor.user_id.clone())
        .with_username(actor.username.clone())
        .with_role(actor.role)
        .with_user_type(actor.user_type)
        .with_auth_disabled(actor.auth_disabled)
        .with_request_origin(talos_auth::ProductRequestOrigin::RemoteProduct);
    match actor.access_token.clone() {
        Some(token) => shared.with_access_token(token),
        None => shared,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn http_actors_always_have_server_assigned_remote_product_origin() {
        let identity = talos_auth::RequestIdentity::auth_disabled();
        assert_eq!(
            super::shared_actor(&identity).request_origin,
            talos_auth::ProductRequestOrigin::RemoteProduct
        );
    }
}
