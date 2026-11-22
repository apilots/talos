//! Redeem a personal Talo grant at the Server that issued it.
use super::*;
use futures_util::{SinkExt, StreamExt};
use reqwest_websocket::{Message, RequestBuilderExt, WebSocket};
use serde_json::json;
use talos_browser::DesktopTransport;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

impl DesktopCloud {
    pub(crate) async fn connect_viewer(
        &self,
        actor: &Actor,
        ticket: &str,
        stream_id: &str,
    ) -> TalosResult<DesktopTransport> {
        let token = actor
            .access_token
            .as_deref()
            .ok_or_else(TalosError::authorization_denied)?;
        let mut url = self
            .origin
            .join("/v1/desktop/stream")
            .map_err(|_| invalid_origin())?;
        url.query_pairs_mut().append_pair("streamId", stream_id);
        let mut socket = self
            .client
            .get(url)
            .timeout(std::time::Duration::from_secs(60))
            .version(reqwest::Version::HTTP_11)
            .header(
                reqwest::header::ORIGIN,
                self.origin.origin().ascii_serialization(),
            )
            .upgrade()
            .send()
            .await
            .map_err(media_error)?
            .into_websocket()
            .await
            .map_err(media_error)?;
        socket
            .send(Message::Text(
                json!({"ticket": ticket, "accessToken": token}).to_string(),
            ))
            .await
            .map_err(media_error)?;
        // The Server validates the ticket after upgrade and RFB speaks first.
        let first = tokio::time::timeout(Duration::from_secs(5), next_media_frame(&mut socket))
            .await
            .map_err(|error| {
                TalosError::from_source(
                    ErrorCode::Timeout,
                    "The Computer Server did not start the desktop stream in time.",
                    error,
                )
            })??
            .ok_or_else(|| {
                TalosError::new(
                    ErrorCode::ServiceUnavailable,
                    "The Computer Server closed the desktop stream before sending media.",
                )
            })?;
        let (outgoing, mut input) = mpsc::channel::<Vec<u8>>(64);
        let (output, incoming) = mpsc::channel(64);
        let stopped = CancellationToken::new();
        let stop = stopped.clone();
        tokio::spawn(async move {
            if output.send(Ok(first)).await.is_err() {
                return;
            }
            let pump = async {
                loop {
                    tokio::select! {
                        frame = input.recv() => {
                            let Some(frame) = frame else { break };
                            socket.send(Message::Binary(frame.into())).await.map_err(media_error)?;
                        }
                        frame = next_media_frame(&mut socket) => {
                            let Some(data) = frame? else { break; };
                            if output.send(Ok(data)).await.is_err() { break; }
                        }
                    }
                }
                Ok(())
            };
            tokio::select! {
                _ = stop.cancelled() => (),
                result = pump => if let Err(error) = result { let _ = output.send(Err(error)).await; },
            }
        });
        Ok(DesktopTransport {
            incoming,
            outgoing,
            stopped,
        })
    }
}

async fn next_media_frame(socket: &mut WebSocket) -> TalosResult<Option<Vec<u8>>> {
    loop {
        match socket.next().await.transpose().map_err(media_error)? {
            Some(Message::Binary(data)) if !data.is_empty() && data.len() <= 64 * 1024 => {
                return Ok(Some(data.to_vec()));
            }
            Some(Message::Close { .. }) | None => return Ok(None),
            Some(Message::Ping(_) | Message::Pong(_)) => (),
            _ => {
                return Err(TalosError::new(
                    ErrorCode::InvalidRequest,
                    "Invalid desktop media frame.",
                ))
            }
        }
    }
}

fn media_error(error: reqwest_websocket::Error) -> TalosError {
    TalosError::from_source(
        ErrorCode::ServiceUnavailable,
        "The Computer Server desktop stream is unavailable.",
        error,
    )
}
