use super::*;
use pretty_assertions::assert_eq;
use talos_sync_sdk::metrics::{MetadataCounts, PeerMetrics};

fn account() -> AccountMetrics {
    AccountMetrics {
        api_version: 1,
        observed_at: 1_700_000_123_456,
        last_flushed_at: Some(1_700_000_120_123),
        complete: true,
        account_id: "account-a".into(),
        metadata: MetadataCounts {
            computers: 2,
            projects: 3,
            sessions: 4,
            files: 5,
        },
        peers: vec![PeerMetrics {
            peer_id: "peer-a".into(),
            computer_ids: vec!["computer-a".into()],
            connections: 2,
            document_rooms: 3,
            last_activity_at: Some(1_700_000_122_999),
            inbound_bytes_per_second: 10.5,
            outbound_bytes_per_second: 20.5,
            updates_per_second: 1.5,
            commit_p95_ms: Some(4.5),
        }],
    }
}

#[test]
fn projection_preserves_observations_and_converts_only_timestamps_to_seconds() {
    let result = project(account(), "https://sync.example".into());
    assert_eq!(
        result,
        SyncMetricsResponse {
            endpoint: "https://sync.example".into(),
            control_plane_peer_id: None,
            observed_at: 1_700_000_123,
            last_flushed_at: Some(1_700_000_120),
            complete: true,
            metadata: SyncMetadataCounts {
                computers: 2,
                projects: 3,
                sessions: 4,
                files: 5
            },
            peers: vec![SyncPeerMetrics {
                peer_id: "peer-a".into(),
                computer_ids: vec!["computer-a".into()],
                connections: 2,
                document_rooms: 3,
                last_activity_at: Some(1_700_000_122),
                inbound_bytes_per_second: 10.5,
                outbound_bytes_per_second: 20.5,
                updates_per_second: 1.5,
                commit_p95_ms: Some(4.5),
            }],
        }
    );
}
