use std::time::Duration;

use talos_error::{ErrorCode, TalosError, TalosResult};
use tokio::sync::oneshot;

use super::SyncHandle;
use crate::{SyncConnectionState, SyncResourceKey, SyncVersion};

pub(super) enum ControlCommand {
    CatchUp {
        resource: SyncResourceKey,
        minimum: Option<SyncVersion>,
        reply: oneshot::Sender<TalosResult<SyncVersion>>,
    },
    Flush {
        resource: SyncResourceKey,
        cut: Option<i64>,
        minimum: SyncVersion,
        reply: oneshot::Sender<TalosResult<SyncVersion>>,
    },
}

impl ControlCommand {
    pub(super) fn reject(self) {
        let reply = match self {
            Self::CatchUp { reply, .. } | Self::Flush { reply, .. } => reply,
        };
        let _ = reply.send(Err(unavailable()));
    }
}

impl SyncHandle {
    /// Recover and materialize a complete hub cut on this account's existing
    /// connection. An unavailable minimum is an explicit conflict, never a fake
    /// terminal history or execution handoff.
    pub async fn catch_up(
        &self,
        resource: SyncResourceKey,
        minimum: Option<SyncVersion>,
    ) -> TalosResult<SyncVersion> {
        let resource = crate::projection::resource(&self.binding, &resource);
        resource.validate().map_err(crate::facade::sync_error)?;
        let (reply, received) = oneshot::channel();
        self.request(
            ControlCommand::CatchUp {
                resource,
                minimum,
                reply,
            },
            received,
        )
        .await
    }

    /// Await the hub's durable ACK for this resource's currently committed local
    /// facts. The fixed outbox cut excludes later commits and survives disconnect
    /// through existing persistence; no execution lock or second queue is added.
    pub async fn flush(&self, resource: SyncResourceKey) -> TalosResult<SyncVersion> {
        let resource = crate::projection::resource(&self.binding, &resource);
        resource.validate().map_err(crate::facade::sync_error)?;
        let (cut, minimum) = self.sync.flush_cut(&self.binding, &resource).await?;
        let (reply, received) = oneshot::channel();
        self.request(
            ControlCommand::Flush {
                resource,
                cut,
                minimum,
                reply,
            },
            received,
        )
        .await
    }

    async fn request(
        &self,
        command: ControlCommand,
        reply: oneshot::Receiver<TalosResult<SyncVersion>>,
    ) -> TalosResult<SyncVersion> {
        if self.stop.is_cancelled() || self.status().connection != SyncConnectionState::Connected {
            return Err(unavailable());
        }
        tokio::time::timeout(Duration::from_secs(30), async {
            self.commands
                .send(command)
                .await
                .map_err(|_| unavailable())?;
            reply.await.map_err(|_| unavailable())?
        })
        .await
        .map_err(|_| TalosError::new(ErrorCode::Timeout, "Synchronization operation timed out."))?
    }
}

fn unavailable() -> TalosError {
    TalosError::new(
        ErrorCode::ServiceUnavailable,
        "Synchronization connection is unavailable.",
    )
}
