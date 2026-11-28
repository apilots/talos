use crate::{
    state::{self, Chunk, Connection, Stream, StreamGuard},
    RelayServer,
};
use axum::{
    body::Body,
    http::{HeaderMap, HeaderName, HeaderValue, Request, Response},
};
use futures_util::{stream, StreamExt};
use std::{
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
use talos_app_protocol::relay::{
    RelayHeader, RelayLane, RelayRequestHead, RelayToHost, RELAY_CHUNK_BYTES, RELAY_MAX_STREAMS,
};
use talos_auth::Actor;
use talos_error::{ErrorCode, TalosError, TalosResult};

impl RelayServer {
    /// Resource-origin requests carry an opaque Host-owned preview capability.
    /// This port admits only fixed read-only preview paths, never product commands.
    pub async fn forward_preview(
        &self,
        computer_id: &str,
        request: Request<Body>,
    ) -> TalosResult<Response<Body>> {
        if !matches!(request.method().as_str(), "GET" | "HEAD" | "OPTIONS")
            || !request
                .uri()
                .path()
                .starts_with("/v1/file-previews/resources/")
        {
            return Err(TalosError::new(
                ErrorCode::PermissionDenied,
                "Only scoped preview resources use capability relay.",
            ));
        }
        let connection = self
            .inner
            .connections
            .read()
            .await
            .get(computer_id)
            .cloned()
            .ok_or_else(state::unavailable)?;
        self.inner
            .nodes
            .authenticate_lease(&connection.auth)
            .await?;
        let owner = self
            .inner
            .nodes
            .authorize_lease_owner(&connection.auth)
            .await?;
        self.forward(&Actor::regular(owner), computer_id, request)
            .await
    }

    /// Forward an authorized personal product request through its attached host.
    /// Cloud products use their own Service path and are rejected here.
    pub async fn forward(
        &self,
        actor: &Actor,
        computer_id: &str,
        request: Request<Body>,
    ) -> TalosResult<Response<Body>> {
        self.require_personal(actor, computer_id).await?;
        let connection = self
            .inner
            .connections
            .read()
            .await
            .get(computer_id)
            .cloned()
            .ok_or_else(state::unavailable)?;
        self.inner
            .nodes
            .authenticate_lease(&connection.auth)
            .await?;
        if self
            .inner
            .nodes
            .authorize_lease_owner(&connection.auth)
            .await?
            != actor.user_id
        {
            return Err(TalosError::new(
                ErrorCode::PermissionDenied,
                "Personal product relay is restricted to its account owner.",
            ));
        }
        if connection.cancel.is_cancelled() {
            return Err(state::unavailable());
        }
        if request.headers().contains_key("upgrade")
            || connection_tokens(request.headers())
                .iter()
                .any(|name| name == "upgrade")
        {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "WebSocket upgrades use the existing node media transport.",
            ));
        }
        let id = self
            .inner
            .next_stream
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .map_err(|_| state::unavailable())?;
        let (mut parts, body) = request.into_parts();
        let generic_file = parts
            .headers
            .remove(talos_app_protocol::relay::RELAY_FILE_TRANSFER_HEADER)
            .is_some_and(|value| value == "true")
            && parts.uri.path() == "/v1/app/request";
        let lane = parts
            .extensions
            .get::<RelayLane>()
            .copied()
            .unwrap_or_else(|| {
                if generic_file {
                    RelayLane::File
                } else {
                    RelayLane::for_path(parts.uri.path())
                }
            });
        let mut identity = actor.clone();
        identity.access_token = None;
        identity.auth_disabled = false;
        let head = RelayRequestHead {
            stream_id: id,
            identity: talos_app_protocol::relay::RelayRequestIdentity(identity),
            method: parts.method.to_string(),
            path_and_query: parts
                .uri
                .path_and_query()
                .map_or("/", |value| value.as_str())
                .into(),
            headers: export_headers(&parts.headers)?,
            lane,
            callback_origin: parts
                .extensions
                .get::<talos_app_protocol::relay::RelayCallbackOrigin>()
                .map(|origin| origin.0.clone()),
        };
        head.validate()?;
        let (pending, reply, receiver) = Stream::new();
        {
            let mut streams = connection.streams.lock().await;
            if streams.len() >= RELAY_MAX_STREAMS {
                return Err(TalosError::new(
                    ErrorCode::ServiceUnavailable,
                    "Personal computer product request limit is reached.",
                ));
            }
            streams.insert(id, pending.clone());
        }
        let guard = StreamGuard {
            stream: pending.clone(),
        };
        let cleanup = connection.clone();
        let finished = pending.clone();
        tokio::spawn(async move {
            finished.cancel.cancelled().await;
            cleanup.streams.lock().await.remove(&id);
            let _ = cleanup
                .outgoing
                .control(RelayToHost::Cancel { stream_id: id });
        });
        connection
            .outgoing
            .send(RelayLane::Control, RelayToHost::Request { head })
            .await?;
        let sending = connection.clone();
        let upload = pending.clone();
        tokio::spawn(async move {
            if let Err(error) = send_body(sending.clone(), upload.clone(), id, lane, body).await {
                if !upload.cancel.is_cancelled() && !sending.cancel.is_cancelled() {
                    let _ = sending.outgoing.control(RelayToHost::Failure {
                        stream_id: id,
                        error: error.to_api_error(),
                    });
                    upload.fail(error).await;
                }
            }
        });
        let head = tokio::select! {
            _=connection.cancel.cancelled()=>return Err(state::unavailable()),
            result=tokio::time::timeout(Duration::from_secs(300),reply)=>result.map_err(|_|state::unavailable())?.map_err(|_|state::unavailable())??,
        };
        let response_state = ResponseState {
            receiver,
            connection,
            pending,
            stream_id: id,
            _guard: guard,
        };
        let body = Body::from_stream(stream::unfold(response_state, |mut state| async move {
            let next = state.receiver.recv().await;
            match next {
                Some(Chunk::Data { sequence, bytes }) => {
                    let acknowledged = state.pending.incoming.lock().await.window.consume(sequence);
                    let result = acknowledged
                        .and_then(|acknowledged| {
                            state.connection.outgoing.control(RelayToHost::Window {
                                stream_id: state.stream_id,
                                acknowledged,
                            })
                        })
                        .map(|()| bytes);
                    Some((result, state))
                }
                Some(Chunk::Failure(error)) => Some((Err(error), state)),
                None => None,
            }
        }));
        let mut response = Response::builder()
            .status(head.status)
            .body(body)
            .map_err(|error| {
                TalosError::internal_with_source("Build product relay response.", error)
            })?;
        *response.headers_mut() = import_headers(head.headers)?;
        Ok(response)
    }
}
struct ResponseState {
    receiver: tokio::sync::mpsc::Receiver<Chunk>,
    connection: Arc<Connection>,
    pending: Arc<Stream>,
    stream_id: u64,
    _guard: StreamGuard,
}
async fn send_body(
    connection: Arc<Connection>,
    stream: Arc<Stream>,
    id: u64,
    lane: RelayLane,
    body: Body,
) -> TalosResult<()> {
    let mut chunks = body.into_data_stream();
    loop {
        let chunk = tokio::select! {
            _=connection.cancel.cancelled()=>return Err(state::unavailable()),
            _=stream.cancel.cancelled()=>return Err(state::unavailable()),
            value=chunks.next()=>value,
        };
        let Some(chunk) = chunk else { break };
        let mut chunk = chunk.map_err(|error| {
            TalosError::internal_with_source("Read product relay request body.", error)
        })?;
        while !chunk.is_empty() {
            let sequence = stream.reserve(&connection.cancel).await?;
            let data = chunk.split_to(chunk.len().min(RELAY_CHUNK_BYTES)).to_vec();
            tokio::select! {
                _=stream.cancel.cancelled()=>return Err(state::unavailable()),
                result=connection.outgoing.send(lane,RelayToHost::Data {stream_id:id,sequence,data})=>result?,
            }
        }
    }
    // Same lane as Data preserves request-body ordering on the socket.
    tokio::select! {
        _=stream.cancel.cancelled()=>Err(state::unavailable()),
        result=connection.outgoing.send(lane,RelayToHost::End {stream_id:id})=>result,
    }
}
fn connection_tokens(headers: &HeaderMap) -> Vec<String> {
    headers
        .get_all("connection")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(|v| v.trim().to_ascii_lowercase())
        .collect()
}
fn hop_header(name: &str) -> bool {
    matches!(
        name,
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
            | "host"
    )
}
fn export_headers(headers: &HeaderMap) -> TalosResult<Vec<RelayHeader>> {
    let excluded = connection_tokens(headers);
    headers
        .iter()
        .filter(|(name, _)| {
            !hop_header(name.as_str())
                && !matches!(name.as_str(), "authorization" | "cookie")
                && !excluded.iter().any(|value| value == name.as_str())
        })
        .map(|(name, value)| {
            Ok(RelayHeader {
                name: name.to_string(),
                value: value
                    .to_str()
                    .map_err(|error| {
                        TalosError::from_source(
                            ErrorCode::InvalidRequest,
                            "Invalid product relay header.",
                            error,
                        )
                    })?
                    .into(),
            })
        })
        .collect()
}
fn import_headers(headers: Vec<RelayHeader>) -> TalosResult<HeaderMap> {
    let mut result = HeaderMap::new();
    let excluded: Vec<String> = headers
        .iter()
        .filter(|h| h.name.eq_ignore_ascii_case("connection"))
        .flat_map(|h| h.value.split(','))
        .map(|v| v.trim().to_ascii_lowercase())
        .collect();
    for header in headers {
        let lower = header.name.to_ascii_lowercase();
        if hop_header(&lower) || excluded.contains(&lower) {
            continue;
        }
        let name = HeaderName::from_bytes(header.name.as_bytes()).map_err(|error| {
            TalosError::from_source(
                ErrorCode::InvalidRequest,
                "Invalid product relay response header.",
                error,
            )
        })?;
        let value = HeaderValue::from_str(&header.value).map_err(|error| {
            TalosError::from_source(
                ErrorCode::InvalidRequest,
                "Invalid product relay response header.",
                error,
            )
        })?;
        result.append(name, value);
    }
    Ok(result)
}
