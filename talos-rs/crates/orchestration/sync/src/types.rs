use std::time::Duration;
pub use talos_sync_parser::{
    ComputerAlias, ResourceKind, SessionExecutionBinding, SyncAcceptedCommand,
    SyncActiveTurnMetadata, SyncBinding, SyncChange, SyncEventKey, SyncFileSnapshot,
    SyncResourceKey, SyncResourceSnapshot, SyncResourceSummary, SyncSessionSnapshot,
    SyncTimelineEntry, SyncTurnMetadata,
};
pub use talos_sync_types::{SyncScope, SyncVersion};

/// Local configuration. Account credentials remain in the existing auth owner.
#[derive(Debug, Clone)]
pub struct SyncOptions {
    pub hub_endpoint: String,
}

/// Already-authorized source association supplied by the management host.
#[derive(Debug, Clone)]
pub struct SyncBindParams {
    pub scope: SyncScope,
    pub source_user_id: String,
    pub computers: Vec<ComputerAlias>,
}

/// Canonical facts to project into the existing outbox.
pub struct SyncCaptureParams {
    pub binding: SyncBinding,
    pub resource: SyncResourceKey,
    pub changes: Vec<SyncChange>,
    pub created_at: i64,
}

/// Encoded outbox rows for one validated resource and source binding.
pub struct PreparedSyncCapture {
    pub(crate) binding: SyncBinding,
    pub(crate) resource: SyncResourceKey,
    pub(crate) batches: Vec<String>,
    pub(crate) created_at: i64,
    pub(crate) stage: Option<crate::store::staged_write::StagedWrite>,
}

/// Pure projection of a business snapshot and the cursor mappings to commit.
#[derive(Default)]
pub struct SyncExportFacts {
    pub changes: Vec<SyncChange>,
    pub event_mappings: Vec<SyncEventMapping>,
}

/// Durable upload candidate; network delivery alone never removes this batch.
pub struct SyncExportBatch {
    pub scope: SyncScope,
    pub resource: SyncResourceKey,
    pub through_sequence: i64,
    pub update: Vec<u8>,
    pub version: SyncVersion,
}

/// A local cursor mapping owned by the same transaction as imported facts.
pub struct SyncEventMapping {
    pub scope: SyncScope,
    pub session_id: String,
    pub generation: u64,
    pub key: SyncEventKey,
    pub local_event_id: String,
    pub local_event_seq: i64,
}

/// One authenticated account connection. Credentials remain owned by original Auth.
pub struct SyncStartParams {
    pub binding: SyncBinding,
    pub credentials: talos_sync_sdk::CredentialCallback,
    pub trusted_ca_pem: Option<Vec<u8>>,
    /// Fixed reconnect interval, capped at 30 seconds; recovery continues until stopped.
    /// Authentication, permission and queue-full rejections wait at least 10 seconds.
    pub reconnect_delay: Duration,
}

pub struct SyncScanParams {
    pub binding: SyncBinding,
    pub cursor: Option<String>,
    pub limit: usize,
}

pub struct SyncScanPage {
    pub resources: Vec<SyncResourceKey>,
    pub next_cursor: Option<String>,
}

pub struct SyncExportParams {
    pub binding: SyncBinding,
    pub resource: SyncResourceKey,
}

/// Already validated shared facts, materialized by existing Store owners.
pub struct SyncImportBatch {
    pub scope: SyncScope,
    pub resource: SyncResourceKey,
    pub version: SyncVersion,
    pub changes: Vec<SyncChange>,
}

pub use talos_types::SyncConnectionState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncStatus {
    pub connection: SyncConnectionState,
    pub pending_changes: u64,
    pub last_acknowledged_at: Option<i64>,
    pub last_error: Option<talos_error::ApiError>,
}
