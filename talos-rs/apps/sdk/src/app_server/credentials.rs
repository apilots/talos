use talos_app_protocol::requests::{ActorCredential, ActorCredentials};

pub(super) fn actor_credentials_for_method(
    method: &str,
    credentials: &Option<ActorCredentials>,
) -> Option<ActorCredentials> {
    if is_public_method(method) {
        None
    } else {
        credentials.clone()
    }
}

pub(super) fn existing_session_credentials(token: String) -> ActorCredentials {
    ActorCredentials {
        credential: ActorCredential::ExistingSessionToken { token },
    }
}

fn is_public_method(method: &str) -> bool {
    matches!(
        method,
        "initialize"
            | "auth.login.start"
            | "auth.login.status"
            | "auth.login.cancel"
            | "auth.session.read"
            | "auth.session.clear"
    )
}

#[cfg(test)]
pub(super) fn is_business_method(method: &str) -> bool {
    !is_public_method(method)
}
