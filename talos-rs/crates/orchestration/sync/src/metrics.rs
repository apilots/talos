//! Product projection of authenticated observations returned by the public Sync SDK.
use talos_error::TalosResult;
use talos_sync_sdk::{
    metrics::{AccountMetrics, ProgressState},
    MetricsClient, VerifiedCredential,
};
use talos_types::{
    SyncMetadataCounts, SyncMetricsResponse, SyncPeerMetrics, SyncPeerProgress, SyncProgressState,
    SyncSourceDelay,
};

/// The management host supplies a freshly verified credential and configured trust.
/// The public SDK owns HTTPS, request limits, account/version validation and errors.
#[tracing::instrument(skip_all)]
pub async fn read_account_metrics(
    endpoint: &str,
    trusted_ca_pem: Option<&[u8]>,
    credential: VerifiedCredential,
) -> TalosResult<SyncMetricsResponse> {
    let metrics =
        MetricsClient::new(endpoint, trusted_ca_pem).map_err(crate::facade::sync_error)?;
    let account = metrics
        .read_account(&credential)
        .await
        .map_err(crate::facade::sync_error)?;
    Ok(project(account, metrics.origin()))
}

fn project(account: AccountMetrics, endpoint: String) -> SyncMetricsResponse {
    let count = |namespace: &str| {
        account
            .metadata
            .by_namespace
            .get(namespace)
            .copied()
            .unwrap_or(0)
    };
    SyncMetricsResponse {
        endpoint,
        control_plane_peer_id: None,
        observed_at: account.observed_at / 1000,
        last_flushed_at: account.last_flushed_at.map(|value| value / 1000),
        complete: account.complete,
        metadata: SyncMetadataCounts {
            computers: count("computer"),
            projects: count("project"),
            sessions: count("session"),
            files: count("file"),
        },
        peers: account
            .peers
            .into_iter()
            .map(|peer| SyncPeerMetrics {
                peer_id: peer.peer_id,
                computer_ids: peer
                    .presence_keys
                    .into_iter()
                    .filter_map(
                        |resource| match crate::SyncResourceKey::try_from(resource) {
                            Ok(crate::SyncResourceKey::Computer { computer_id }) => {
                                Some(computer_id)
                            }
                            _ => None,
                        },
                    )
                    .collect(),
                connections: peer.connections,
                document_rooms: peer.document_rooms,
                last_activity_at: peer.last_activity_at.map(|value| value / 1000),
                last_heartbeat_at: peer.last_heartbeat_at.map(|value| value / 1000),
                progress: peer.progress.map(|sample| SyncPeerProgress {
                    observed_at: sample.observed_at / 1000,
                    last_applied_at: sample.last_applied_at.map(|value| value / 1000),
                    pending_uploads: sample.pending_uploads,
                    state: match sample.state {
                        ProgressState::CaughtUp => SyncProgressState::CaughtUp,
                        ProgressState::Behind => SyncProgressState::Behind,
                        ProgressState::Recovering => SyncProgressState::Recovering,
                    },
                    download_lag_ms: sample.download_lag_ms,
                    source_delays: sample
                        .source_delays
                        .into_iter()
                        .map(|delay| SyncSourceDelay {
                            source_peer_id: delay.source_peer_id,
                            observed_delay_ms: delay.observed_delay_ms,
                        })
                        .collect(),
                }),
                inbound_bytes_per_second: peer.inbound_bytes_per_second,
                outbound_bytes_per_second: peer.outbound_bytes_per_second,
                updates_per_second: peer.updates_per_second,
                commit_p95_ms: peer.commit_p95_ms,
            })
            .collect(),
    }
}

#[cfg(test)]
#[path = "metrics_tests.rs"]
mod tests;
