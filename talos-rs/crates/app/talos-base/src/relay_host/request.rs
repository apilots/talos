use super::{
    invalid,
    state::{self, Chunk, Connection, Stream},
};
use axum::{
    body::Body,
    http::{HeaderName, HeaderValue, Request},
    Router,
};
use futures_util::{stream, StreamExt};
use std::sync::Arc;
use talos_app_protocol::relay::{
    HostToRelay, RelayHeader, RelayRequestHead, RelayResponseHead, RelayToHost, RELAY_CHUNK_BYTES,
    RELAY_MAX_STREAMS,
};
use talos_error::{TalosError, TalosResult};
use tower::ServiceExt;

pub(super) async fn dispatch(
    connection: &Arc<Connection>,
    router: &Router,
    frame: RelayToHost,
) -> TalosResult<()> {
    if let RelayToHost::Request { head } = frame {
        head.validate()?;
        if head.identity.0.user_id != connection.owner_user_id {
            return Err(TalosError::new(
                talos_error::ErrorCode::PermissionDenied,
                "Product relay identity does not own this Host lease.",
            ));
        }
        let id = head.stream_id;
        let (pending, receiver) = Stream::new();
        {
            let mut streams = connection.streams.lock().await;
            if streams.len() >= RELAY_MAX_STREAMS || streams.contains_key(&id) {
                return Err(invalid("Product relay stream admission is invalid."));
            }
            streams.insert(id, pending.clone());
        }
        let connection = connection.clone();
        let router = router.clone();
        tokio::spawn(async move {
            let outcome = serve(connection.clone(), pending.clone(), router, head, receiver);
            let result = tokio::select! {
                _=connection.cancel.cancelled()=>Ok(()),_=pending.cancel.cancelled()=>Ok(()),result=outcome=>result,
            };
            if let Err(error) = result {
                // A full control queue closes the connection; no failure is hidden.
                let _ = connection.outgoing.control(HostToRelay::Failure {
                    stream_id: id,
                    error: error.to_api_error(),
                });
            }
            pending.cancel.cancel();
            connection.streams.lock().await.remove(&id);
        });
        return Ok(());
    }
    let id = match &frame {
        RelayToHost::Ready { .. } | RelayToHost::Request { .. } => {
            return Err(invalid("Product relay readiness cannot be repeated."))
        }
        RelayToHost::Data { stream_id, .. }
        | RelayToHost::End { stream_id }
        | RelayToHost::Window { stream_id, .. }
        | RelayToHost::Cancel { stream_id }
        | RelayToHost::Failure { stream_id, .. } => *stream_id,
    };
    let Some(pending) = connection.streams.lock().await.get(&id).cloned() else {
        return Ok(());
    };
    let result: TalosResult<()> = async {
        match frame {
            RelayToHost::Data { sequence, data, .. } => {
                let mut incoming = pending.incoming.lock().await;
                incoming.window.receive(sequence)?;
                incoming
                    .sender
                    .as_ref()
                    .ok_or_else(|| invalid("Product request data arrived after end."))?
                    .try_send(Chunk::Data {
                        sequence,
                        bytes: data.into(),
                    })
                    .map_err(|_| invalid("Product request exceeded its bounded receive queue."))?;
            }
            RelayToHost::End { .. } => {
                pending.incoming.lock().await.sender.take();
            }
            RelayToHost::Window { acknowledged, .. } => {
                pending.outgoing.lock().await.acknowledge(acknowledged)?;
                pending.credit.notify_one();
            }
            RelayToHost::Cancel { .. } => {
                pending.fail(state::unavailable()).await;
            }
            RelayToHost::Failure { error, .. } => {
                pending.fail(error.into()).await;
            }
            _ => unreachable!("request and readiness were handled above"),
        }
        Ok(())
    }
    .await;
    if let Err(error) = result {
        let public = error.to_api_error();
        pending.fail(error).await;
        connection.outgoing.control(HostToRelay::Failure {
            stream_id: id,
            error: public,
        })?;
    }
    Ok(())
}
struct RequestBody {
    receiver: tokio::sync::mpsc::Receiver<Chunk>,
    connection: Arc<Connection>,
    pending: Arc<Stream>,
    id: u64,
}
async fn serve(
    connection: Arc<Connection>,
    pending: Arc<Stream>,
    router: Router,
    head: RelayRequestHead,
    receiver: tokio::sync::mpsc::Receiver<Chunk>,
) -> TalosResult<()> {
    let id = head.stream_id;
    let lane = head.lane;
    let state = RequestBody {
        receiver,
        connection: connection.clone(),
        pending: pending.clone(),
        id,
    };
    let body = Body::from_stream(stream::unfold(state, |mut state| async move {
        match state.receiver.recv().await {
            Some(Chunk::Data { sequence, bytes }) => {
                let consumed = state.pending.incoming.lock().await.window.consume(sequence);
                let result = consumed
                    .and_then(|acknowledged| {
                        state.connection.outgoing.control(HostToRelay::Window {
                            stream_id: state.id,
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
    let mut request = Request::builder()
        .method(head.method.as_str())
        .uri(head.path_and_query)
        .body(body)
        .map_err(|error| TalosError::internal_with_source("Build product request.", error))?;
    let mut identity = head.identity;
    request
        .extensions_mut()
        .insert(talos_gateway_http::SelectedComputer {
            computer_id: "embedded".into(),
            requested_alias: Some(connection.node_id.clone()),
        });
    identity.0.request_origin = talos_auth::ProductRequestOrigin::RemoteProduct;
    request.extensions_mut().insert(identity);
    if let Some(origin) = head.callback_origin {
        request
            .extensions_mut()
            .insert(talos_app_protocol::relay::RelayCallbackOrigin(origin));
    }
    for header in head.headers {
        if hop_header(&header.name) {
            continue;
        }
        let name = HeaderName::from_bytes(header.name.as_bytes()).map_err(|error| {
            TalosError::from_source(
                talos_error::ErrorCode::InvalidRequest,
                "Invalid product request header.",
                error,
            )
        })?;
        let value = HeaderValue::from_str(&header.value).map_err(|error| {
            TalosError::from_source(
                talos_error::ErrorCode::InvalidRequest,
                "Invalid product request header.",
                error,
            )
        })?;
        request.headers_mut().append(name, value);
    }
    let response = match router.oneshot(request).await {
        Ok(response) => response,
        Err(never) => match never {},
    };
    if response.status().as_u16() < 200 {
        return Err(invalid("HTTP upgrades are not product body streams."));
    }
    let excluded: Vec<String> = response
        .headers()
        .get_all("connection")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(|value| value.trim().to_ascii_lowercase())
        .collect();
    let headers = response
        .headers()
        .iter()
        .filter(|(name, _)| {
            !hop_header(name.as_str()) && !excluded.iter().any(|x| x == name.as_str())
        })
        .map(|(name, value)| {
            Ok(RelayHeader {
                name: name.to_string(),
                value: value
                    .to_str()
                    .map_err(|error| {
                        TalosError::internal_with_source("Encode product response header.", error)
                    })?
                    .into(),
            })
        })
        .collect::<TalosResult<Vec<_>>>()?;
    connection.outgoing.control(HostToRelay::ResponseHead {
        head: RelayResponseHead {
            stream_id: id,
            status: response.status().as_u16(),
            headers,
        },
    })?;
    let mut chunks = response.into_body().into_data_stream();
    while let Some(chunk) = chunks.next().await {
        let mut chunk = chunk.map_err(|error| {
            TalosError::internal_with_source("Read product response body.", error)
        })?;
        while !chunk.is_empty() {
            let sequence = pending.reserve(&connection.cancel).await?;
            let data = chunk.split_to(chunk.len().min(RELAY_CHUNK_BYTES)).to_vec();
            connection
                .outgoing
                .send(
                    lane,
                    HostToRelay::Data {
                        stream_id: id,
                        sequence,
                        data,
                    },
                )
                .await?;
        }
    }
    // Keep End behind the response data in the same lane.
    connection
        .outgoing
        .send(lane, HostToRelay::End { stream_id: id })
        .await
}
fn hop_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
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
