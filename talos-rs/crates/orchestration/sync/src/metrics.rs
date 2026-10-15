//! Read-only, bounded HTTPS adapter for the canonical authenticated Sync contract.
use crate::metrics_wire::AccountMetrics;
use std::time::Duration;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_sync_sdk::VerifiedCredential;
use talos_types::{SyncMetadataCounts, SyncMetricsResponse, SyncPeerMetrics};

const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

fn transport_error(error: reqwest::Error) -> TalosError {
    let code = if error.is_timeout() {
        ErrorCode::Timeout
    } else {
        ErrorCode::ServiceUnavailable
    };
    TalosError::from_source(
        code,
        "The synchronization metrics service is unavailable.",
        error.without_url(),
    )
}

fn metrics_url(endpoint: &str) -> TalosResult<url::Url> {
    let mut url = url::Url::parse(endpoint).map_err(|error| {
        TalosError::from_source(
            ErrorCode::InvalidRequest,
            "The synchronization endpoint is invalid.",
            error,
        )
    })?;
    if url.scheme() != "wss"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(TalosError::new(
            ErrorCode::InvalidRequest,
            "Synchronization metrics require a secure endpoint without credentials or selectors.",
        ));
    }
    url.set_scheme("https").map_err(|()| {
        TalosError::internal("The synchronization endpoint scheme could not be converted.")
    })?;
    url.set_path("/v1/metrics/account");
    Ok(url)
}

/// The management host supplies a freshly verified credential and configured trust.
/// This read never enables account sync, enrolls a Computer or follows redirects.
#[tracing::instrument(skip_all)]
pub async fn read_account_metrics(
    endpoint: &str,
    trusted_ca_pem: Option<&[u8]>,
    credential: VerifiedCredential,
) -> TalosResult<SyncMetricsResponse> {
    let url = metrics_url(endpoint)?;
    let origin = url.origin().ascii_serialization();
    let mut builder = reqwest::Client::builder()
        .use_rustls_tls()
        .no_proxy()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(5));
    if let Some(certificate) = trusted_ca_pem {
        builder = builder.add_root_certificate(
            reqwest::Certificate::from_pem(certificate).map_err(transport_error)?,
        );
    }
    let mut response = builder
        .build()
        .map_err(transport_error)?
        .get(url)
        .bearer_auth(&credential.access_token)
        .send()
        .await
        .map_err(transport_error)?;
    let status = response.status();
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(transport_error)? {
        if body.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
            return Err(TalosError::internal(
                "The synchronization metrics response exceeds its size limit.",
            ));
        }
        body.extend_from_slice(&chunk);
    }
    if !status.is_success() {
        if let Ok(error) = serde_json::from_slice::<talos_sync_types::SyncError>(&body) {
            return Err(crate::facade::sync_error(error));
        }
        return Err(TalosError::new(
            ErrorCode::ServiceUnavailable,
            "The synchronization metrics request was rejected.",
        ));
    }
    let account = serde_json::from_slice::<AccountMetrics>(&body).map_err(|error| {
        TalosError::internal_with_source(
            "The synchronization metrics response is invalid; technical details were withheld.",
            error,
        )
    })?;
    project(account, &credential.account_id, origin)
}

fn project(
    account: AccountMetrics,
    expected_account: &str,
    endpoint: String,
) -> TalosResult<SyncMetricsResponse> {
    if account.account_id != expected_account {
        return Err(TalosError::new(
            ErrorCode::PermissionDenied,
            "Synchronization metrics do not belong to the signed-in account.",
        ));
    }
    if account.api_version != 1 {
        return Err(TalosError::new(
            ErrorCode::ServiceUnavailable,
            "The synchronization service uses an unsupported metrics version.",
        ));
    }
    Ok(SyncMetricsResponse {
        endpoint,
        control_plane_peer_id: None,
        observed_at: account.observed_at / 1000,
        last_flushed_at: account.last_flushed_at.map(|value| value / 1000),
        complete: account.complete,
        metadata: SyncMetadataCounts {
            computers: account.metadata.computers,
            projects: account.metadata.projects,
            sessions: account.metadata.sessions,
            files: account.metadata.files,
        },
        peers: account
            .peers
            .into_iter()
            .map(|peer| SyncPeerMetrics {
                peer_id: peer.peer_id,
                computer_ids: peer.computer_ids,
                connections: peer.connections,
                document_rooms: peer.document_rooms,
                last_activity_at: peer.last_activity_at.map(|value| value / 1000),
                inbound_bytes_per_second: peer.inbound_bytes_per_second,
                outbound_bytes_per_second: peer.outbound_bytes_per_second,
                updates_per_second: peer.updates_per_second,
                commit_p95_ms: peer.commit_p95_ms,
            })
            .collect(),
    })
}

#[cfg(test)]
#[path = "metrics_tests.rs"]
mod tests;
