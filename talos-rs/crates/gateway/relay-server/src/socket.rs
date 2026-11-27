use crate::{
    state::{self, Chunk, Connection},
    RelayServer,
};
use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};
use talos_app_protocol::relay::{
    HostToRelay, RelayLane, RelayToHost, RELAY_MAX_FRAME_BYTES, RELAY_VERSION,
};
use talos_auth::Actor;
use talos_error::{ErrorCode, TalosError, TalosResult};
use tokio_util::sync::CancellationToken;

impl RelayServer {
    pub(crate) async fn connected(
        &self,
        mut socket: WebSocket,
        shutdown: CancellationToken,
    ) -> TalosResult<()> {
        let hello = tokio::time::timeout(Duration::from_secs(10), socket.recv())
            .await
            .map_err(|_| invalid("Product relay authentication timed out."))?
            .ok_or_else(state::unavailable)?
            .map_err(|error| {
                TalosError::internal_with_source("Read product relay authentication.", error)
            })?;
        let HostToRelay::Hello {
            version: _,
            auth,
            host_id,
        } = decode(hello)?.ok_or_else(|| invalid("Product relay authentication is required."))?
        else {
            return Err(invalid("Product relay authentication is required."));
        };
        let guard = self.inner.nodes.lock_lifecycle(&auth.node_id).await;
        self.inner.nodes.authenticate_lease(&auth).await?;
        let owner = self.inner.nodes.authorize_lease_owner(&auth).await?;
        self.require_personal(&Actor::regular(owner), &auth.node_id)
            .await?;
        let lifetime = self.inner.nodes.lease_cancellation(&auth).await?;
        let (connection, receivers) = Connection::new(auth, host_id);
        let previous = self
            .inner
            .connections
            .write()
            .await
            .insert(connection.auth.node_id.clone(), connection.clone());
        if let Some(previous) = previous {
            previous.cancel.cancel();
        }
        drop(guard);
        connection
            .outgoing
            .send(
                RelayLane::Control,
                RelayToHost::Ready {
                    version: RELAY_VERSION,
                },
            )
            .await?;
        let (mut sink, mut source) = socket.split();
        let received_pong = AtomicU64::new(0);
        let pong_ready = tokio::sync::Notify::new();
        let writer = async {
            let mut receivers = receivers;
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
                        tokio::time::timeout(Duration::from_secs(30),sink.send(Message::Ping(nonce.to_be_bytes().to_vec()))).await.map_err(|error|TalosError::from_source(ErrorCode::Timeout,"Product relay write timed out.",error))?.map_err(|error|TalosError::internal_with_source("Probe product relay liveness.",error))?;
                        pending_probe=Some((nonce,tokio::time::Instant::now()+Duration::from_secs(30)));continue;
                    },
                    frame=receivers.next(&mut position)=>frame.ok_or_else(state::unavailable)?,
                };
                let bytes = serde_json::to_string(&frame).map_err(|error| {
                    TalosError::internal_with_source("Encode product relay frame.", error)
                })?;
                if bytes.len() > RELAY_MAX_FRAME_BYTES {
                    return Err(invalid("Product relay frame exceeds the transport limit."));
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
                    .map_err(|error| {
                        TalosError::internal_with_source("Write product relay frame.", error)
                    })?;
            }
        };
        let reader = async {
            while let Some(frame) = source.next().await {
                let frame = frame.map_err(|error| {
                    TalosError::internal_with_source("Read product relay frame.", error)
                })?;
                if let Message::Pong(bytes) = &frame {
                    if let Ok(bytes) = <[u8; 8]>::try_from(bytes.as_slice()) {
                        received_pong.store(u64::from_be_bytes(bytes), Ordering::Release);
                        pong_ready.notify_one();
                    }
                }
                if let Message::Close(_) = frame {
                    return Ok(());
                }
                if let Some(frame) = decode(frame)? {
                    dispatch(&connection, frame).await?;
                }
            }
            Ok(())
        };
        let validity = async {
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            loop {
                interval.tick().await;
                self.inner
                    .nodes
                    .authenticate_lease(&connection.auth)
                    .await?;
                if self
                    .inner
                    .nodes
                    .product_owner(
                        &Actor::regular(
                            self.inner
                                .nodes
                                .authorize_lease_owner(&connection.auth)
                                .await?,
                        ),
                        &connection.auth.node_id,
                    )
                    .await?
                    != Some(talos_types::NodeProductOwner::Computer)
                {
                    return Err(invalid("Product relay role is no longer available."));
                }
            }
        };
        let result = tokio::select! {
            _=shutdown.cancelled()=>Ok(()),
            _=lifetime.cancelled()=>Ok(()),
            _=connection.cancel.cancelled()=>Ok(()),
            r=writer=>r,r=reader=>r,r=validity=>r,
        };
        {
            let mut current = self.inner.connections.write().await;
            if current
                .get(&connection.auth.node_id)
                .is_some_and(|entry| entry.id == connection.id)
            {
                current.remove(&connection.auth.node_id);
            }
        }
        connection.fail_all().await;
        result
    }
}
fn decode(message: Message) -> TalosResult<Option<HostToRelay>> {
    let bytes = match message {
        Message::Text(value) => value.into_bytes(),
        Message::Binary(value) => value,
        Message::Ping(_) | Message::Pong(_) | Message::Close(_) => return Ok(None),
    };
    if bytes.len() > RELAY_MAX_FRAME_BYTES {
        return Err(invalid("Product relay frame exceeds the transport limit."));
    }
    let frame: HostToRelay = serde_json::from_slice(&bytes).map_err(|error| {
        TalosError::from_source(
            ErrorCode::InvalidRequest,
            "Invalid product relay frame.",
            error,
        )
    })?;
    frame.validate()?;
    Ok(Some(frame))
}
async fn dispatch(connection: &Connection, frame: HostToRelay) -> TalosResult<()> {
    let id = match &frame {
        HostToRelay::Hello { .. } => {
            return Err(invalid("Product relay authentication cannot be repeated."))
        }
        HostToRelay::ResponseHead { head } => head.stream_id,
        HostToRelay::Data { stream_id, .. }
        | HostToRelay::End { stream_id }
        | HostToRelay::Window { stream_id, .. }
        | HostToRelay::Cancel { stream_id }
        | HostToRelay::Failure { stream_id, .. } => *stream_id,
    };
    let Some(stream) = connection.streams.lock().await.get(&id).cloned() else {
        return Ok(());
    };
    let result = async {
        match frame {
            HostToRelay::Hello { .. } => unreachable!("authentication was rejected above"),
            HostToRelay::ResponseHead { head } => {
                head.validate()?;
                if head.status < 200 {
                    return Err(invalid(
                        "HTTP upgrades and provisional responses are not product body streams.",
                    ));
                }
                let sender = stream
                    .head
                    .lock()
                    .await
                    .take()
                    .ok_or_else(|| invalid("Product relay response head was repeated."))?;
                stream.started.store(true, Ordering::Release);
                // The HTTP caller may have closed between dispatch and delivery.
                if sender.send(Ok(head)).is_err() {
                    stream.cancel.cancel();
                }
                Ok(())
            }
            HostToRelay::Data { sequence, data, .. } => {
                if !stream.started() {
                    return Err(invalid(
                        "Product relay data arrived before its response head.",
                    ));
                }
                let mut incoming = stream.incoming.lock().await;
                incoming.window.receive(sequence)?;
                let sender = incoming
                    .sender
                    .as_ref()
                    .ok_or_else(|| invalid("Product relay data arrived after end of stream."))?;
                sender
                    .try_send(Chunk::Data {
                        sequence,
                        bytes: data.into(),
                    })
                    .map_err(|_| {
                        invalid("Product relay stream exceeded its bounded receive queue.")
                    })?;
                Ok(())
            }
            HostToRelay::Window { acknowledged, .. } => {
                stream.outgoing.lock().await.acknowledge(acknowledged)?;
                stream.credit.notify_one();
                Ok(())
            }
            HostToRelay::End { .. } => {
                if !stream.started() {
                    return Err(invalid(
                        "Product relay stream ended before its response head.",
                    ));
                }
                stream.incoming.lock().await.sender.take();
                Ok(())
            }
            HostToRelay::Cancel { .. } => {
                stream
                    .fail(TalosError::new(
                        ErrorCode::Cancelled,
                        "Personal product request was cancelled.",
                    ))
                    .await;
                Ok(())
            }
            HostToRelay::Failure { error, .. } => {
                stream.fail(error.into()).await;
                Ok(())
            }
        }
    }
    .await;
    if let Err(error) = result {
        let public = error.to_api_error();
        stream.fail(error).await;
        connection.outgoing.control(RelayToHost::Failure {
            stream_id: id,
            error: public,
        })?;
    }
    Ok(())
}
fn invalid(message: &'static str) -> TalosError {
    TalosError::new(ErrorCode::InvalidRequest, message)
}
