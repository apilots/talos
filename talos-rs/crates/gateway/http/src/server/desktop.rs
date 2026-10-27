//! Same-origin viewer endpoint. Browser credentials appear only in the first message.
use super::{
    access::shared_actor,
    errors::{api_success, ApiError},
    state::HttpApiState,
};
use axum::{
    extract::{
        ws::{Message, WebSocketUpgrade},
        State,
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use std::{sync::Arc, time::Duration};
use talos_auth::RequestIdentity;
use talos_error::{ErrorCode, TalosError, TalosResult};

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Ticket {
    ticket: String,
    #[serde(rename = "accessToken")]
    access_token: Option<String>,
}

pub(super) async fn open(
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<RequestIdentity>,
    Json(request): Json<talos_types::DesktopOpenRequest>,
) -> Result<Response, ApiError> {
    let grant = state
        .gateway
        .open_desktop(&shared_actor(&auth.0), request)
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok((
        [(axum::http::header::CACHE_CONTROL, "no-store")],
        api_success(grant),
    )
        .into_response())
}
pub(super) async fn stream(
    State(state): State<Arc<HttpApiState>>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, StatusCode> {
    if !same_origin(&headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(ws.max_message_size(64*1024).max_frame_size(64*1024).write_buffer_size(32*1024).max_write_buffer_size(128*1024)
        .on_upgrade(move |mut socket|async move {
            let result=async {
                let message=tokio::time::timeout(Duration::from_secs(5),socket.recv()).await.map_err(|_|denied())?
                    .ok_or_else(denied)?.map_err(|_|denied())?;
                let Message::Text(text)=message else {return Err(denied());};
                if text.len()>8192 {return Err(denied());}
                let ticket:Ticket=serde_json::from_str(&text).map_err(|_|denied())?;
                let identity=stream_identity(&state,ticket.access_token.as_deref())?;
                let mut media=state.app.consume_desktop_ticket(&shared_actor(&identity),ticket.ticket,None).await?;
                loop {
                    tokio::select! {
                        _=media.stopped.cancelled()=>break,
                        incoming=socket.recv()=>{
                            let Some(incoming)=incoming else {break;};
                            match incoming.map_err(|_|denied())? {
                                Message::Binary(bytes) if !bytes.is_empty() && bytes.len()<=64*1024=>{
                                    tokio::time::timeout(Duration::from_secs(2),media.outgoing.send(bytes)).await.map_err(|_|denied())?.map_err(|_|denied())?;
                                },
                                Message::Close(_)=>break,
                                Message::Ping(_) | Message::Pong(_)=>{},
                                _=>return Err(denied()),
                            }
                        },
                        outgoing=media.incoming.recv()=>{
                            let Some(outgoing)=outgoing else {break;};
                            tokio::time::timeout(Duration::from_secs(2),socket.send(Message::Binary(outgoing?))).await.map_err(|_|denied())?.map_err(|_|denied())?;
                        }
                    }
                }
                Ok::<_,TalosError>(())
            }.await;
            if let Err(error)=result {talos_logging::report_error(&error);}
            // A stalled peer can also block the close handshake. Bound that
            // final flush so dropping the socket actually releases its lease.
            let _=tokio::time::timeout(Duration::from_millis(250),socket.close()).await;
        }))
}
/// Stream admission consumes the same verified local identity as other data APIs.
pub(super) fn stream_identity(
    state: &HttpApiState,
    access_token: Option<&str>,
) -> TalosResult<RequestIdentity> {
    if state.auth_disabled {
        Ok(RequestIdentity::auth_disabled())
    } else {
        state
            .auth_service
            .local_identity(access_token.ok_or_else(denied)?)
    }
}
fn denied() -> TalosError {
    TalosError::new(
        ErrorCode::PermissionDenied,
        "Desktop stream admission or framing was rejected.",
    )
}
fn same_origin(headers: &HeaderMap) -> bool {
    let Some(host) = headers.get("host").and_then(|value| value.to_str().ok()) else {
        return false;
    };
    let Some(origin) = headers
        .get("origin")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| url::Url::parse(value).ok())
    else {
        return false;
    };
    matches!(origin.scheme(), "http" | "https")
        && origin.username().is_empty()
        && origin.password().is_none()
        && origin.path() == "/"
        && origin.query().is_none()
        && origin.fragment().is_none()
        && origin[url::Position::BeforeHost..url::Position::AfterPort].eq_ignore_ascii_case(host)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_missing_cross_origin_and_credential_origins() {
        let mut headers = HeaderMap::new();
        headers.insert("host", "talos.example".parse().unwrap());
        for origin in [
            None,
            Some("null"),
            Some("https://attacker.example"),
            Some("https://talos.example.attacker.example"),
            Some("https://user@talos.example"),
            Some("https://talos.example/path"),
        ] {
            headers.remove("origin");
            if let Some(origin) = origin {
                headers.insert("origin", origin.parse().unwrap());
            }
            assert!(!same_origin(&headers));
        }
        headers.insert("origin", "https://talos.example".parse().unwrap());
        assert!(same_origin(&headers));
    }
}
