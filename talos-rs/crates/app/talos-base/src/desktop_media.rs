//! Protected Desktop IPC last mile; Worker media remains a separate outbound gRPC stream.
use base64::Engine;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_service::Actor;
use talos_types::*;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

pub(crate) struct DesktopMedia {
    host: Arc<talos_app_server::DesktopHostBridge>,
    state: Arc<Mutex<State>>,
}

#[derive(Default)]
struct State {
    streams: HashMap<String, Stream>,
    suspensions: usize,
}

struct Stream {
    user: String,
    input: Option<mpsc::Sender<Vec<u8>>>,
    stop: Arc<CancellationToken>,
}

impl Drop for Stream {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

pub(crate) struct Viewer {
    state: Arc<Mutex<State>>,
    stream_id: String,
    stop: Arc<CancellationToken>,
}

impl Drop for Viewer {
    fn drop(&mut self) {
        let mut state = self.state.lock().expect("Desktop media state");
        if state
            .streams
            .get(&self.stream_id)
            .is_some_and(|stream| Arc::ptr_eq(&stream.stop, &self.stop))
        {
            state.streams.remove(&self.stream_id);
        }
    }
}

pub(crate) struct MediaSuspension(Arc<Mutex<State>>);

impl Drop for MediaSuspension {
    fn drop(&mut self) {
        self.0.lock().expect("Desktop media state").suspensions -= 1;
    }
}

impl DesktopMedia {
    pub(crate) fn new(host: Arc<talos_app_server::DesktopHostBridge>) -> Self {
        Self {
            host,
            state: Arc::default(),
        }
    }

    pub(crate) fn reserve(&self, actor: &Actor, stream_id: String) -> TalosResult<Viewer> {
        if uuid::Uuid::parse_str(&stream_id).is_err() {
            return Err(denied());
        }
        let mut state = self.state.lock().expect("Desktop media state");
        if state.suspensions > 0 {
            return Err(denied());
        }
        if state.streams.len() >= 2 || state.streams.contains_key(&stream_id) {
            return Err(TalosError::new(
                ErrorCode::Conflict,
                "Desktop viewer capacity exhausted.",
            ));
        }
        let stop = Arc::new(CancellationToken::new());
        state.streams.insert(
            stream_id.clone(),
            Stream {
                user: actor.user_id.clone(),
                input: None,
                stop: stop.clone(),
            },
        );
        Ok(Viewer {
            state: self.state.clone(),
            stream_id,
            stop,
        })
    }

    pub(crate) async fn open(
        &self,
        service: &talos_service::TalosService,
        actor: &Actor,
        ticket: String,
        viewer: Viewer,
    ) -> TalosResult<()> {
        let transport = tokio::select! {
            biased;
            _ = viewer.stop.cancelled() => return Err(denied()),
            result = service.consume_desktop_ticket(actor, ticket, Some(&viewer.stream_id)) => result?,
        };
        self.start_stream(viewer, transport)
    }

    fn start_stream(
        &self,
        viewer: Viewer,
        mut transport: talos_browser::DesktopTransport,
    ) -> TalosResult<()> {
        {
            let mut state = self.state.lock().expect("Desktop media state");
            let stream = state
                .streams
                .get_mut(&viewer.stream_id)
                .filter(|stream| Arc::ptr_eq(&stream.stop, &viewer.stop))
                .ok_or_else(denied)?;
            stream.input = Some(transport.outgoing.clone());
        }
        let host = self.host.clone();
        tokio::spawn(async move {
            loop {
                let frame = tokio::select! {
                    biased;
                    _ = viewer.stop.cancelled() => return,
                    frame = transport.incoming.recv() => frame,
                };
                let data = match frame {
                    Some(Ok(data)) if data.len() <= 64 * 1024 => data,
                    Some(Err(error)) => {
                        talos_logging::report_error(&error);
                        break;
                    }
                    Some(Ok(_)) | None => break,
                };
                let frame = DesktopMediaFrame {
                    stream_id: viewer.stream_id.clone(),
                    data_base64: Some(base64::engine::general_purpose::STANDARD.encode(data)),
                };
                let result = tokio::select! {
                    biased;
                    _ = viewer.stop.cancelled() => return,
                    result = host.media(frame) => result,
                };
                if let Err(error) = result {
                    talos_logging::report_error(&error);
                    break;
                }
            }
            drop(transport);
            let frame = DesktopMediaFrame {
                stream_id: viewer.stream_id.clone(),
                data_base64: None,
            };
            let result = tokio::select! {
                biased;
                _ = viewer.stop.cancelled() => return,
                result = host.media(frame) => result,
            };
            if let Err(error) = result {
                talos_logging::report_error(&error);
            }
            drop(viewer);
        });
        Ok(())
    }

    pub(crate) fn send(&self, actor: &Actor, params: DesktopStreamSend) -> TalosResult<()> {
        if params.data_base64.len() > 88 * 1024 {
            return Err(denied());
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(params.data_base64)
            .map_err(|e| denied().with_source(e))?;
        if bytes.is_empty() || bytes.len() > 64 * 1024 {
            return Err(denied());
        }
        let state = self.state.lock().expect("Desktop media state");
        let stream = state
            .streams
            .get(&params.stream_id)
            .filter(|s| s.user == actor.user_id)
            .ok_or_else(denied)?;
        let input = stream.input.as_ref().ok_or_else(denied)?;
        input.try_send(bytes).map_err(|_| {
            stream.stop.cancel();
            TalosError::new(
                ErrorCode::Conflict,
                "Desktop input stopped because the viewer is too slow.",
            )
        })
    }
    pub(crate) fn close(&self, actor: &Actor, params: DesktopStreamClose) -> TalosResult<()> {
        let mut state = self.state.lock().expect("Desktop media state");
        if state
            .streams
            .get(&params.stream_id)
            .is_some_and(|s| s.user != actor.user_id)
        {
            return Err(denied());
        }
        state.streams.remove(&params.stream_id);
        Ok(())
    }
    pub(crate) fn close_all(&self) {
        self.state
            .lock()
            .expect("Desktop media state")
            .streams
            .clear();
    }

    pub(crate) fn suspend(&self) -> MediaSuspension {
        let mut state = self.state.lock().expect("Desktop media state");
        state.suspensions += 1;
        state.streams.clear();
        MediaSuspension(self.state.clone())
    }
}
fn denied() -> TalosError {
    TalosError::new(
        ErrorCode::PermissionDenied,
        "The Desktop media stream is invalid or belongs to another account.",
    )
}

#[cfg(test)]
#[path = "desktop_media_tests.rs"]
mod tests;
