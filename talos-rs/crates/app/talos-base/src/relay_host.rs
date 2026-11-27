//! Outbound product relay adapter. The existing HTTP router remains the API owner.
mod request;
mod state;

use agent_client::{AgentNodeLeaseUpdate, AgentTlsTrust};
use axum::Router;
use futures_util::{SinkExt, StreamExt};
use reqwest_websocket::{Message, RequestBuilderExt};
use state::Connection;
use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
use talos_app_protocol::relay::{HostToRelay, RelayToHost, RELAY_MAX_FRAME_BYTES, RELAY_VERSION};
use talos_error::{ErrorCode, TalosError, TalosResult};
use tokio::{sync::watch, task::JoinHandle};

/// Reconnect delivery independently of the local Worker and its admitted turns.
/// Lease changes invalidate only this transport and its open HTTP streams.
pub(crate) fn spawn(
    endpoint: String,
    trust: AgentTlsTrust,
    host_id: String,
    mut leases: watch::Receiver<Option<AgentNodeLeaseUpdate>>,
    router: Router,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let result = client(&trust);
        let http = match result {
            Ok(value) => value,
            Err(error) => {
                talos_logging::report_error(&error);
                return;
            }
        };
        let endpoint = match product_endpoint(&endpoint) {
            Ok(endpoint) => endpoint,
            Err(error) => {
                talos_logging::report_error(&error);
                return;
            }
        };
        loop {
            let lease = leases.borrow_and_update().clone();
            let Some(lease) = lease.filter(|lease| lease.deadline > tokio::time::Instant::now())
            else {
                if leases.changed().await.is_err() {
                    break;
                }
                continue;
            };
            let (connection, receivers) =
                Connection::new(lease.owner_user_id.clone(), lease.auth.node_id.clone());
            let _lifetime = ConnectionLifetime(connection.cancel.clone());
            let session = connected(
                &http,
                &endpoint,
                lease.auth.clone(),
                &host_id,
                router.clone(),
                connection.clone(),
                receivers,
            );
            tokio::pin!(session);
            let mut deadline = lease.deadline;
            loop {
                tokio::select! {
                    changed=leases.changed()=>{
                        if changed.is_err() { break; }
                        let current = leases.borrow_and_update().clone();
                        if let Some(current) = current.filter(|current| current.auth.node_id == lease.auth.node_id && current.auth.token == lease.auth.token && current.owner_user_id == lease.owner_user_id && current.deadline > tokio::time::Instant::now()) {
                            deadline = current.deadline;
                        } else { break; }
                    },
                    _=tokio::time::sleep_until(deadline)=>break,
                    result=&mut session=>{
                        if let Err(error)=result { talos_logging::report_error(&error); }
                        break;
                    },
                }
            }
            connection.fail_all().await;
            tokio::select! {
                changed=leases.changed()=>if changed.is_err(){break},
                _=tokio::time::sleep(Duration::from_secs(1))=>{},
            }
        }
    })
}
fn client(trust: &AgentTlsTrust) -> TalosResult<reqwest::Client> {
    let mut builder = reqwest::Client::builder()
        .http1_only()
        .connect_timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none());
    if let AgentTlsTrust::Certificate(pem) = trust {
        builder = builder.tls_built_in_root_certs(false);
        for certificate in reqwest::Certificate::from_pem_bundle(pem).map_err(|error| {
            TalosError::from_source(
                ErrorCode::InvalidRequest,
                "Product relay CA is invalid.",
                error,
            )
        })? {
            builder = builder.add_root_certificate(certificate);
        }
    }
    builder
        .build()
        .map_err(|error| TalosError::internal_with_source("Configure product relay TLS.", error))
}
async fn connected(
    http: &reqwest::Client,
    endpoint: &str,
    auth: agent_protocol::NodeAuth,
    host_id: &str,
    router: Router,
    connection: Arc<Connection>,
    mut receivers: state::Receivers,
) -> TalosResult<()> {
    let mut socket = tokio::time::timeout(Duration::from_secs(10), async {
        http.get(endpoint)
            .upgrade()
            .web_socket_config(
                tungstenite::protocol::WebSocketConfig::default()
                    .max_message_size(Some(RELAY_MAX_FRAME_BYTES))
                    .max_frame_size(Some(RELAY_MAX_FRAME_BYTES)),
            )
            .send()
            .await?
            .into_websocket()
            .await
    })
    .await
    .map_err(|error| {
        TalosError::from_source(
            ErrorCode::Timeout,
            "Product relay handshake timed out.",
            error,
        )
    })?
    .map_err(socket_error)?;
    socket
        .send(Message::Text(
            serde_json::to_string(&HostToRelay::Hello {
                version: RELAY_VERSION,
                auth,
                host_id: host_id.to_owned(),
            })
            .map_err(|error| {
                TalosError::internal_with_source("Encode product relay authentication.", error)
            })?,
        ))
        .await
        .map_err(socket_error)?;
    let first = tokio::time::timeout(Duration::from_secs(10), socket.next())
        .await
        .map_err(|error| {
            TalosError::from_source(
                ErrorCode::Timeout,
                "Product relay readiness timed out.",
                error,
            )
        })?
        .ok_or_else(state::unavailable)?
        .map_err(socket_error)?;
    if !matches!(
        decode(first)?,
        Some(RelayToHost::Ready {
            version: RELAY_VERSION
        })
    ) {
        return Err(invalid("Product relay readiness is required."));
    }
    let (mut sink, mut source) = socket.split();
    let received_pong = AtomicU64::new(0);
    let pong_ready = tokio::sync::Notify::new();
    let writer = async {
        let mut position = 0;
        let mut nonce = 0_u64;
        let mut pending_probe = None;
        let mut interval = tokio::time::interval_at(
            tokio::time::Instant::now() + Duration::from_secs(15),
            Duration::from_secs(15),
        );
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            let deadline =
                pending_probe.map_or_else(tokio::time::Instant::now, |(_, deadline)| deadline);
            let frame = tokio::select! {
                biased;
                _=pong_ready.notified()=>{if pending_probe.is_some_and(|(nonce,_)|received_pong.load(Ordering::Acquire)==nonce){pending_probe=None;}continue;},
                _=tokio::time::sleep_until(deadline),if pending_probe.is_some()=>return Err(TalosError::new(ErrorCode::Timeout,"Product relay liveness timed out.")),
                _=interval.tick(),if pending_probe.is_none()=>{
                    nonce=nonce.checked_add(1).ok_or_else(state::unavailable)?;
                    tokio::time::timeout(Duration::from_secs(30),sink.send(Message::Ping(nonce.to_be_bytes().to_vec().into()))).await.map_err(|error|TalosError::from_source(ErrorCode::Timeout,"Product relay write timed out.",error))?.map_err(socket_error)?;
                    pending_probe=Some((nonce,tokio::time::Instant::now()+Duration::from_secs(30)));continue;
                },
                frame=receivers.next(&mut position)=>frame.ok_or_else(state::unavailable)?,
            };
            let bytes = serde_json::to_string(&frame).map_err(|error| {
                TalosError::internal_with_source("Encode product relay frame.", error)
            })?;
            if bytes.len() > RELAY_MAX_FRAME_BYTES {
                return Err(invalid("Product relay frame exceeds its limit."));
            }
            tokio::time::timeout(Duration::from_secs(30), sink.send(Message::Text(bytes)))
                .await
                .map_err(|error| {
                    TalosError::from_source(
                        ErrorCode::Timeout,
                        "Product relay write timed out.",
                        error,
                    )
                })?
                .map_err(socket_error)?;
        }
    };
    let reader = async {
        while let Some(frame) = source.next().await {
            let frame = frame.map_err(socket_error)?;
            if let Message::Pong(bytes) = &frame {
                if let Ok(bytes) = <[u8; 8]>::try_from(bytes.as_ref()) {
                    received_pong.store(u64::from_be_bytes(bytes), Ordering::Release);
                    pong_ready.notify_one();
                }
            }
            if matches!(frame, Message::Close { .. }) {
                return Ok(());
            }
            if let Some(frame) = decode(frame)? {
                request::dispatch(&connection, &router, frame).await?;
            }
        }
        Ok(())
    };
    tokio::select! {_=connection.cancel.cancelled()=>Ok(()),r=writer=>r,r=reader=>r}
}
fn decode(message: Message) -> TalosResult<Option<RelayToHost>> {
    let bytes = match message {
        Message::Text(value) => value.into_bytes(),
        Message::Binary(value) => value.to_vec(),
        Message::Ping(_) | Message::Pong(_) | Message::Close { .. } => return Ok(None),
    };
    if bytes.len() > RELAY_MAX_FRAME_BYTES {
        return Err(invalid("Product relay frame exceeds its limit."));
    }
    let frame: RelayToHost = serde_json::from_slice(&bytes).map_err(|error| {
        TalosError::from_source(
            ErrorCode::InvalidRequest,
            "Invalid product relay frame.",
            error,
        )
    })?;
    frame.validate()?;
    Ok(Some(frame))
}
fn socket_error(error: reqwest_websocket::Error) -> TalosError {
    TalosError::from_source(
        ErrorCode::ServiceUnavailable,
        "Product relay transport is unavailable.",
        error,
    )
}
fn invalid(message: &'static str) -> TalosError {
    TalosError::new(ErrorCode::InvalidRequest, message)
}

struct ConnectionLifetime(tokio_util::sync::CancellationToken);
impl Drop for ConnectionLifetime {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

#[cfg(test)]
mod tests;

fn product_endpoint(endpoint: &str) -> TalosResult<String> {
    let endpoint = reqwest::Url::parse(endpoint).map_err(|error| {
        TalosError::from_source(
            ErrorCode::InvalidRequest,
            "Product relay endpoint is invalid.",
            error,
        )
    })?;
    let loopback = endpoint.host_str().is_some_and(|host| {
        host == "localhost"
            || host
                .trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    });
    if !(endpoint.scheme() == "https" || (endpoint.scheme() == "http" && loopback))
        || !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.query().is_some()
        || endpoint.fragment().is_some()
    {
        return Err(TalosError::new(
            ErrorCode::InvalidRequest,
            "Product relay requires HTTPS, except for explicit local loopback development.",
        ));
    }
    Ok(format!(
        "{}/talos/product/connect",
        endpoint.as_str().trim_end_matches('/')
    ))
}
