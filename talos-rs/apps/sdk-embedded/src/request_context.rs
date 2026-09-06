use std::env;

use talos_auth::RequestIdentity;
use talos_base::TalosApplicationRuntime;
use talos_error::{TalosError, TalosResult};
use talos_service::Actor;
use talos_types::{
    talos::{SessionSource, TalosConnectionMode, TalosSessionEnvironment},
    AuthSessionReadRequest, StartTurnRequest,
};

pub(super) async fn resolve_cached_actor(
    runtime: &TalosApplicationRuntime,
) -> TalosResult<Option<Actor>> {
    let response = runtime
        .gateway
        .read_auth_session(&Actor::regular("anonymous"), AuthSessionReadRequest {})
        .await?;
    let Some(session) = response.session else {
        return Ok(None);
    };
    let identity = runtime
        .auth_service
        .verify_access_token(&session.access_token)
        .await?;
    Ok(Some(actor_from_identity(&identity)))
}

fn actor_from_identity(identity: &RequestIdentity) -> Actor {
    let actor = Actor::regular(identity.user_id.clone())
        .with_username(identity.username.clone())
        .with_role(identity.role);
    match identity.access_token.clone() {
        Some(token) => actor.with_access_token(token),
        None => actor,
    }
}

pub(super) async fn authenticated_actor_from_access_token(
    runtime: &TalosApplicationRuntime,
    access_token: &str,
) -> TalosResult<Actor> {
    let identity = runtime
        .auth_service
        .verify_access_token(access_token)
        .await?;
    Ok(actor_from_identity(&identity))
}

pub(super) fn prepare_embedded_start_turn(
    mut request: StartTurnRequest,
) -> TalosResult<(StartTurnRequest, SessionSource)> {
    let directory = match request.current_directory.clone() {
        Some(directory) => directory,
        None => env::current_dir().map_err(|err| {
            TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                format!("failed to resolve embedded current directory: {err}"),
            )
        })?,
    };

    let source = SessionSource::cli().with_environment(TalosSessionEnvironment {
        directory: directory.clone(),
        connection_mode: TalosConnectionMode::Embedded,
    });
    if request.current_directory.is_none() {
        request.current_directory = Some(directory);
    }
    Ok((request, source))
}
