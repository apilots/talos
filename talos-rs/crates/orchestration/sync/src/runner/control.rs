use std::time::Duration;

use talos_error::{ErrorCode, TalosError, TalosResult};
use tokio::sync::oneshot;

use super::SyncHandle;
use crate::{SyncConnectionState, SyncResourceKey, SyncVersion};

const CONTROL_TIMEOUT: Duration = Duration::from_secs(30);

pub(super) enum ControlCommand {
    CatchUp {
        resource: SyncResourceKey,
        minimum: Option<SyncVersion>,
        reply: oneshot::Sender<TalosResult<SyncVersion>>,
    },
    Flush {
        resource: SyncResourceKey,
        target: FlushTarget,
        reply: oneshot::Sender<TalosResult<SyncVersion>>,
    },
}

pub(super) enum FlushTarget {
    Captured(i64),
    Outbox {
        cut: Option<i64>,
        minimum: SyncVersion,
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
        let resource = talos_sync_parser::resource(&self.binding, &resource);
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
        let resource = talos_sync_parser::resource(&self.binding, &resource);
        resource.validate().map_err(crate::facade::sync_error)?;
        self.require_connected()?;
        let source_cut = self.wait_for_capture(&resource).await?;
        let target = match source_cut {
            Some(0) => FlushTarget::Outbox {
                cut: None,
                minimum: SyncVersion::default(),
            },
            Some(sequence) => FlushTarget::Captured(sequence),
            None => {
                let (cut, minimum) = self.sync.flush_cut(&self.binding, &resource).await?;
                FlushTarget::Outbox { cut, minimum }
            }
        };
        let (reply, received) = oneshot::channel();
        self.request(
            ControlCommand::Flush {
                resource,
                target,
                reply,
            },
            received,
        )
        .await
    }

    async fn wait_for_capture(&self, resource: &SyncResourceKey) -> TalosResult<Option<i64>> {
        let binding = &self.binding;
        let rows = self.sync.storage.query(
            "SELECT MAX(revision) AS tail FROM resource_revisions WHERE kind=? AND resource_id=? AND owner_user_id IN (?,?)",
            &[resource.kind().as_str(), resource.id(), &binding.source_user_id, &binding.scope.account_id],
        ).await?;
        let Some(target) = rows.first().and_then(|row| row.get_i64("tail")) else {
            return Ok(None);
        };
        tokio::time::timeout(CONTROL_TIMEOUT, async {
            loop {
                self.require_connected()?;
                let progress = self.sync.storage.query(
                    "SELECT outbox_sequence FROM sync_resource_capture WHERE hub_id=? AND account_id=? AND kind=? AND resource_id=? AND revision=?",
                    &[&binding.scope.hub_id, &binding.scope.account_id, resource.kind().as_str(), resource.id(), &target.to_string()],
                ).await?;
                if let Some(sequence) = progress.first().and_then(|row| row.get_i64("outbox_sequence")) {
                    return Ok(Some(sequence));
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }).await.map_err(|_| TalosError::new(ErrorCode::Timeout, "Capture of committed resource facts timed out."))?
    }

    async fn request(
        &self,
        command: ControlCommand,
        reply: oneshot::Receiver<TalosResult<SyncVersion>>,
    ) -> TalosResult<SyncVersion> {
        self.require_connected()?;
        tokio::time::timeout(CONTROL_TIMEOUT, async {
            self.commands
                .send(command)
                .await
                .map_err(|_| unavailable())?;
            reply.await.map_err(|_| unavailable())?
        })
        .await
        .map_err(|_| TalosError::new(ErrorCode::Timeout, "Synchronization operation timed out."))?
    }

    fn require_connected(&self) -> TalosResult<()> {
        if self.stop.is_cancelled() || self.status().connection != SyncConnectionState::Connected {
            return Err(unavailable());
        }
        Ok(())
    }
}

fn unavailable() -> TalosError {
    TalosError::new(
        ErrorCode::ServiceUnavailable,
        "Synchronization connection is unavailable.",
    )
}
