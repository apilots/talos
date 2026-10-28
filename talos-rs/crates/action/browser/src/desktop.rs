//! Worker-owned immutable desktop streams. Account/node routing stays outside this crate.
use super::*;
use crate::rfb::RfbInput;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::mpsc,
};

pub struct DesktopTransport {
    pub incoming: mpsc::Receiver<TalosResult<Vec<u8>>>,
    pub outgoing: mpsc::Sender<Vec<u8>>,
    pub stopped: CancellationToken,
}
impl Drop for DesktopTransport {
    fn drop(&mut self) {
        self.stopped.cancel();
    }
}
pub type DesktopConnector =
    Arc<dyn Fn(WorkerDesktopOpen) -> BrowserFuture<'static, DesktopTransport> + Send + Sync>;
pub(super) struct View {
    pub(super) binding: DesktopStreamBinding,
    user_id: String,
    pub(super) stop: CancellationToken,
}

impl BrowserRuntime {
    pub(super) fn check_desktop_cleanup(&self) -> TalosResult<()> {
        match self
            .state
            .lock()
            .map_err(|_| poisoned())?
            .desktop_cleanup_error
            .clone()
        {
            Some(error) => Err(error.into()),
            None => Ok(()),
        }
    }
    /// Registration withdrawal fences only the controlled remote Docker environment.
    pub fn registration_revoked(&self) -> TalosResult<()> {
        let mut s = self.state.lock().map_err(|_| poisoned())?;
        if s.host != BrowserHostKind::Docker {
            return Ok(());
        }
        for view in s.views.values() {
            view.stop.cancel();
        }
        s.preferences.manual_hold = true;
        revoke(&mut s);
        s.owner = BrowserControlOwner::Suspended {
            reason: BrowserSuspensionReason::ViewerDisconnected,
        };
        self.mutations.close();
        self.persist(&s.preferences)
    }
    /// Composition supplies a client for the preconfigured trusted ingress only.
    pub fn set_desktop_connector(&self, connector: DesktopConnector) -> TalosResult<()> {
        let mut slot = self.desktop_connector.write().map_err(|_| poisoned())?;
        if slot.is_some() {
            return Err(conflict("The desktop ingress is already bound."));
        }
        *slot = Some(connector);
        Ok(())
    }
    pub async fn open_desktop(
        self: &Arc<Self>,
        user_id: String,
        request: WorkerDesktopOpen,
    ) -> TalosResult<()> {
        self.require_desktop()?;
        let binding = request.binding.clone();
        for id in [
            &binding.stream_id,
            &binding.viewer_id,
            &binding.generation,
            &request.nonce,
        ] {
            if Uuid::parse_str(id).is_err() {
                return Err(denied());
            }
        }
        if binding.target_id != "managed" {
            return Err(denied());
        }
        let stop = CancellationToken::new();
        {
            let mut state = self.state.lock().map_err(|_| poisoned())?;
            if state.generation != binding.generation
                || state.views.values().any(|view| view.user_id != user_id)
                || state.views.contains_key(&binding.stream_id)
            {
                return Err(conflict("The desktop viewer is no longer available."));
            }
            match binding.input_mode {
                BrowserInputMode::ReadOnly
                    if binding.controller_id.is_none() && binding.input_epoch.is_none() => {}
                BrowserInputMode::Control
                    if matches!(&state.owner, BrowserControlOwner::HandingOver { controller_id }
                    if Some(controller_id) == binding.controller_id.as_ref())
                        && binding.input_epoch == Some(state.epoch)
                        && state.controller_stream.is_none() => {}
                _ => return Err(denied()),
            }
            if state.views.values().any(|view| {
                view.binding.viewer_id == binding.viewer_id
                    && view.binding.input_mode == BrowserInputMode::Control
                    && !view.stop.is_cancelled()
            }) {
                return Err(conflict(
                    "Release the current control stream before replacing it.",
                ));
            }
            let same_viewer = state
                .views
                .values()
                .filter(|view| view.binding.viewer_id == binding.viewer_id)
                .count();
            let viewers: std::collections::HashSet<_> = state
                .views
                .values()
                .map(|view| &view.binding.viewer_id)
                .collect();
            // Keep the admitted stream until its single bounded candidate has
            // established both the private socket and authenticated ingress.
            if same_viewer >= 2 || (same_viewer == 0 && viewers.len() >= 2) {
                return Err(conflict("Desktop viewer capacity is exhausted."));
            }
            if binding.input_mode == BrowserInputMode::Control {
                state.controller_stream = Some(binding.stream_id.clone());
            }
            state.views.insert(
                binding.stream_id.clone(),
                View {
                    binding: binding.clone(),
                    user_id,
                    stop: stop.clone(),
                },
            );
        }
        let prepared = async {
            let connector = self
                .desktop_connector
                .read()
                .map_err(|_| poisoned())?
                .clone()
                .ok_or_else(|| unavailable("The trusted desktop ingress is unavailable."))?;
            let socket = self.backend()?.desktop().await?;
            self.check_view(&binding)?;
            let transport = connector(request).await?;
            self.check_view(&binding)?;
            {
                let mut state = self.state.lock().map_err(|_| poisoned())?;
                Self::check_view_state(&state, &binding)?;
                state.views.retain(|id, view| {
                    if *id != binding.stream_id && view.binding.viewer_id == binding.viewer_id {
                        view.stop.cancel();
                        false
                    } else {
                        true
                    }
                });
            }
            Ok::<_, TalosError>((socket, transport))
        }
        .await;
        let (socket, transport) = match prepared {
            Ok(value) => value,
            Err(error) => {
                self.close_view(&binding).await?;
                return Err(error);
            }
        };
        let runtime = self.clone();
        let mutation =
            (binding.input_mode == BrowserInputMode::Control).then(|| self.mutations.token());
        tokio::spawn(async move {
            let _mutation = mutation;
            let result = runtime
                .forward_desktop(binding.clone(), socket, transport, stop)
                .await;
            // A single handling owner reports transport failure without stream credentials.
            if let Err(error) = result {
                talos_logging::report_error(&error);
            }
            if let Err(error) = runtime.close_view(&binding).await {
                talos_logging::report_error(&error);
            }
        });
        Ok(())
    }

    pub(super) fn require_desktop(&self) -> TalosResult<()> {
        let snapshot = self.snapshot()?;
        if !snapshot.enabled {
            return Err(unavailable("Browser is disabled."));
        }
        if let BrowserDisplayCapability::Unavailable { reason } = snapshot.display {
            return Err(TalosError::localized(
                talos_error::ErrorLocalization::BrowserCapabilityUnavailable {
                    operation: talos_error::BrowserCapabilityOperation::RemoteDesktop,
                    reason: reason.into(),
                },
            ));
        }
        Ok(())
    }
    fn check_view(&self, binding: &DesktopStreamBinding) -> TalosResult<()> {
        if self.shutdown.is_cancelled() || !(self.enabled)()? {
            return Err(denied());
        }
        let s = self.state.lock().map_err(|_| poisoned())?;
        Self::check_view_state(&s, binding)
    }
    fn check_view_state(s: &State, binding: &DesktopStreamBinding) -> TalosResult<()> {
        let view = s.views.get(&binding.stream_id).ok_or_else(denied)?;
        if view.binding != *binding
            || view.stop.is_cancelled()
            || s.generation != binding.generation
        {
            return Err(denied());
        }
        if binding.input_mode == BrowserInputMode::Control
            && (binding.input_epoch != Some(s.epoch)
                || s.controller_stream.as_ref() != Some(&binding.stream_id)
                || !matches!(&s.owner, BrowserControlOwner::HandingOver {controller_id} | BrowserControlOwner::Human {controller_id}
                    if Some(controller_id) == binding.controller_id.as_ref()))
        {
            return Err(denied());
        }
        Ok(())
    }
    async fn forward_desktop(
        &self,
        binding: DesktopStreamBinding,
        socket: tokio::net::TcpStream,
        mut transport: DesktopTransport,
        stop: CancellationToken,
    ) -> TalosResult<()> {
        let (mut reader, mut writer) = socket.into_split();
        let mut buffer = vec![0u8; 64 * 1024];
        let mut parser = RfbInput::new(binding.input_mode == BrowserInputMode::Control);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        let mut initialized = false;
        let result=async {
            loop {
                tokio::select! {
                    biased;
                    _=stop.cancelled()=>break,
                    _=self.shutdown.cancelled()=>break,
                    _=transport.stopped.cancelled()=>break,
                    _=tokio::time::sleep_until(deadline), if !initialized=>return Err(unavailable("The desktop handshake did not complete in time.")),
                    chunk=transport.incoming.recv()=>{
                        let Some(chunk)=chunk else {break;};
                        let chunk=chunk?;
                        self.check_view(&binding)?;
                        for message in parser.receive(&chunk)? {
                            let _input=self.input.clone().lock_owned().await;
                            self.check_view(&binding)?;
                            parser.prepare_send(&message.data)?;
                            tokio::time::timeout(Duration::from_secs(2),writer.write_all(&message.data)).await
                                .map_err(|error|TalosError::from_source(ErrorCode::Timeout,"Desktop input is not being consumed.",error))?
                                .map_err(|error|TalosError::internal_with_source("The private desktop input connection failed.",error))?;
                            parser.sent(&message.data);
                            if message.initializes {
                                self.desktop_ready(&binding)?;
                                initialized=true;
                            }
                        }
                    },
                    length=reader.read(&mut buffer)=>{
                        let length=length.map_err(|error|TalosError::internal_with_source("The private desktop display connection failed.",error))?;
                        if length==0 {break;}
                        tokio::time::timeout(Duration::from_secs(2),transport.outgoing.send(buffer[..length].to_vec())).await
                            .map_err(|error|TalosError::from_source(ErrorCode::Timeout,"The desktop viewer is too slow.",error))?
                            .map_err(|error|TalosError::internal_with_source("The desktop media connection closed.",error))?;
                    },
                }
            }
            Ok(())
        }.await;
        // The mutation tracker remains held until all old pressed inputs have
        // been released. A handoff cannot acknowledge before this completes.
        let released = async {
            let _input = self.input.clone().lock_owned().await;
            for message in parser.releases() {
                writer.write_all(&message).await.map_err(|error| {
                    TalosError::from_source(
                        ErrorCode::RunCleanupUnconfirmed,
                        "Desktop held-input release could not be confirmed.",
                        error,
                    )
                })?;
            }
            if binding.input_mode == BrowserInputMode::Control {
                writer.shutdown().await.map_err(|error| {
                    TalosError::from_source(
                        ErrorCode::RunCleanupUnconfirmed,
                        "The old desktop input stream could not be closed.",
                        error,
                    )
                })?;
                // EOF from the private RFB server confirms it consumed or
                // discarded the old stream before new input admission.
                while reader.read(&mut buffer).await.map_err(|error| {
                    TalosError::from_source(
                        ErrorCode::RunCleanupUnconfirmed,
                        "The private desktop server did not confirm stream closure.",
                        error,
                    )
                })? != 0
                {}
            }
            Ok::<_, TalosError>(())
        };
        let released = tokio::time::timeout(Duration::from_secs(2), released)
            .await
            .map_err(|error| {
                TalosError::from_source(
                    ErrorCode::RunCleanupUnconfirmed,
                    "Desktop held-input release timed out.",
                    error,
                )
            })
            .and_then(|value| value);
        transport.stopped.cancel();
        if let Err(error) = &released {
            if binding.input_mode == BrowserInputMode::Control {
                let mut s = self.state.lock().map_err(|_| poisoned())?;
                s.desktop_cleanup_error = Some(error.to_api_error());
                s.preferences.manual_hold = true;
                revoke(&mut s);
                s.owner = BrowserControlOwner::Suspended {
                    reason: BrowserSuspensionReason::TakeoverFailed,
                };
                self.persist(&s.preferences)?;
            }
        }
        released?;
        result
    }
    fn desktop_ready(&self, binding: &DesktopStreamBinding) -> TalosResult<()> {
        if self.shutdown.is_cancelled() || !(self.enabled)()? {
            return Err(denied());
        }
        let mut s = self.state.lock().map_err(|_| poisoned())?;
        Self::check_view_state(&s, binding)?;
        if binding.input_mode == BrowserInputMode::Control {
            s.owner = BrowserControlOwner::Human {
                controller_id: binding.controller_id.clone().ok_or_else(denied)?,
            };
            s.revision += 1;
        }
        Ok(())
    }
    async fn close_view(&self, binding: &DesktopStreamBinding) -> TalosResult<()> {
        let mut s = self.state.lock().map_err(|_| poisoned())?;
        if !s
            .views
            .get(&binding.stream_id)
            .is_some_and(|v| v.binding == *binding)
        {
            return Ok(());
        }
        if let Some(view) = s.views.remove(&binding.stream_id) {
            view.stop.cancel();
        }
        if s.controller_stream.as_ref() == Some(&binding.stream_id)
            && binding.input_epoch == Some(s.epoch)
        {
            s.controller_stream = None;
            s.preferences.manual_hold = true;
            revoke(&mut s);
            s.owner = BrowserControlOwner::Suspended {
                reason: BrowserSuspensionReason::ViewerDisconnected,
            };
            self.persist(&s.preferences)?;
        }
        Ok(())
    }

    pub(super) fn stop_desktop_views(&self, controls_only: bool) -> TalosResult<()> {
        let mut s = self.state.lock().map_err(|_| poisoned())?;
        for view in s.views.values() {
            if !controls_only || view.binding.input_mode == BrowserInputMode::Control {
                view.stop.cancel();
            }
        }
        s.controller_stream = None;
        Ok(())
    }
}

#[cfg(test)]
#[path = "desktop_tests.rs"]
mod tests;
