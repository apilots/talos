//! Protected Desktop IPC last mile; Worker media remains a separate outbound gRPC stream.
use base64::Engine;
use std::{collections::HashMap, sync::Arc};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_service::Actor;
use talos_types::*;
use tokio::sync::{mpsc, Mutex};
use tokio_util::sync::CancellationToken;

pub(crate) struct DesktopMedia {
    host: Arc<talos_app_server::DesktopHostBridge>,
    streams: Arc<Mutex<HashMap<String, Stream>>>,
}
struct Stream {
    user: String,
    input: mpsc::Sender<Vec<u8>>,
    stop: CancellationToken,
}
impl Drop for Stream {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}
impl DesktopMedia {
    pub(crate) fn new(host: Arc<talos_app_server::DesktopHostBridge>) -> Self {
        Self {
            host,
            streams: Arc::default(),
        }
    }
    pub(crate) async fn open(
        &self,
        service: &talos_service::TalosService,
        actor: &Actor,
        params: DesktopStreamOpen,
    ) -> TalosResult<()> {
        if uuid::Uuid::parse_str(&params.stream_id).is_err() {
            return Err(denied());
        }
        let mut streams = self.streams.lock().await;
        streams.retain(|_, stream| !stream.stop.is_cancelled() && !stream.input.is_closed());
        if streams.len() >= 2 || streams.contains_key(&params.stream_id) {
            return Err(TalosError::new(
                ErrorCode::Conflict,
                "Desktop viewer capacity exhausted.",
            ));
        }
        let mut transport = service
            .consume_desktop_ticket(actor, params.ticket, Some(&params.stream_id))
            .await?;
        let stop = CancellationToken::new();
        streams.insert(
            params.stream_id.clone(),
            Stream {
                user: actor.user_id.clone(),
                input: transport.outgoing.clone(),
                stop: stop.clone(),
            },
        );
        let all = self.streams.clone();
        let host = self.host.clone();
        tokio::spawn(async move {
            loop {
                let data = tokio::select! {_=stop.cancelled()=>break,frame=transport.incoming.recv()=>match frame{Some(Ok(data)) if data.len()<=64*1024=>data,_=>break}};
                let frame = DesktopMediaFrame {
                    stream_id: params.stream_id.clone(),
                    data_base64: Some(base64::engine::general_purpose::STANDARD.encode(data)),
                };
                let result =
                    tokio::select! {_=stop.cancelled()=>break,result=host.media(frame)=>result};
                if let Err(error) = result {
                    talos_logging::report_error(&error);
                    break;
                }
            }
            drop(transport);
            all.lock().await.remove(&params.stream_id);
            if !stop.is_cancelled() {
                if let Err(error) = host
                    .media(DesktopMediaFrame {
                        stream_id: params.stream_id,
                        data_base64: None,
                    })
                    .await
                {
                    talos_logging::report_error(&error);
                }
            }
        });
        Ok(())
    }
    pub(crate) async fn send(&self, actor: &Actor, params: DesktopStreamSend) -> TalosResult<()> {
        if params.data_base64.len() > 88 * 1024 {
            return Err(denied());
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(params.data_base64)
            .map_err(|e| denied().with_source(e))?;
        if bytes.is_empty() || bytes.len() > 64 * 1024 {
            return Err(denied());
        }
        let streams = self.streams.lock().await;
        let stream = streams
            .get(&params.stream_id)
            .filter(|s| s.user == actor.user_id)
            .ok_or_else(denied)?;
        stream.input.try_send(bytes).map_err(|_| {
            stream.stop.cancel();
            TalosError::new(
                ErrorCode::Conflict,
                "Desktop input stopped because the viewer is too slow.",
            )
        })
    }
    pub(crate) async fn close(&self, actor: &Actor, params: DesktopStreamClose) -> TalosResult<()> {
        let mut streams = self.streams.lock().await;
        if streams
            .get(&params.stream_id)
            .is_some_and(|s| s.user != actor.user_id)
        {
            return Err(denied());
        }
        streams.remove(&params.stream_id);
        Ok(())
    }
    pub(crate) async fn close_all(&self) {
        self.streams.lock().await.clear();
    }
}
fn denied() -> TalosError {
    TalosError::new(
        ErrorCode::PermissionDenied,
        "The Desktop media stream is invalid or belongs to another account.",
    )
}
